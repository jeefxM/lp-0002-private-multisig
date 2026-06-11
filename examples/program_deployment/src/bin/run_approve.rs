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
use sequencer_service_rpc::RpcClient as _;
use wallet::WalletCore;

const TESTNET_ENDPOINT: &str = "https://testnet.lez.logos.co";

// DEMO VOTER KEYS — one DISTINCT (nsk, vsk) rider pair per member index. Each rider note's
// commitment/init-nullifier is deterministic from npk, so two approves in the SAME run MUST use
// DISTINCT npks or the second rider collides on-chain. These are brand-new throwaways, NOT the
// prior run's pair (now spent) and NOT test_private_account_keys_*. High bytes kept small so the
// viewing scalar stays under the curve order. The voter is anonymous; no later step references
// these keys.
/// Voter #0 private nullifier key (fresh throwaway).
const DEMO_NSK_0: NullifierSecretKey = [
    0x0c, 0x71, 0xa4, 0x3e, 0x95, 0x18, 0xbd, 0x27, 0x6a, 0xc1, 0x04, 0x8f, 0x32, 0xe6, 0x5b, 0x90, 0x47, 0x1c, 0x88, 0x2a, 0x0d, 0xf3, 0x59, 0xb4, 0x66, 0x21, 0x7e, 0xd5, 0x39, 0xaa, 0x0b, 0x12,
];
/// Voter #0 private viewing key (fresh throwaway, top byte small).
const DEMO_VSK_0: Scalar = [
    0x0e, 0x53, 0xc7, 0x21, 0x8a, 0x46, 0x1f, 0xb9, 0x05, 0x6d, 0xe2, 0x33, 0x9c, 0x40, 0x7a, 0x18, 0xcc, 0x2b, 0x65, 0x91, 0x04, 0xd8, 0x57, 0xba, 0x3e, 0x09, 0x6f, 0xa1, 0x12, 0xe4, 0x4d, 0x08,
];
/// Voter #1 private nullifier key (fresh throwaway, DISTINCT from #0).
const DEMO_NSK_1: NullifierSecretKey = [
    0x0a, 0x38, 0xd6, 0x14, 0x7f, 0xb2, 0x29, 0x5c, 0x81, 0x03, 0xe9, 0x46, 0x1b, 0xc7, 0x50, 0x8d, 0x32, 0xa4, 0x69, 0x15, 0xde, 0x07, 0xb8, 0x4f, 0x21, 0x9a, 0x63, 0x0c, 0xf5, 0x38, 0xab, 0x11,
];
/// Voter #1 private viewing key (fresh throwaway, DISTINCT from #0, top byte small).
const DEMO_VSK_1: Scalar = [
    0x07, 0x4a, 0xb1, 0x2f, 0x96, 0x53, 0x1d, 0xc8, 0x60, 0x0b, 0xe5, 0x37, 0x9f, 0x42, 0x71, 0x1a, 0xd3, 0x28, 0x5b, 0x0c, 0x6e, 0xa9, 0x14, 0xf7, 0x30, 0x8d, 0x52, 0xb6, 0x09, 0xe1, 0x47, 0x0a,
];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = TESTNET_ENDPOINT;

    let wallet = WalletCore::from_env()?;

    let program = msig_demo::msig_program()?;
    let _program_id = program.id();

    // Voter keys — one DISTINCT rider pair per member index (so the two approves in this run do
    // not collide their rider commitment/init-nullifier on-chain).
    let approver_index = msig_demo::approver_index();
    let (nsk, vsk): (NullifierSecretKey, Scalar) = match approver_index {
        0 => (DEMO_NSK_0, DEMO_VSK_0),
        1 => (DEMO_NSK_1, DEMO_VSK_1),
        other => anyhow::bail!("APPROVER_INDEX {other} has no dedicated rider key pair (only 0,1 wired)"),
    };
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
    .map_err(|e| anyhow::anyhow!("execute_and_prove failed: {e}"))?;
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
