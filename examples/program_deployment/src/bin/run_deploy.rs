//! TASK C runner 1 — deploy the `msig` program to LEZ testnet.
//!
//! Loads the deployable `msig.bin` ELF (via the shared [`msig_demo`] fixture), wraps it in a
//! `ProgramDeploymentTransaction`, and submits it via the wallet's sequencer client. Prints the
//! program id (the RISC0 image id) so subsequent runners can target it.
//!
//! NOTE: this performs an ON-CHAIN action when run. Build-only is safe. To fire it:
//!   NSSA_WALLET_HOME_DIR=/root/lez-v012/wallet-home-lp0002 \
//!     cargo run --release -p program_deployment --bin run_deploy
//! (wallet config `sequencer_addr` must point at the testnet — see report.)

use common::transaction::NSSATransaction;
use nssa::program_deployment_transaction::{Message, ProgramDeploymentTransaction};
use program_deployment::msig_demo;
use sequencer_service_rpc::RpcClient as _;
use wallet::WalletCore;

/// Endpoint the wallet config should target for the live flow.
const TESTNET_ENDPOINT: &str = "https://testnet.lez.logos.co";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = TESTNET_ENDPOINT; // documented target; actual addr comes from wallet config.

    let wallet = WalletCore::from_env()?;

    // Load the program; program.id() == the on-chain ProgramId after deployment.
    let program = msig_demo::msig_program()?;
    let program_id = program.id();
    println!("msig program_id (8x u32 le): {program_id:?}");

    // A deployment tx carries only the bytecode.
    let message = Message::new(program.elf().to_vec());
    let tx = ProgramDeploymentTransaction::new(message);

    let tx_hash = wallet
        .sequencer_client
        .send_transaction(NSSATransaction::ProgramDeployment(tx))
        .await
        .map_err(|e| anyhow::anyhow!("send_transaction failed: {e}"))?;
    println!("deploy tx_hash: {tx_hash}");
    Ok(())
}
