//! TASK C runner 5b — PRIVACY tx: anonymous M-of-N approval, SECRET-DRIVEN (anti-#87).
//!
//! Identical proof/tx machinery to `run_approve`, with ONE load-bearing difference: the member
//! identity is NOT read from the compile-time `msig_demo` fixture (`approver_index()` /
//! `approver_secret()`). It is derived ENTIRELY from the runtime env var `APPROVER_SECRET_HEX`:
//!
//!   * `secret`      = hex-decoded `APPROVER_SECRET_HEX` (32 bytes).
//!   * `target_leaf` = `msig_core::member_leaf(&secret)`.
//!   * `idx`         = position of `target_leaf` in `msig_demo::member_leaves()`; if absent we
//!                     print "not an enrolled member" and exit NON-ZERO WITHOUT proving.
//!   * `merkle_path` = `msig_core::merkle_path(&member_leaves(), idx)` against `member_root()`.
//!   * the SAME `secret` is placed in `MsigInstruction::Approve { secret, .. }`.
//!
//! So the user's entered secret simultaneously (a) selects the Merkle leaf/path the guest checks
//! membership against AND (b) is the witness the guest hashes into the proposal-bound vote
//! nullifier. A wrong/absent secret cannot drive a vote: it is rejected at the `.position()` gate
//! (non-member) before any proof, and even if forced through it would fail the in-guest Merkle
//! check. This is the property #87 lacked.
//!
//! Rider keys (mask-2 private rider that emits the vote commitment/nullifier) are generated FRESH
//! from the OS RNG every run (was: per-index `DEMO_NSK_*`/`DEMO_VSK_*` consts gated by a match that
//! only wired indices 0/1). Fresh randomness gives every approve a distinct `npk` automatically, so
//! ANY enrolled member (0,1,2,...) can vote — not just 0/1 — and two approves never collide their
//! rider commitment/init-nullifier on-chain. The viewing scalar's top bit is cleared so it stays
//! `< 2^255 < secp256k1 order` and `ViewingPublicKey::from_scalar` (which does `from_repr().unwrap()`)
//! never panics. The nullifier secret key needs no such constraint (it is SHA256'd into `npk`).
//!
//! Everything downstream (live `get_account` of the proposal, `execute_and_prove`,
//! `Message::try_from_circuit_output`, `WitnessSet`, `send_transaction`, `approve tx_hash:` print)
//! is byte-for-byte the same as `run_approve`.
//!
//!   APPROVER_SECRET_HEX=a7a7...a7 NSSA_WALLET_HOME_DIR=<home> RISC0_DEV_MODE=<0|1> \
//!     cargo run --release -p program_deployment --bin run_approve_secret

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

/// Reads `APPROVER_SECRET_HEX` and decodes it to a 32-byte secret. Errors clearly if the env var
/// is missing or not exactly 32 hex-encoded bytes.
fn read_secret_from_env() -> anyhow::Result<[u8; 32]> {
    let raw = std::env::var("APPROVER_SECRET_HEX").map_err(|_| {
        anyhow::anyhow!(
            "APPROVER_SECRET_HEX is not set. Provide the member secret as 64 hex chars \
             (32 bytes), e.g. APPROVER_SECRET_HEX=a7a7...a7"
        )
    })?;
    let trimmed = raw.trim().trim_start_matches("0x");
    let bytes = hex::decode(trimmed).map_err(|e| {
        anyhow::anyhow!("APPROVER_SECRET_HEX is not valid hex: {e}")
    })?;
    let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
        anyhow::anyhow!(
            "APPROVER_SECRET_HEX must decode to exactly 32 bytes; got {} bytes",
            bytes.len()
        )
    })?;
    Ok(arr)
}

/// Generates a fresh rider keypair from the OS RNG. The viewing scalar's top bit is cleared so it
/// stays under the secp256k1 order (so `ViewingPublicKey::from_scalar`'s `from_repr().unwrap()`
/// cannot panic); the nullifier secret key is unconstrained (it is SHA256'd into the npk).
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

    // ---- SECRET-DRIVEN membership (the anti-#87 core) ---------------------------------------
    // The user's entered secret drives BOTH the leaf/path selection AND the instruction witness.
    let secret = read_secret_from_env()?;
    let target_leaf = msig_core::member_leaf(&secret);
    let member_leaves = msig_demo::member_leaves();
    let idx = match member_leaves.iter().position(|l| *l == target_leaf) {
        Some(i) => i,
        None => {
            eprintln!(
                "REJECTED: the supplied APPROVER_SECRET_HEX is not an enrolled member of this \
                 proposal's frozen member set (its member_leaf is not in the published leaves). \
                 No proof was generated and nothing was submitted; the on-chain approval count is \
                 unchanged."
            );
            std::process::exit(1);
        }
    };
    println!("secret resolves to enrolled member index {idx}");

    // Fresh rider keypair from OS RNG (distinct npk per run -> any member can vote, no collisions).
    let (nsk, vsk) = fresh_rider_keys();
    let npk = NullifierPublicKey::from(&nsk);
    let vpk = ViewingPublicKey::from_scalar(vsk);

    // Shared depth-5 member set: root + this secret's membership path from the fixture leaves.
    let member_root = msig_demo::member_root();
    let merkle_path = msig_core::merkle_path(&member_leaves, idx);

    // The unified ProposalState account id (claimed by create_proposal, referenced here).
    let proposal_id_acc = msig_demo::proposal_account_id()?;

    // LIVE pre_state: read the actual on-chain ProposalState from the sequencer.
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

    // Build the proposal pre_state DIRECTLY from the fetched live Account (mask 0). is_authorized
    // MUST be false (program-owned PDA, no signer) to match the live apply reconstruction.
    let proposal = AccountWithMetadata::new(live_account, false, proposal_id_acc);

    // Fresh private rider (mask 2) at the voter's npk - emits the vote commitment/nullifier.
    let rider = AccountWithMetadata::new(Account::default(), false, AccountId::from(&npk));
    let eph = EphemeralKeyHolder::new(&npk);
    let rider_ssk: SharedSecretKey = eph.calculate_shared_secret_sender(&vpk);
    let epk = eph.generate_ephemeral_public_key();

    // The SAME secret the leaf/path were derived from is the in-guest membership + nullifier witness.
    let instruction = Program::serialize_instruction(msig_core::MsigInstruction::Approve {
        secret,
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

    // Assemble the submittable privacy tx (proposal referenced public/no-signer -> nonces empty).
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
