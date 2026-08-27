//! TASK C runner 5 — PRIVACY tx: anonymous M-of-N approval (the hard one).
//!
//! A privacy-preserving transaction mutates a PUBLIC ProposalState (mask 0) while the member secret
//! + Merkle path + proposal_id travel as a PRIVATE instruction witness. The guest verifies in-guest
//! Merkle membership against the snapshotted member_root, derives a proposal-bound vote nullifier,
//! rejects double-votes, and increments the count. The voter rides a PRIVATE shielded account (the
//! member's LIVE voting account, identifier [`msig_demo::VOTE_IDENTIFIER`]) that emits the
//! commitment/nullifier the privacy tx requires. The voter stays anonymous.
//!
//! rc5 PORT: this drops the v0.1.2 manual path (fresh throwaway rider keys, `get_account`,
//! hand-built `AccountWithMetadata`, `execute_and_prove`, `Message`/`WitnessSet`, raw
//! `send_transaction`) for the rc5 WALLET-MANAGED API:
//! [`WalletCore::send_privacy_preserving_tx`]. The wallet derives the pre_states (incl. the live
//! private rider via its membership proof), builds the message, proves, and submits. Membership
//! inputs still come from the shared [`msig_demo`] fixture so `enroll`/`create_proposal`/`approve`
//! compose into one valid chain.
//!
//! REVIEW ITEM #6 (in-circuit live-account binding): the rider is presented as the member's LIVE
//! shielded voting account at [`msig_demo::VOTE_IDENTIFIER`], keyed by the SAME membership `secret`
//! (`nsk`). The guest asserts `rider.account_id == for_regular_private_account(npk(secret),
//! VOTE_IDENTIFIER)` AND `rider.account != Account::default()`, so the anonymous vote is bound to
//! the member's LIVE shielded account — not a fabricated fresh rider. The pre-check below fails fast
//! (before the prove) if that voting account is not live/tracked on this wallet.
//!
//! rc5-LOCAL RIDER VARIANT: the rider is `AccountIdentity::PrivateOwned(voting_id)`, NOT
//! `PrivateShared`. For the LOCAL demo the member's voting KeyChain is imported into THIS wallet's
//! key tree (`run_setup_voters`) and funded+tracked by a shielded transfer to `Private/<voting_id>`.
//! `PrivateOwned` is the ONLY rider variant whose pre_state the wallet can correctly supply for a
//! demo-keyed account: `private_key_tree_acc_preparation` reads the live state + membership proof
//! from the imported key tree (whereas `PrivateShared`'s pre_state comes from a shared-account map
//! with no public injection path). The circuit still builds the SAME
//! `PrivateAuthorizedUpdate{nsk, membership_proof}` arm review item #6 binds the vote to, so the
//! in-circuit semantics are unchanged.
//!
//! NOTE: ON-CHAIN when run. Build-only is safe. To fire it (LOCAL rc5):
//!   APPROVER_INDEX=0 LEE_WALLET_HOME_DIR=<home> RISC0_DEV_MODE=1 \
//!     cargo run --release -p program_deployment --bin run_approve

use anyhow::{anyhow, bail};
use lee::AccountId;
use lee::privacy_preserving_transaction::circuit::ProgramWithDependencies;
use lee::program::Program;
use lee_core::NullifierPublicKey;
use msig_core::MsigInstruction;
use program_deployment::msig_demo;
use wallet::{AccountIdentity, WalletCore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let wallet = WalletCore::from_env().await?;

    let program = msig_demo::msig_program()?;
    let program_with_deps: ProgramWithDependencies = program.clone().into();

    // Membership identity from the shared fixture: the approving member's nsk (`secret`) and its
    // npk. The voting account lives at the standard private AccountId for (npk, VOTE_IDENTIFIER).
    let secret = msig_demo::approver_secret();
    let npk = NullifierPublicKey::from(&secret);
    let vpk = msig_demo::member_vpks()[msig_demo::approver_index()].clone();
    let voting_id = AccountId::for_regular_private_account(&npk, &vpk, msig_demo::VOTE_IDENTIFIER);
    println!("approving as member index {}", msig_demo::approver_index());

    // Pre-check BEFORE the prove: the member's voting account must be live + tracked on this wallet
    // (the wallet needs its membership proof to build the rider pre_state, and review item #6's
    // `rider.account != default` assert rejects a non-live rider in-circuit anyway).
    if wallet.get_account_private(voting_id).is_none()
    {
        bail!(
            "voting account {voting_id} not live/tracked; fund+import it before voting \
             (run_setup_voters + a shielded transfer to Private/<voting_id>)"
        );
    }

    // The SAME `secret` is the in-guest membership + nullifier witness.
    let instruction = Program::serialize_instruction(MsigInstruction::Approve {
        secret,
        merkle_path: msig_demo::approver_path(),
        proposal_id: msig_demo::PROPOSAL_ID,
        vpk: vpk.clone(),
    })?;

    // Account order MUST match the guest's `[proposal, rider]`: public ProposalState (no signer —
    // it is program-owned) then the member's PRIVATE voting account as the OWNED rider.
    let accounts = vec![
        AccountIdentity::PublicNoSign(msig_demo::proposal_account_id()?),
        AccountIdentity::PrivateOwned(voting_id),
    ];

    println!("Proving + submitting approve...");
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
