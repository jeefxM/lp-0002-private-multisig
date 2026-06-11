//! TASK C runner 3 — PUBLIC tx: create + freeze a ProposalState.
//!
//! `MsigInstruction::CreateProposal { member_root, proposal_id }` claims a fresh public account and
//! freezes the proposal's `member_root` (count 0). Modeled on
//! `msig_create_proposal_public_tx_claims_and_freezes` in nssa/src/state.rs.
//!
//! The proposal account is the fixed demo-keypair-derived account
//! ([`msig_demo::proposal_account_id`]) claimed via the proposal keypair's signature. `approve` and
//! `execute` reference this SAME account id (they do not re-derive a PDA). The frozen `member_root`
//! is [`msig_demo::member_root`] — the depth-5 root over the enrolled demo members.
//!
//! NOTE: ON-CHAIN when run. Build-only is safe. To fire it:
//!   NSSA_WALLET_HOME_DIR=/root/lez-v012/wallet-home-lp0002 \
//!     cargo run --release -p program_deployment --bin run_create_proposal

use common::transaction::NSSATransaction;
use nssa::public_transaction::{Message, WitnessSet};
use nssa::PublicTransaction;
use nssa_core::account::Nonce;
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

    // The unified ProposalState account = the demo proposal keypair's public-key-derived id.
    let proposal_key = msig_demo::proposal_keypair()?;
    let proposal_account = msig_demo::proposal_account_id()?;
    println!("proposal account: {proposal_account}");

    let instruction = msig_core::MsigInstruction::CreateProposal {
        member_root: msig_demo::member_root(),
        proposal_id: msig_demo::PROPOSAL_ID,
    };
    // Fresh account → nonce 0; claimed by the proposal key's signature.
    let message = Message::try_new(
        program_id,
        vec![proposal_account],
        vec![Nonce(0)],
        instruction,
    )?;
    let witness_set = WitnessSet::for_message(&message, &[&proposal_key]);
    let tx = PublicTransaction::new(message, witness_set);

    let tx_hash = wallet
        .sequencer_client
        .send_transaction(NSSATransaction::Public(tx))
        .await
        .map_err(|e| anyhow::anyhow!("send_transaction failed: {e}"))?;
    println!("create_proposal tx_hash: {tx_hash}");
    Ok(())
}
