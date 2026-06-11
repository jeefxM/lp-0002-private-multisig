//! LP-0002 unified `msig` program (v0.1.2 / live testnet rev).
//!
//! Two instructions sharing one program so the ProposalState it creates is also owned by the
//! program that mutates it on approval:
//!   - `CreateProposal`: claim + freeze a ProposalState with the proposal's `member_root`.
//!   - `Approve`: anonymous Semaphore-style approval — membership proof + proposal-bound nullifier,
//!     with the member secret carried as a private witness (never committed).
use msig_core::{
    MerkleProof, MsigInstruction, PROPOSAL_HEADER_LEN, REGISTRY_HEADER_LEN, member_leaf,
    merkle_root, root_from_path, vote_nullifier,
};
use nssa_core::{
    account::AccountWithMetadata,
    program::{
        AccountPostState, ChainedCall, Claim, DEFAULT_PROGRAM_ID, PdaSeed, ProgramInput,
        ProgramOutput, read_nssa_inputs,
    },
};

/// Claim a fresh public ProposalState and freeze `member_root` + `proposal_id` (count 0).
fn create_proposal(
    proposal: AccountWithMetadata,
    member_root: [u8; 32],
    proposal_id: [u8; 32],
) -> Vec<AccountPostState> {
    let mut data = Vec::with_capacity(PROPOSAL_HEADER_LEN);
    data.extend_from_slice(&member_root);
    data.extend_from_slice(&proposal_id);
    data.extend_from_slice(&0_u32.to_le_bytes());

    let mut account = proposal.account;
    account.data = data.try_into().expect("proposal state fits into data limit");
    // The account must be fresh; claiming an initialized account is rejected by the circuit.
    vec![AccountPostState::new_claimed(account, Claim::Authorized)]
}

/// Append a member's public leaf to the MembersRegistry (a program-owned public account) and
/// recompute `member_root`. A plain public transaction — no ZK, no rider. Only the leaf
/// (= H(secret)) is published; the member secret stays off-chain.
fn enroll(registry: AccountWithMetadata, leaf: [u8; 32]) -> Vec<AccountPostState> {
    let data = registry.account.data.clone().into_inner();
    let mut leaves: Vec<[u8; 32]> = Vec::new();
    if data.len() >= REGISTRY_HEADER_LEN {
        let count = u32::from_le_bytes(data[32..REGISTRY_HEADER_LEN].try_into().unwrap()) as usize;
        let mut offset = REGISTRY_HEADER_LEN;
        for _ in 0..count {
            let end = offset + 32;
            leaves.push(data[offset..end].try_into().expect("registry leaf truncated"));
            offset = end;
        }
    }
    leaves.push(leaf);
    let root = merkle_root(&leaves);

    let mut new_data = Vec::with_capacity(REGISTRY_HEADER_LEN + leaves.len() * 32);
    new_data.extend_from_slice(&root);
    new_data.extend_from_slice(&(leaves.len() as u32).to_le_bytes());
    for l in &leaves {
        new_data.extend_from_slice(l);
    }

    let mut account = registry.account.clone();
    account.data = new_data.try_into().expect("registry should fit into data limit");

    if registry.account.program_owner == DEFAULT_PROGRAM_ID {
        vec![AccountPostState::new_claimed(account, Claim::Authorized)]
    } else {
        vec![AccountPostState::new(account)]
    }
}

/// Anonymous approval. ProposalState is public (mask 0); rider is a fresh private account (mask 2)
/// emitting the commitment/nullifier the privacy tx requires.
fn approve(
    proposal: AccountWithMetadata,
    rider: AccountWithMetadata,
    secret: [u8; 32],
    merkle_path: MerkleProof,
    proposal_id: [u8; 32],
) -> Vec<AccountPostState> {
    let data = proposal.account.data.clone().into_inner();
    assert!(data.len() >= PROPOSAL_HEADER_LEN, "proposal state header too short");
    let member_root: [u8; 32] = data[..32].try_into().unwrap();
    let proposal_id_state: [u8; 32] = data[32..64].try_into().unwrap();
    let count = u32::from_le_bytes(data[64..PROPOSAL_HEADER_LEN].try_into().unwrap());

    assert_eq!(proposal_id, proposal_id_state, "proposal id mismatch");

    // MEMBERSHIP: the approver's leaf is in member_root, without revealing which leaf.
    let leaf = member_leaf(&secret);
    assert_eq!(
        root_from_path(leaf, &merkle_path),
        member_root,
        "approver is not an enrolled member"
    );

    // Proposal-bound nullifier + NO DOUBLE VOTE.
    let nullifier = vote_nullifier(&secret, &proposal_id);
    let mut nullifiers: Vec<[u8; 32]> = Vec::with_capacity(count as usize);
    let mut offset = PROPOSAL_HEADER_LEN;
    for _ in 0..count {
        let end = offset + 32;
        nullifiers.push(data[offset..end].try_into().expect("nullifier set truncated"));
        offset = end;
    }
    assert!(
        !nullifiers.contains(&nullifier),
        "approval nullifier already recorded (double vote)"
    );
    nullifiers.push(nullifier);
    let new_count = count + 1;

    let mut new_data = Vec::with_capacity(PROPOSAL_HEADER_LEN + nullifiers.len() * 32);
    new_data.extend_from_slice(&member_root);
    new_data.extend_from_slice(&proposal_id_state);
    new_data.extend_from_slice(&new_count.to_le_bytes());
    for n in &nullifiers {
        new_data.extend_from_slice(n);
    }

    let mut proposal_post = proposal.account.clone();
    proposal_post.data = new_data
        .try_into()
        .expect("proposal state should fit into data limit");

    let rider_post = if rider.account.program_owner == DEFAULT_PROGRAM_ID {
        AccountPostState::new_claimed(rider.account, Claim::Authorized)
    } else {
        AccountPostState::new(rider.account)
    };

    vec![AccountPostState::new(proposal_post), rider_post]
}

