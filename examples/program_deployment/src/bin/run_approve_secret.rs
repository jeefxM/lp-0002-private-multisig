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
//!   * `vpk`         = `msig_demo::member_vpks()[idx]` — the SAME member's post-quantum ML-KEM
//!                     viewing key (the rider account's keys are bound to the resolved member).
//!   * the SAME `secret` is placed in `MsigInstruction::Approve { secret, .. }`.
//!
//! So the user's entered secret simultaneously (a) selects the Merkle leaf/path the guest checks
//! membership against, (b) is the witness the guest hashes into the proposal-bound vote nullifier,
//! AND (c) keys the PRIVATE voting account (`PrivateShared` rider, identifier
//! [`msig_demo::VOTE_IDENTIFIER`]) that review item #6 binds the vote to. A wrong/absent secret is
//! rejected at the `.position()` gate (non-member) before any proof.
//!
//! rc5 PORT: drops the v0.1.2 manual rider/`execute_and_prove`/`Message`/`send_transaction` path
//! for the wallet-managed [`WalletCore::send_privacy_preserving_tx`] (see `run_approve`).
//!
//!   APPROVER_SECRET_HEX=a7a7...a7 LEE_WALLET_HOME_DIR=<home> RISC0_DEV_MODE=<0|1> \
//!     cargo run --release -p program_deployment --bin run_approve_secret

use anyhow::{anyhow, bail};
use lee::AccountId;
use lee::privacy_preserving_transaction::circuit::ProgramWithDependencies;
use lee::program::Program;
use lee_core::NullifierPublicKey;
use msig_core::MsigInstruction;
use program_deployment::msig_demo;
use wallet::{AccountIdentity, WalletCore};

/// Reads `APPROVER_SECRET_HEX` and decodes it to a 32-byte secret. Errors clearly if the env var
/// is missing or not exactly 32 hex-encoded bytes.
fn read_secret_from_env() -> anyhow::Result<[u8; 32]> {
    let raw = std::env::var("APPROVER_SECRET_HEX").map_err(|_| {
        anyhow!(
            "APPROVER_SECRET_HEX is not set. Provide the member secret as 64 hex chars \
             (32 bytes), e.g. APPROVER_SECRET_HEX=a7a7...a7"
        )
    })?;
    let trimmed = raw.trim().trim_start_matches("0x");
    let bytes = hex::decode(trimmed)
        .map_err(|e| anyhow!("APPROVER_SECRET_HEX is not valid hex: {e}"))?;
    let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
        anyhow!(
            "APPROVER_SECRET_HEX must decode to exactly 32 bytes; got {} bytes",
            bytes.len()
        )
    })?;
    Ok(arr)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let wallet = WalletCore::from_env().await?;

    let program = msig_demo::msig_program()?;
    let program_with_deps: ProgramWithDependencies = program.clone().into();

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

    // The resolved member's identity: npk from the secret, its post-quantum ML-KEM viewing key, and
    // the standard private AccountId for (npk, VOTE_IDENTIFIER) — the LIVE voting account to ride.
    let npk = NullifierPublicKey::from(&secret);
    let vpk = msig_demo::member_vpks()[idx].clone();
    let voting_id = AccountId::for_regular_private_account(&npk, &vpk, msig_demo::VOTE_IDENTIFIER);

    // Pre-check BEFORE the ~90s prove: the member's voting account must be live + synced on this
    // wallet (the wallet needs its membership proof to build the rider; review item #6's
    // `rider.account != default` assert rejects a non-live rider in-circuit anyway).
    if wallet.get_account_private(voting_id).is_none()
    {
        bail!(
            "voting account {voting_id} not live/synced; fund+sync it before voting \
             (re-sync if you voted on another proposal)"
        );
    }

    // The SAME secret the leaf/path were derived from is the in-guest membership + nullifier witness.
    let instruction = Program::serialize_instruction(MsigInstruction::Approve {
        secret,
        merkle_path: msig_core::merkle_path(&member_leaves, idx),
        proposal_id: msig_demo::PROPOSAL_ID,
        vpk: vpk.clone(),
    })?;

    // Account order MUST match the guest's `[proposal, rider]`.
    let accounts = vec![
        AccountIdentity::PublicNoSign(msig_demo::proposal_account_id()?),
        AccountIdentity::PrivateShared {
            nsk: secret,
            npk,
            vpk,
            identifier: msig_demo::VOTE_IDENTIFIER,
        },
    ];

    println!("Proving + submitting approve (RISC0_DEV_MODE=0 -> ~90s)...");
    let (tx_hash, _) = wallet
        .send_privacy_preserving_tx(accounts, instruction, &program_with_deps)
        .await
        .map_err(|e| {
            anyhow!(
                "approve rejected (not enrolled / already voted / proposal mismatch / \
                 voting account not live): {e}"
            )
        })?;
    println!("approve tx_hash: {tx_hash}");
    Ok(())
}
