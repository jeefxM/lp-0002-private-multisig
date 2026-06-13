//! TASK C runner 5 — PRIVACY tx: anonymous M-of-N approval (the hard one).
//!
//! A privacy-preserving transaction mutates a PUBLIC ProposalState (mask 0) while the member secret
//! + Merkle path + proposal_id travel as a PRIVATE instruction witness. The guest verifies in-guest
//! Merkle membership against the snapshotted member_root, derives a proposal-bound vote nullifier,
//! rejects double-votes, and increments the count. A fresh private rider (mask 2) emits the
//! commitment/nullifier the privacy tx requires. The voter stays anonymous.
//!
//! Membership inputs come from the shared [`msig_demo`] fixture: [`msig_demo::approver_secret`] and
//! its depth-5 [`msig_demo::approver_path`] against the SAME [`msig_demo::member_root`] that
//! `create_proposal` freezes. The ProposalState pre_state is the SAME unified account id
//! ([`msig_demo::proposal_account_id`]) that `create_proposal` claimed and `execute` references.
//!
//! HARDENING (live-state): the proposal pre_state fed into the STARK + the message nonce are read
//! from the LIVE sequencer via `get_account(proposal_id)`, NOT fabricated. After `create_proposal`
//! claims the account by signature, its on-chain nonce is incremented (public_account_nonce_increment
//! at apply), so a hardcoded Nonce(0) would mismatch and waste the ~90s proof. Fetching the live
//! Account (owner/balance/data/nonce) makes the proof + message match whatever state actually landed.
//! The fixture-derived header is retained only as a sanity assertion against the fetched data.
//!
//! Proof construction is lifted from `msig_approve_anonymous_membership`
//! (nssa/src/privacy_preserving_transaction/circuit.rs); tx assembly mirrors
//! `WalletCore::send_privacy_preserving_tx` (wallet/src/lib.rs).
//!
//! NOTE: ON-CHAIN when run, and runs a ~90s proof first. Build-only is safe. To fire it:
//!   NSSA_WALLET_HOME_DIR=/root/lez-v012/wallet-home-lp0002 \
//!     RISC0_DEV_MODE=0 cargo run --release -p program_deployment --bin run_approve

use common::transaction::NSSATransaction;
use key_protocol::key_management::ephemeral_key_holder::EphemeralKeyHolder;
use nssa::privacy_preserving_transaction::circuit::{ProgramWithDependencies, execute_and_prove};
use nssa::privacy_preserving_transaction::message::Message;
use nssa::privacy_preserving_transaction::witness_set::WitnessSet;
use nssa::program::Program;
use nssa::{AccountId, PrivacyPreservingTransaction};
use nssa_core::account::{Account, AccountWithMetadata};
use nssa_core::encryption::Scalar;
use nssa_core::encryption::ViewingPublicKey;
use nssa_core::{NullifierPublicKey, NullifierSecretKey, SharedSecretKey};
use program_deployment::msig_demo;
use rand::RngCore as _;
use sequencer_service_rpc::RpcClient as _;
use wallet::WalletCore;

const TESTNET_ENDPOINT: &str = "https://testnet.lez.logos.co";

