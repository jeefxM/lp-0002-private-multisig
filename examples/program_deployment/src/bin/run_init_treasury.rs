//! TASK C runner — PUBLIC tx(s): bootstrap the treasury (and recipient) PDAs on-chain.
//!
//! BUG-2 FIX. A plain `authenticated_transfer` to the uninitialized treasury PDA is rejected:
//! authenticated_transfer would `Claim::Authorized` the fresh (default-owned) recipient, and apply
//! only accepts that claim for a SIGNER or a caller-authorized PDA — a PDA address holds no key and
//! can never sign, so a top-level transfer can never bootstrap it (proven in-process by
//! `msig_fund_treasury_pda_rejected`, which reproduces the exact `ClaimedUnauthorizedAccount`).
//!
//! `MsigInstruction::InitTreasury { seed, transfer_program_id }` fixes this: msig chains to
//! authenticated_transfer's amount-0 initialize with `pda_seeds = [seed]`, so the callee claims the
//! treasury PDA under msig's PDA authorization. The treasury ends up authenticated_transfer-owned
//! with balance 0. A SUBSEQUENT plain transfer (no claim — the PDA is now non-default-owned) funds
//! it, and `Execute` later drains it. We init BOTH the treasury PDA and the recipient PDA (the
//! execute credit needs the recipient owned by authenticated_transfer too).
//!
//! Full on-chain bootstrap order:
//!   1. run_init_treasury           (this runner: InitTreasury treasury + recipient)
//!   2. wallet auth-transfer send --from <payer> --to Public/<treasury> --amount <N>   (fund it)
//!   3. run_execute                 (drains treasury → recipient at threshold)
//!
//! NOTE: ON-CHAIN when run. Build-only is safe. To fire it:
//!   NSSA_WALLET_HOME_DIR=/root/lez-v012/wallet-home-lp0002 \
//!     cargo run --release -p program_deployment --bin run_init_treasury

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
    let transfer_program_id = msig_demo::transfer_program_id();

    let treasury_id = msig_demo::treasury_account_id(&program_id);
    let recipient_id = msig_demo::recipient_account_id(&program_id);
    println!("treasury PDA:  {treasury_id}");
    println!("recipient PDA: {recipient_id}");

    // Each InitTreasury: msig chains to authenticated_transfer's init under PDA authorization.
    // No signer — the PDA is authorized via the chained pda_seeds.
    for (label, seed, account_id) in [
        ("treasury", msig_demo::TREASURY_SEED, treasury_id),
        ("recipient", msig_demo::RECIPIENT_SEED, recipient_id),
    ] {
        let instruction = msig_core::MsigInstruction::InitTreasury {
            seed,
            transfer_program_id,
        };
        let message = Message::try_new(program_id, vec![account_id], vec![], instruction)?;
        let witness_set = WitnessSet::for_message(&message, &[]);
        let tx = PublicTransaction::new(message, witness_set);

        let tx_hash = wallet
            .sequencer_client
            .send_transaction(NSSATransaction::Public(tx))
            .await
            .map_err(|e| anyhow::anyhow!("send_transaction failed for {label}: {e}"))?;
        println!("init_treasury {label} tx_hash: {tx_hash}");
    }

    println!("next: fund the treasury with a plain `wallet auth-transfer send` (it is now");
    println!("authenticated_transfer-owned, so the transfer needs no PDA signer), then run_execute.");
    Ok(())
}