/// Threshold-gated treasury release. pre_states = [proposal, treasury, recipient].
/// proposal = ProposalState (read approval_count). treasury = msig PDA owned by the transfer
/// program. recipient = payout target. Drains the full treasury balance to recipient.
fn execute(
    proposal: AccountWithMetadata,
    treasury: AccountWithMetadata,
    recipient: AccountWithMetadata,
    threshold: u32,
    seed: [u8; 32],
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let data = proposal.account.data.clone().into_inner();
    assert!(data.len() >= PROPOSAL_HEADER_LEN, "proposal state header too short");
    let count = u32::from_le_bytes(data[64..PROPOSAL_HEADER_LEN].try_into().unwrap());
    assert!(count >= threshold, "approval count below threshold");

    let amount = treasury.account.balance;
    let transfer_program_id = treasury.account.program_owner;

    let mut treasury_authorized = treasury.clone();
    treasury_authorized.is_authorized = true;

    let chained_call = ChainedCall {
        program_id: transfer_program_id,
        pre_states: vec![treasury_authorized, recipient.clone()],
        instruction_data: risc0_zkvm::serde::to_vec(&amount).unwrap(),
        pda_seeds: vec![PdaSeed::new(seed)],
    };

    // Execute mutates none of its own accounts; the chained call performs the debit/credit.
    let post = vec![
        AccountPostState::new(proposal.account),
        AccountPostState::new(treasury.account),
        AccountPostState::new(recipient.account),
    ];
    (post, vec![chained_call])
}

/// Bootstrap the treasury PDA. pre_states = [treasury], where `treasury` is the fresh
/// `for_public_pda(self_program_id, seed)` account (default-owned, balance 0). We authorize it via
/// `pda_seeds = [seed]` and chain to `authenticated_transfer`'s amount-0 initialize, which claims
/// it `Authorized` — leaving the treasury `authenticated_transfer`-owned (balance 0). msig itself
/// mutates nothing; the chained call performs the claim.
fn init_treasury(
    treasury: AccountWithMetadata,
    seed: [u8; 32],
    transfer_program_id: [u32; 8],
) -> (Vec<AccountPostState>, Vec<ChainedCall>) {
    let mut treasury_authorized = treasury.clone();
    treasury_authorized.is_authorized = true;

    let chained_call = ChainedCall {
        program_id: transfer_program_id,
        pre_states: vec![treasury_authorized],
        // amount-0 → authenticated_transfer's `initialize_account` path.
        instruction_data: risc0_zkvm::serde::to_vec(&0_u128).unwrap(),
        pda_seeds: vec![PdaSeed::new(seed)],
    };

    // msig mutates none of its own accounts; the chained call claims/initializes the treasury.
    let post = vec![AccountPostState::new(treasury.account)];
    (post, vec![chained_call])
}

fn main() {
    let (
        ProgramInput {
            self_program_id,
            caller_program_id,
            pre_states,
            instruction,
        },
        instruction_words,
    ) = read_nssa_inputs::<MsigInstruction>();

    let post_states = match instruction {
        MsigInstruction::CreateProposal {
            member_root,
            proposal_id,
        } => {
            let Ok([proposal]) = <[_; 1]>::try_from(pre_states.clone()) else {
                return;
            };
            create_proposal(proposal, member_root, proposal_id)
        }
        MsigInstruction::Approve {
            secret,
            merkle_path,
            proposal_id,
        } => {
            let Ok([proposal, rider]) = <[_; 2]>::try_from(pre_states.clone()) else {
                return;
            };
            approve(proposal, rider, secret, merkle_path, proposal_id)
        }
        MsigInstruction::Enroll { leaf } => {
            let Ok([registry]) = <[_; 1]>::try_from(pre_states.clone()) else {
                return;
            };
            enroll(registry, leaf)
        }
        MsigInstruction::InitTreasury {
            seed,
            transfer_program_id,
        } => {
            let Ok([treasury]) = <[_; 1]>::try_from(pre_states.clone()) else {
                return;
            };
            let (post_states, chained_calls) =
                init_treasury(treasury, seed, transfer_program_id);
            ProgramOutput::new(
                self_program_id,
                caller_program_id,
                instruction_words,
                pre_states,
                post_states,
            )
            .with_chained_calls(chained_calls)
            .write();
            return;
        }
        MsigInstruction::Execute { threshold, seed } => {
            let Ok([proposal, treasury, recipient]) = <[_; 3]>::try_from(pre_states.clone()) else {
                return;
            };
            let (post_states, chained_calls) =
                execute(proposal, treasury, recipient, threshold, seed);
            ProgramOutput::new(
                self_program_id,
                caller_program_id,
                instruction_words,
                pre_states,
                post_states,
            )
            .with_chained_calls(chained_calls)
            .write();
            return;
        }
    };

    ProgramOutput::new(
        self_program_id,
        caller_program_id,
        instruction_words,
        pre_states,
        post_states,
    )
    .write();
}
