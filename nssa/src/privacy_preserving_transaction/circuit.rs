use std::collections::{HashMap, VecDeque};

use borsh::{BorshDeserialize, BorshSerialize};
use nssa_core::{
    MembershipProof, NullifierPublicKey, NullifierSecretKey, PrivacyPreservingCircuitInput,
    PrivacyPreservingCircuitOutput, SharedSecretKey,
    account::AccountWithMetadata,
    program::{ChainedCall, InstructionData, ProgramId, ProgramOutput},
};
use risc0_zkvm::{ExecutorEnv, InnerReceipt, ProverOpts, Receipt, default_prover};

use crate::{
    error::{InvalidProgramBehaviorError, NssaError},
    program::Program,
    program_methods::{PRIVACY_PRESERVING_CIRCUIT_ELF, PRIVACY_PRESERVING_CIRCUIT_ID},
    state::MAX_NUMBER_CHAINED_CALLS,
};

/// Proof of the privacy preserving execution circuit.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Proof(pub(crate) Vec<u8>);

impl Proof {
    #[must_use]
    pub fn into_inner(self) -> Vec<u8> {
        self.0
    }

    #[must_use]
    pub const fn from_inner(inner: Vec<u8>) -> Self {
        Self(inner)
    }

    pub(crate) fn is_valid_for(&self, circuit_output: &PrivacyPreservingCircuitOutput) -> bool {
        let inner: InnerReceipt = borsh::from_slice(&self.0).unwrap();
        let receipt = Receipt::new(inner, circuit_output.to_bytes());
        receipt.verify(PRIVACY_PRESERVING_CIRCUIT_ID).is_ok()
    }
}

#[derive(Clone)]
pub struct ProgramWithDependencies {
    pub program: Program,
    // TODO: avoid having a copy of the bytecode of each dependency.
    pub dependencies: HashMap<ProgramId, Program>,
}

impl ProgramWithDependencies {
    #[must_use]
    pub const fn new(program: Program, dependencies: HashMap<ProgramId, Program>) -> Self {
        Self {
            program,
            dependencies,
        }
    }
}

impl From<Program> for ProgramWithDependencies {
    fn from(program: Program) -> Self {
        Self::new(program, HashMap::new())
    }
}

/// Generates a proof of the execution of a NSSA program inside the privacy preserving execution
/// circuit.
/// TODO: too many parameters.
pub fn execute_and_prove(
    pre_states: Vec<AccountWithMetadata>,
    instruction_data: InstructionData,
    visibility_mask: Vec<u8>,
    private_account_keys: Vec<(NullifierPublicKey, SharedSecretKey)>,
    private_account_nsks: Vec<NullifierSecretKey>,
    private_account_membership_proofs: Vec<Option<MembershipProof>>,
    program_with_dependencies: &ProgramWithDependencies,
) -> Result<(PrivacyPreservingCircuitOutput, Proof), NssaError> {
    let ProgramWithDependencies {
        program: initial_program,
        dependencies,
    } = program_with_dependencies;
    let mut env_builder = ExecutorEnv::builder();
    let mut program_outputs = Vec::new();

    let initial_call = ChainedCall {
        program_id: initial_program.id(),
        instruction_data,
        pre_states,
        pda_seeds: vec![],
    };

    let mut chained_calls = VecDeque::from_iter([(initial_call, initial_program, None)]);
    let mut chain_calls_counter = 0;
    while let Some((chained_call, program, caller_program_id)) = chained_calls.pop_front() {
        if chain_calls_counter >= MAX_NUMBER_CHAINED_CALLS {
            return Err(NssaError::MaxChainedCallsDepthExceeded);
        }

        let inner_receipt = execute_and_prove_program(
            program,
            caller_program_id,
            &chained_call.pre_states,
            &chained_call.instruction_data,
        )?;

        let program_output: ProgramOutput = inner_receipt
            .journal
            .decode()
            .map_err(|e| NssaError::ProgramOutputDeserializationError(e.to_string()))?;

        // TODO: remove clone
        program_outputs.push(program_output.clone());

        // Prove circuit.
        env_builder.add_assumption(inner_receipt);

        for new_call in program_output.chained_calls.into_iter().rev() {
            let next_program = dependencies.get(&new_call.program_id).ok_or(
                InvalidProgramBehaviorError::UndeclaredProgramDependency {
                    program_id: new_call.program_id,
                },
            )?;
            chained_calls.push_front((new_call, next_program, Some(chained_call.program_id)));
        }

        chain_calls_counter = chain_calls_counter
            .checked_add(1)
            .expect("we check the max depth at the beginning of the loop");
    }

    let circuit_input = PrivacyPreservingCircuitInput {
        program_outputs,
        visibility_mask,
        private_account_keys,
        private_account_nsks,
        private_account_membership_proofs,
        program_id: program_with_dependencies.program.id(),
    };

    env_builder.write(&circuit_input).unwrap();
    let env = env_builder.build().unwrap();
    let prover = default_prover();
    let opts = ProverOpts::succinct();
    let prove_info = prover
        .prove_with_opts(env, PRIVACY_PRESERVING_CIRCUIT_ELF, &opts)
        .map_err(|e| NssaError::CircuitProvingError(e.to_string()))?;

    let proof = Proof(borsh::to_vec(&prove_info.receipt.inner)?);

    let circuit_output: PrivacyPreservingCircuitOutput = prove_info
        .receipt
        .journal
        .decode()
        .map_err(|e| NssaError::CircuitOutputDeserializationError(e.to_string()))?;

    Ok((circuit_output, proof))
}

