//! LP-0002 read-only proposal status for the Basecamp sidecar (/status).
//!
//! Mutates NOTHING (only `get_account` on the unified `ProposalState`). Prints exactly ONE JSON
//! line so the Node sidecar can parse it the same way the Qt backend parses the sidecar's JSON.
//! Unlike `run_assert_state` it asserts nothing and does NOT touch the treasury/recipient PDAs
//! (which do not exist before `execute`), so it is safe to poll at any point in the flow.
//!
//! Output shape (single line):
//!   ready=true  : {"ready":true,"proposal_id":"<hex>","member_root":"<hex>","approval_count":N,"threshold":T}
//!   ready=false : {"ready":false,"reason":"<text>","threshold":T}   (proposal not created yet)
//!
//! The sequencer URL comes from the wallet config under NSSA_WALLET_HOME_DIR (WalletCore::from_env).
//!
//!   NSSA_WALLET_HOME_DIR=<home> cargo run --release -p program_deployment --bin run_read_status

use msig_core::PROPOSAL_HEADER_LEN;
use nssa_core::account::Account;
use program_deployment::msig_demo;
use sequencer_service_rpc::RpcClient as _;
use wallet::WalletCore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let wallet = WalletCore::from_env()?;
    let threshold = msig_demo::THRESHOLD;
    let proposal_id_hex = hex::encode(msig_demo::PROPOSAL_ID);

    let proposal_acc_id = msig_demo::proposal_account_id()?;
    let proposal: Account = match wallet.sequencer_client.get_account(proposal_acc_id).await {
        Ok(a) => a,
        Err(e) => {
            // Network/RPC failure: report not-ready rather than crashing the sidecar.
            println!(
                "{{\"ready\":false,\"reason\":\"get_account failed: {}\",\"threshold\":{}}}",
                escape(&e.to_string()),
                threshold
            );
            return Ok(());
        }
    };

    let data = proposal.data.clone().into_inner();
    if data.len() < PROPOSAL_HEADER_LEN {
        // create_proposal has not landed yet (account empty / uninitialized).
        println!(
            "{{\"ready\":false,\"reason\":\"proposal not created (data_len {})\",\"threshold\":{},\"proposal_id\":\"{}\"}}",
            data.len(),
            threshold,
            proposal_id_hex
        );
        return Ok(());
    }

    let member_root_hex = hex::encode(&data[0..32]);
    let count = u32::from_le_bytes(data[64..68].try_into().unwrap());
    println!(
        "{{\"ready\":true,\"proposal_id\":\"{}\",\"member_root\":\"{}\",\"approval_count\":{},\"threshold\":{}}}",
        proposal_id_hex, member_root_hex, count, threshold
    );
    Ok(())
}

/// Minimal JSON string escaping for the error-reason field (backslash + quote + control chars).
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}
