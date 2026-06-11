//! TASK C runner 4 — PUBLIC tx: threshold-gated treasury release.
//!
//! `MsigInstruction::Execute { threshold, seed }`: once the proposal's approval_count >= threshold,
//! the guest emits a chained call to authenticated_transfer draining the treasury PDA to the
//! recipient. Modeled on `msig_execute_releases_at_threshold` in nssa/src/state.rs.
//!
//! pre_states order is [proposal, treasury(PDA), recipient]. The proposal is the SAME unified
//! account `create_proposal` claimed ([`msig_demo::proposal_account_id`]); treasury and recipient
//! are public PDAs of msig owned by authenticated_transfer → PDA-authorised, no signer.
//!
//! NOTE: ON-CHAIN when run. Build-only is safe. To fire it:
//!   NSSA_WALLET_HOME_DIR=/root/lez-v012/wallet-home-lp0002 \
//!     cargo run --release -p program_deployment --bin run_execute

use common::transaction::NSSATransaction;
use nssa::public_transaction::{Message, WitnessSet};
use nssa::PublicTransaction;
use program_deployment::msig_demo;
use sequencer_service_rpc::RpcClient as _;
use wallet::WalletCore;

const TESTNET_ENDPOINT: &str = "https://testnet.lez.logos.co";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = TESTNET_ENDPOINT;

    let wallet = WalletCore::from_env()?;

    let program = msig_demo::msig_program()?;
    let program_id = program.id();

    // Proposal = the unified keypair-derived account; treasury + recipient are public PDAs of msig.
    let proposal_id = msig_demo::proposal_account_id()?;
    let treasury_id = msig_demo::treasury_account_id(&program_id);
    let recipient_id = msig_demo::recipient_account_id(&program_id);
    println!("proposal:  {proposal_id}");
    println!("treasury:  {treasury_id}");
    println!("recipient: {recipient_id}");

    let instruction = msig_core::MsigInstruction::Execute {
        threshold: msig_demo::THRESHOLD,
        seed: msig_demo::TREASURY_SEED,
    };
    // No signers — treasury is PDA-authorised.
    let message = Message::try_new(
        program_id,
        vec![proposal_id, treasury_id, recipient_id],
        vec![],
        instruction,
    )?;
    let witness_set = WitnessSet::for_message(&message, &[]);
    let tx = PublicTransaction::new(message, witness_set);

    let tx_hash = wallet
        .sequencer_client
        .send_transaction(NSSATransaction::Public(tx))
        .await
        .map_err(|e| anyhow::anyhow!("send_transaction failed: {e}"))?;
    println!("execute tx_hash: {tx_hash}");
    Ok(())
}