fn execute_and_prove_program(
    program: &Program,
    caller_program_id: Option<ProgramId>,
    pre_states: &[AccountWithMetadata],
    instruction_data: &InstructionData,
) -> Result<Receipt, NssaError> {
    // Write inputs to the program
    let mut env_builder = ExecutorEnv::builder();
    Program::write_inputs(
        program.id(),
        caller_program_id,
        pre_states,
        instruction_data,
        &mut env_builder,
    )?;
    let env = env_builder.build().unwrap();

    // Prove the program
    let prover = default_prover();
    Ok(prover
        .prove(env, program.elf())
        .map_err(|e| NssaError::ProgramProveFailed(e.to_string()))?
        .receipt)
}

#[cfg(test)]
mod tests {
    #![expect(clippy::shadow_unrelated, reason = "We don't care about it in tests")]

    use nssa_core::{
        Commitment, DUMMY_COMMITMENT_HASH, EncryptionScheme, Nullifier, SharedSecretKey,
        account::{Account, AccountId, AccountWithMetadata, Nonce, data::Data},
    };

    use super::*;
    use crate::{
        error::NssaError,
        privacy_preserving_transaction::circuit::execute_and_prove,
        program::Program,
        state::{
            CommitmentSet,
            tests::{test_private_account_keys_1, test_private_account_keys_2},
        },
    };

    #[test]
    fn prove_privacy_preserving_execution_circuit_public_and_private_pre_accounts() {
        let recipient_keys = test_private_account_keys_1();
        let program = Program::authenticated_transfer_program();
        let sender = AccountWithMetadata::new(
            Account {
                program_owner: program.id(),
                balance: 100,
                ..Account::default()
            },
            true,
            AccountId::new([0; 32]),
        );

        let recipient = AccountWithMetadata::new(
            Account::default(),
            false,
            AccountId::from(&recipient_keys.npk()),
        );

        let balance_to_move: u128 = 37;

        let expected_sender_post = Account {
            program_owner: program.id(),
            balance: 100 - balance_to_move,
            nonce: Nonce::default(),
            data: Data::default(),
        };

        let expected_recipient_post = Account {
            program_owner: program.id(),
            balance: balance_to_move,
            nonce: Nonce::private_account_nonce_init(&recipient_keys.npk()),
            data: Data::default(),
        };

        let expected_sender_pre = sender.clone();

        let esk = [3; 32];
        let shared_secret = SharedSecretKey::new(&esk, &recipient_keys.vpk());

        let (output, proof) = execute_and_prove(
            vec![sender, recipient],
            Program::serialize_instruction(balance_to_move).unwrap(),
            vec![0, 2],
            vec![(recipient_keys.npk(), shared_secret)],
            vec![],
            vec![None],
            &Program::authenticated_transfer_program().into(),
        )
        .unwrap();

        assert!(proof.is_valid_for(&output));

        let [sender_pre] = output.public_pre_states.try_into().unwrap();
        let [sender_post] = output.public_post_states.try_into().unwrap();
        assert_eq!(sender_pre, expected_sender_pre);
        assert_eq!(sender_post, expected_sender_post);
        assert_eq!(output.new_commitments.len(), 1);
        assert_eq!(output.new_nullifiers.len(), 1);
        assert_eq!(output.ciphertexts.len(), 1);

        let recipient_post = EncryptionScheme::decrypt(
            &output.ciphertexts[0],
            &shared_secret,
            &output.new_commitments[0],
            0,
        )
        .unwrap();
        assert_eq!(recipient_post, expected_recipient_post);
    }