// Rider keys (mask-2 private rider that emits the vote commitment/nullifier) are minted FRESH from
// the OS RNG every run. Fresh randomness gives every approve a distinct npk automatically, so two
// approves never collide their rider commitment/init-nullifier on-chain (the prior fixed
// DEMO_NSK_*/DEMO_VSK_* pairs collided once their init-nullifier was spent by an earlier run). The
// viewing scalar top bit is cleared so it stays < 2^255 < secp256k1 order and
// ViewingPublicKey::from_scalar (from_repr().unwrap()) never panics; the nullifier secret key needs
// no such constraint (it is SHA256'd into npk). The voter is anonymous; no later step references
// these keys. The membership witness (msig_demo::approver_secret(), the HD-derived nsk) is separate
// and unchanged.
fn fresh_rider_keys() -> (NullifierSecretKey, Scalar) {
    let mut rng = rand::rngs::OsRng;
    let mut nsk: NullifierSecretKey = [0u8; 32];
    rng.fill_bytes(&mut nsk);
    let mut vsk: Scalar = [0u8; 32];
    rng.fill_bytes(&mut vsk);
    vsk[0] &= 0x7F; // clear MSB: value < 2^255 < secp256k1 order -> from_repr() never returns None.
    (nsk, vsk)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = TESTNET_ENDPOINT;

    let wallet = WalletCore::from_env()?;

    let program = msig_demo::msig_program()?;
    let _program_id = program.id();

    // Voter keys — a FRESH rider pair minted per invocation (so approves never collide their rider
    // commitment/init-nullifier on-chain, even across runs against a chain carrying prior state).
    let approver_index = msig_demo::approver_index();
    let (nsk, vsk): (NullifierSecretKey, Scalar) = fresh_rider_keys();
    println!("approving as member index {approver_index}");
    let npk = NullifierPublicKey::from(&nsk);
    let vpk = ViewingPublicKey::from_scalar(vsk);

    // Shared depth-5 member set: root + approver membership path from the fixture.
    let member_root = msig_demo::member_root();
    let merkle_path = msig_demo::approver_path();
    let approver_secret = msig_demo::approver_secret();

    // The unified ProposalState account id (claimed by create_proposal, referenced here).
    let proposal_id_acc = msig_demo::proposal_account_id()?;

    // LIVE pre_state: read the actual on-chain ProposalState from the sequencer.
    // This carries the real program_owner / balance / data / nonce so the STARK pre_state
    // and the message nonce match whatever create_proposal landed (post-claim nonce may be != 0).
    let live_account: Account = wallet
        .sequencer_client
        .get_account(proposal_id_acc)
        .await
        .map_err(|e| anyhow::anyhow!("get_account(proposal) failed: {e}"))?;
    let live_nonce = live_account.nonce;
    println!(
        "live proposal: owner_le0={}, balance={}, data_len={}, nonce={}",
        live_account.program_owner[0],
        live_account.balance,
        live_account.data.clone().into_inner().len(),
        live_nonce.0
    );

    // Sanity: the live frozen header must match the fixture (member_root || PROPOSAL_ID || count).
    let live_data = live_account.data.clone().into_inner();
    if live_data.len() >= msig_core::PROPOSAL_HEADER_LEN {
        let mut expected = Vec::with_capacity(msig_core::PROPOSAL_HEADER_LEN);
        expected.extend_from_slice(&member_root);
        expected.extend_from_slice(&msig_demo::PROPOSAL_ID);
        if live_data[..64] != expected[..] {
            anyhow::bail!(
                "live proposal header (root||id) does not match fixture; create_proposal not landed?"
            );
        }
        let count = u32::from_le_bytes(live_data[64..68].try_into().unwrap());
        println!("live proposal approval_count = {count}");
        if count >= msig_demo::THRESHOLD {
            println!(
                "NOTE: approval_count {count} already >= THRESHOLD {} - an approve may have landed; re-proving will be rejected as a double-vote (nullifier spent).",
                msig_demo::THRESHOLD
            );
        }
    } else {
        anyhow::bail!(
            "live proposal data_len {} < PROPOSAL_HEADER_LEN {}; create_proposal not landed?",
            live_data.len(),
            msig_core::PROPOSAL_HEADER_LEN
        );
    }

    // Build the proposal pre_state DIRECTLY from the fetched live Account (mask 0).
    // is_authorized MUST be false: the proposal is a program-owned PDA with NO signer, and the
    // live apply path reconstructs it as `signer_account_ids.contains(..) == false`. The proof's
    // committed pre_state is compared against that reconstruction (check_privacy_preserving_circuit_proof_is_valid),
    // so a `true` here mismatches and the tx is silently rejected at apply. Validated in-process by
    // nssa::state::tests::msig_approve_live_apply_is_authorized_false.
    let proposal = AccountWithMetadata::new(live_account, false, proposal_id_acc);

    // Fresh private rider (mask 2) at the voter's npk - emits the vote commitment/nullifier.
    let rider = AccountWithMetadata::new(Account::default(), false, AccountId::from(&npk));
    let eph = EphemeralKeyHolder::new(&npk);
    let rider_ssk: SharedSecretKey = eph.calculate_shared_secret_sender(&vpk);
    let epk = eph.generate_ephemeral_public_key();

    let instruction = Program::serialize_instruction(msig_core::MsigInstruction::Approve {
        secret: approver_secret,
        merkle_path,
        proposal_id: msig_demo::PROPOSAL_ID,
    })?;

    println!("Proving approve (RISC0_DEV_MODE=0 -> ~90s)...");
    let prove_start = std::time::Instant::now();
    let program_with_deps: ProgramWithDependencies = program.into();
    let (output, proof) = execute_and_prove(
        vec![proposal, rider],
        instruction,
        vec![0, 2],                  // visibility_mask: proposal public, rider private-unauth
        vec![(npk, rider_ssk)],      // private_account_keys
        vec![],                      // private_account_nsks
        vec![None],                  // private_account_membership_proofs
        &program_with_deps,
    )
    .map_err(|e| {
        // REL-1: the inner-guest prove fails (panics) on every approve-side rejection, so
        // the raw RISC0 error is an opaque panic dump (e.g. a 32-byte hash mismatch for a
        // non-member, or "approval nullifier already recorded" for a double vote). We cannot
        // reliably classify WHICH guest assert fired from that string, so we surface a single
        // clear member-facing message that enumerates the only conditions that can reject an
        // approve, then attach the raw error for operators. The on-chain proposal count is
        // unchanged by a failed prove (nothing was submitted), so a fixed approve can be re-run.
        anyhow::anyhow!(
            "approval proof could not be generated. The approve guest rejected this attempt; \
             the cause is one of: (1) you are not an enrolled member of this proposal\u{2019}s frozen \
             member set, (2) you have already approved this proposal (your proposal-bound vote \
             nullifier is already recorded \u{2014} no double votes), or (3) the proposal id / member \
             root you supplied does not match the live ProposalState. Nothing was submitted, so the \
             on-chain approval count is unchanged; fix the input and re-run. Raw prover error: {e}"
        )
    })?;
    println!(
        "Proved in {:?}: commitments={}, nullifiers={}, ciphertexts={}",
        prove_start.elapsed(),
        output.new_commitments.len(),
        output.new_nullifiers.len(),
        output.ciphertexts.len()
    );

    // Assemble the submittable privacy tx. The proposal is referenced as a public account but has
    // NO signer (program-owned), so its nonce is NOT carried in the message: nonces pair 1:1 with
    // signatures (from_privacy_preserving_transaction enforces nonces.len() == signatures.len()),
    // and the chain reads the unsigned account's state directly. So nonces = empty. One
    // (npk, vpk, epk) tuple per ciphertext (the rider). Validated in-process by the de-risk test.
    let _ = live_nonce; // live nonce informs the proof pre_state, not the message.
    let message = Message::try_from_circuit_output(
        vec![proposal_id_acc],
        vec![],
        vec![(npk, vpk, epk)],
        output,
    )
    .map_err(|e| anyhow::anyhow!("message build failed: {e}"))?;
    let witness_set = WitnessSet::for_message(&message, proof, &[]);
    let tx = PrivacyPreservingTransaction::new(message, witness_set);

    let tx_hash = wallet
        .sequencer_client
        .send_transaction(NSSATransaction::PrivacyPreserving(tx))
        .await
        .map_err(|e| anyhow::anyhow!("send_transaction failed: {e}"))?;
    println!("approve tx_hash: {tx_hash}");
    Ok(())
}
