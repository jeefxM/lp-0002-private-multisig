//! TASK C runner 2 — PUBLIC tx(s): enroll the demo member leaves into the msig MembersRegistry.
//!
//! `MsigInstruction::Enroll { leaf }` appends `leaf = member_leaf(secret)` to the registry account
//! and recomputes `member_root`. Enroll is one-leaf-per-tx, so this runner builds and submits ONE
//! enroll tx per demo member (see [`msig_demo::member_leaves`]). After all three land, the
//! registry's `member_root` equals [`msig_demo::member_root`] — the exact root that
//! `create_proposal` freezes and `approve` proves membership against.
//!
//! BUG-1 FIX: the registry is a SIGNER-OWNED account (the demo registry keypair), NOT a PDA. Each
//! enroll is signed by [`msig_demo::registry_keypair`] so the guest's `Claim::Authorized` of the
//! registry passes apply (the registry is a signer). The guest does not require the registry at any
//! specific PDA address. Nonces advance 0,1,2 as each enroll lands (the registry's first claim and
//! every subsequent signed enroll bumps its public-account nonce); submit them in order.
//!
//! NOTE: ON-CHAIN when run. Build-only is safe. To fire it:
//!   NSSA_WALLET_HOME_DIR=/root/lez-v012/wallet-home-lp0002 \
//!     cargo run --release -p program_deployment --bin run_enroll

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

    // Signer-owned registry = the registry keypair's derived id; signed every enroll.
    let registry_key = msig_demo::registry_keypair()?;
    let registry_id = msig_demo::registry_account_id()?;
    println!("registry account (signer-owned): {registry_id}");
    println!(
        "target member_root after all enrolls: {}",
        hex::encode(msig_demo::member_root())
    );

    // One enroll tx per demo member leaf; nonce advances 0,1,2. Submit in order (each must land
    // before the next, since the signed registry's nonce increments per applied enroll).
    for (i, leaf) in msig_demo::member_leaves().into_iter().enumerate() {
        println!("enrolling leaf {i}: {}", hex::encode(leaf));
        let instruction = msig_core::MsigInstruction::Enroll { leaf };
        let message = Message::try_new(
            program_id,
            vec![registry_id],
            vec![Nonce(i as u128)],
            instruction,
        )?;
        let witness_set = WitnessSet::for_message(&message, &[&registry_key]);
        let tx = PublicTransaction::new(message, witness_set);

        let tx_hash = wallet
            .sequencer_client
            .send_transaction(NSSATransaction::Public(tx))
            .await
            .map_err(|e| anyhow::anyhow!("send_transaction failed for leaf {i}: {e}"))?;
        println!("enroll {i} tx_hash: {tx_hash}");
    }
    Ok(())
}