    /// LP-0002 (v0.1.2) anonymous M-of-N approval: a privacy-preserving transaction mutates a
    /// PUBLIC ProposalState (mask 0) while the member secret + Merkle path + proposal_id travel as
    /// a PRIVATE instruction witness (never committed). The guest verifies in-guest Merkle
    /// membership against the snapshotted member_root, derives a proposal-bound vote nullifier,
    /// rejects double-votes, and increments the count. A fresh private rider (mask 2) emits the
    /// commitment/nullifier the privacy tx requires. The voter stays anonymous.
    #[test]
    fn msig_approve_anonymous_membership() {
        use risc0_zkvm::sha::{Impl, Sha256 as _};

        let program = Program::msig();
        let voter_keys = test_private_account_keys_1();

        const LEAF_DOMAIN: &[u8] = b"/lp0002/leaf/\x00";
        const NULL_DOMAIN: &[u8] = b"/lp0002/null/\x00";
        let sha256 = |b: &[u8]| -> [u8; 32] { Impl::hash_bytes(b).as_bytes().try_into().unwrap() };
        let leaf_of = |s: &[u8; 32]| -> [u8; 32] { sha256(&[LEAF_DOMAIN, s.as_slice()].concat()) };

        // Two-member set; approver holds secret_a (leaf index 0), leaf_b is the sibling.
        let secret_a = [0xA7u8; 32];
        let secret_b = [0x42u8; 32];
        let leaf_a = leaf_of(&secret_a);
        let leaf_b = leaf_of(&secret_b);
        let member_root = sha256(&[leaf_a, leaf_b].concat());
        let merkle_path: (u32, Vec<[u8; 32]>) = (0, vec![leaf_b]);
        let proposal_id = [0x11u8; 32];

        // Public ProposalState (program-owned): member_root || proposal_id || count(0).
        let mut initial = Vec::with_capacity(68);
        initial.extend_from_slice(&member_root);
        initial.extend_from_slice(&proposal_id);
        initial.extend_from_slice(&0_u32.to_le_bytes());
        let proposal = AccountWithMetadata::new(
            Account {
                program_owner: program.id(),
                balance: 0,
                data: Data::try_from(initial).unwrap(),
                ..Account::default()
            },
            true,
            AccountId::new([9; 32]),
        );

        // Fresh private rider (mask 2) at the voter's npk — the "vote note" emitting the commitment.
        let rider = AccountWithMetadata::new(
            Account::default(),
            false,
            AccountId::from(&voter_keys.npk()),
        );
        let esk = [3u8; 32];
        let rider_ssk = SharedSecretKey::new(&esk, &voter_keys.vpk());

        let (output, proof) = execute_and_prove(
            vec![proposal, rider],
            Program::serialize_instruction(msig_core::MsigInstruction::Approve { secret: secret_a, merkle_path, proposal_id }).unwrap(),
            vec![0, 2],
            vec![(voter_keys.npk(), rider_ssk)],
            vec![],
            vec![None],
            &program.clone().into(),
        )
        .unwrap();

        assert!(proof.is_valid_for(&output));

        // ProposalState: root + proposal id preserved, count incremented, vote nullifier recorded.
        let [ps_post] = output.public_post_states.try_into().unwrap();
        let d = ps_post.data.clone().into_inner();
        assert_eq!(&d[..32], &member_root);
        assert_eq!(&d[32..64], &proposal_id);
        assert_eq!(u32::from_le_bytes(d[64..68].try_into().unwrap()), 1);
        let expected_null =
            sha256(&[NULL_DOMAIN, secret_a.as_slice(), proposal_id.as_slice()].concat());
        assert_eq!(&d[68..100], &expected_null);

        // Rider emitted the required commitment + nullifier (non-empty-output guard satisfied).
        assert_eq!(output.new_commitments.len(), 1);
        assert_eq!(output.new_nullifiers.len(), 1);
    }

    /// A secret whose leaf is not in member_root cannot approve (in-guest membership fails → no
    /// valid proof). This is what makes it an M-of-N multisig, not a public counter.
    #[test]
    fn msig_approve_rejects_non_member() {
        use risc0_zkvm::sha::{Impl, Sha256 as _};

        let program = Program::msig();
        let voter_keys = test_private_account_keys_1();
        const LEAF_DOMAIN: &[u8] = b"/lp0002/leaf/\x00";
        let sha256 = |b: &[u8]| -> [u8; 32] { Impl::hash_bytes(b).as_bytes().try_into().unwrap() };
        let leaf_of = |s: &[u8; 32]| -> [u8; 32] { sha256(&[LEAF_DOMAIN, s.as_slice()].concat()) };

        let leaf_a = leaf_of(&[0xA7u8; 32]);
        let leaf_b = leaf_of(&[0x42u8; 32]);
        let member_root = sha256(&[leaf_a, leaf_b].concat());
        let proposal_id = [0x11u8; 32];

        let mut initial = Vec::with_capacity(68);
        initial.extend_from_slice(&member_root);
        initial.extend_from_slice(&proposal_id);
        initial.extend_from_slice(&0_u32.to_le_bytes());
        let proposal = AccountWithMetadata::new(
            Account {
                program_owner: program.id(),
                balance: 0,
                data: Data::try_from(initial).unwrap(),
                ..Account::default()
            },
            true,
            AccountId::new([9; 32]),
        );
        let rider = AccountWithMetadata::new(
            Account::default(),
            false,
            AccountId::from(&voter_keys.npk()),
        );
        let rider_ssk = SharedSecretKey::new(&[3u8; 32], &voter_keys.vpk());

        // secret_x is not enrolled; presenting leaf_b as sibling cannot reproduce member_root.
        let secret_x = [0xFFu8; 32];
        let merkle_path: (u32, Vec<[u8; 32]>) = (0, vec![leaf_b]);

        let result = execute_and_prove(
            vec![proposal, rider],
            Program::serialize_instruction(msig_core::MsigInstruction::Approve { secret: secret_x, merkle_path, proposal_id }).unwrap(),
            vec![0, 2],
            vec![(voter_keys.npk(), rider_ssk)],
            vec![],
            vec![None],
            &program.clone().into(),
        );
        assert!(result.is_err(), "non-member approval must be rejected");
    }

    /// The same member cannot approve the same proposal twice: the proposal-bound nullifier is
    /// already recorded, so the in-guest double-vote check fails.
    #[test]
    fn msig_approve_rejects_double_vote() {
        use risc0_zkvm::sha::{Impl, Sha256 as _};

        let program = Program::msig();
        let voter_keys = test_private_account_keys_1();
        const LEAF_DOMAIN: &[u8] = b"/lp0002/leaf/\x00";
        const NULL_DOMAIN: &[u8] = b"/lp0002/null/\x00";
        let sha256 = |b: &[u8]| -> [u8; 32] { Impl::hash_bytes(b).as_bytes().try_into().unwrap() };
        let leaf_of = |s: &[u8; 32]| -> [u8; 32] { sha256(&[LEAF_DOMAIN, s.as_slice()].concat()) };

        let secret_a = [0xA7u8; 32];
        let leaf_a = leaf_of(&secret_a);
        let leaf_b = leaf_of(&[0x42u8; 32]);
        let member_root = sha256(&[leaf_a, leaf_b].concat());
        let proposal_id = [0x11u8; 32];
        let nullifier =
            sha256(&[NULL_DOMAIN, secret_a.as_slice(), proposal_id.as_slice()].concat());

        // ProposalState already records secret_a's nullifier (count = 1).
        let mut initial = Vec::with_capacity(100);
        initial.extend_from_slice(&member_root);
        initial.extend_from_slice(&proposal_id);
        initial.extend_from_slice(&1_u32.to_le_bytes());
        initial.extend_from_slice(&nullifier);
        let proposal = AccountWithMetadata::new(
            Account {
                program_owner: program.id(),
                balance: 0,
                data: Data::try_from(initial).unwrap(),
                ..Account::default()
            },
            true,
            AccountId::new([9; 32]),
        );
        let rider = AccountWithMetadata::new(
            Account::default(),
            false,
            AccountId::from(&voter_keys.npk()),
        );
        let rider_ssk = SharedSecretKey::new(&[3u8; 32], &voter_keys.vpk());
        let merkle_path: (u32, Vec<[u8; 32]>) = (0, vec![leaf_b]);

        let result = execute_and_prove(
            vec![proposal, rider],
            Program::serialize_instruction(msig_core::MsigInstruction::Approve { secret: secret_a, merkle_path, proposal_id }).unwrap(),
            vec![0, 2],
            vec![(voter_keys.npk(), rider_ssk)],
            vec![],
            vec![None],
            &program.clone().into(),
        );
        assert!(result.is_err(), "double vote must be rejected");
    }

    /// Members enroll by appending their public leaf to the registry; the program recomputes
    /// member_root over all leaves. A plain public tx — only H(secret) is published.
    #[test]
    fn msig_enroll_appends_member() {
        let program = Program::msig();
        let leaf1 = msig_core::member_leaf(&[0xA7u8; 32]);
        let leaf2 = msig_core::member_leaf(&[0x42u8; 32]);

        // First enroll: registry starts as a fresh default public account.
        let registry0 = AccountWithMetadata::new(Account::default(), true, AccountId::new([8; 32]));
        let (out1, _proof) = execute_and_prove(
            vec![registry0],
            Program::serialize_instruction(msig_core::MsigInstruction::Enroll { leaf: leaf1 })
                .unwrap(),
            vec![0],
            vec![],
            vec![],
            vec![],
            &program.clone().into(),
        )
        .unwrap();
        let [reg1] = out1.public_post_states.try_into().unwrap();
        let d1 = reg1.data.clone().into_inner();
        assert_eq!(u32::from_le_bytes(d1[32..36].try_into().unwrap()), 1);
        assert_eq!(&d1[..32], &msig_core::merkle_root(&[leaf1]));

        // Second enroll: registry is now program-owned and already holds leaf1.
        let registry1 = AccountWithMetadata::new(
            Account {
                program_owner: program.id(),
                data: Data::try_from(d1).unwrap(),
                ..Account::default()
            },
            true,
            AccountId::new([8; 32]),
        );
        let (out2, _proof) = execute_and_prove(
            vec![registry1],
            Program::serialize_instruction(msig_core::MsigInstruction::Enroll { leaf: leaf2 })
                .unwrap(),
            vec![0],
            vec![],
            vec![],
            vec![],
            &program.clone().into(),
        )
        .unwrap();
        let [reg2] = out2.public_post_states.try_into().unwrap();
        let d2 = reg2.data.clone().into_inner();
        assert_eq!(u32::from_le_bytes(d2[32..36].try_into().unwrap()), 2);
        assert_eq!(&d2[..32], &msig_core::merkle_root(&[leaf1, leaf2]));
    }

    #[test]
    fn prove_privacy_preserving_execution_circuit_fully_private() {
        let program = Program::authenticated_transfer_program();
        let sender_keys = test_private_account_keys_1();
        let recipient_keys = test_private_account_keys_2();

        let sender_nonce = Nonce(0xdead_beef);
        let sender_pre = AccountWithMetadata::new(
            Account {
                balance: 100,
                nonce: sender_nonce,
                program_owner: program.id(),
                data: Data::default(),
            },
            true,
            AccountId::from(&sender_keys.npk()),
        );
        let commitment_sender = Commitment::new(&sender_keys.npk(), &sender_pre.account);

        let recipient = AccountWithMetadata::new(
            Account::default(),
            false,
            AccountId::from(&recipient_keys.npk()),
        );
        let balance_to_move: u128 = 37;

        let mut commitment_set = CommitmentSet::with_capacity(2);
        commitment_set.extend(std::slice::from_ref(&commitment_sender));

        let expected_new_nullifiers = vec![
            (
                Nullifier::for_account_update(&commitment_sender, &sender_keys.nsk),
                commitment_set.digest(),
            ),
            (
                Nullifier::for_account_initialization(&recipient_keys.npk()),
                DUMMY_COMMITMENT_HASH,
            ),
        ];

        let program = Program::authenticated_transfer_program();

        let expected_private_account_1 = Account {
            program_owner: program.id(),
            balance: 100 - balance_to_move,
            nonce: sender_nonce.private_account_nonce_increment(&sender_keys.nsk),
            ..Default::default()
        };
        let expected_private_account_2 = Account {
            program_owner: program.id(),
            balance: balance_to_move,
            nonce: Nonce::private_account_nonce_init(&recipient_keys.npk()),
            ..Default::default()
        };
        let expected_new_commitments = vec![
            Commitment::new(&sender_keys.npk(), &expected_private_account_1),
            Commitment::new(&recipient_keys.npk(), &expected_private_account_2),
        ];

        let esk_1 = [3; 32];
        let shared_secret_1 = SharedSecretKey::new(&esk_1, &sender_keys.vpk());

        let esk_2 = [5; 32];
        let shared_secret_2 = SharedSecretKey::new(&esk_2, &recipient_keys.vpk());

        let (output, proof) = execute_and_prove(
            vec![sender_pre, recipient],
            Program::serialize_instruction(balance_to_move).unwrap(),
            vec![1, 2],
            vec![
                (sender_keys.npk(), shared_secret_1),
                (recipient_keys.npk(), shared_secret_2),
            ],
            vec![sender_keys.nsk],
            vec![commitment_set.get_proof_for(&commitment_sender), None],
            &program.clone().into(),
        )
        .unwrap();

        assert!(proof.is_valid_for(&output));
        assert!(output.public_pre_states.is_empty());
        assert!(output.public_post_states.is_empty());
        assert_eq!(output.new_commitments, expected_new_commitments);
        assert_eq!(output.new_nullifiers, expected_new_nullifiers);
        assert_eq!(output.ciphertexts.len(), 2);

        let sender_post = EncryptionScheme::decrypt(
            &output.ciphertexts[0],
            &shared_secret_1,
            &expected_new_commitments[0],
            0,
        )
        .unwrap();
        assert_eq!(sender_post, expected_private_account_1);

        let recipient_post = EncryptionScheme::decrypt(
            &output.ciphertexts[1],
            &shared_secret_2,
            &expected_new_commitments[1],
            1,
        )
        .unwrap();
        assert_eq!(recipient_post, expected_private_account_2);
    }

    #[test]
    fn circuit_fails_when_chained_validity_windows_have_empty_intersection() {
        let account_keys = test_private_account_keys_1();
        let pre = AccountWithMetadata::new(
            Account::default(),
            false,
            AccountId::from(&account_keys.npk()),
        );

        let validity_window_chain_caller = Program::validity_window_chain_caller();
        let validity_window = Program::validity_window();

        let instruction = Program::serialize_instruction((
            Some(1_u64),
            Some(4_u64),
            validity_window.id(),
            Some(4_u64),
            Some(7_u64),
        ))
        .unwrap();

        let esk = [3; 32];
        let shared_secret = SharedSecretKey::new(&esk, &account_keys.vpk());

        let program_with_deps = ProgramWithDependencies::new(
            validity_window_chain_caller,
            [(validity_window.id(), validity_window)].into(),
        );

        let result = execute_and_prove(
            vec![pre],
            instruction,
            vec![2],
            vec![(account_keys.npk(), shared_secret)],
            vec![],
            vec![None],
            &program_with_deps,
        );

        assert!(matches!(result, Err(NssaError::CircuitProvingError(_))));
    }
}
