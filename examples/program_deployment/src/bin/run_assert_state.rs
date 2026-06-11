//! LP-0002 read-only post-flow assertion (no network mutation).
//!
//! Reads the live on-chain state from the sequencer the wallet config points at and asserts the
//! demo outcome. Used by `scripts/lp0002-demo.sh` as the load-bearing green/red gate. It mutates
//! nothing (only `get_account`), so it is safe to run repeatedly.
//!
//! Assertions (all derived from the shared `msig_demo` fixture so they track the real demo):
//!   * proposal `approval_count` (data[64..68] LE u32) == `EXPECT_COUNT`   (default THRESHOLD=2)
//!   * treasury PDA balance                          == `EXPECT_TREASURY`  (default 0 after drain)
//!   * recipient PDA balance                         == `EXPECT_RECIPIENT` (default = funded amount)
//!
//! Overridable via env (all decimal): EXPECT_COUNT, EXPECT_TREASURY, EXPECT_RECIPIENT.
//! Exits non-zero on the first failed assertion so the shell script fails loudly.
//!
//!   NSSA_WALLET_HOME_DIR=<home> cargo run --release -p program_deployment --bin run_assert_state

use msig_core::PROPOSAL_HEADER_LEN;
use nssa_core::account::Account;
use program_deployment::msig_demo;
use sequencer_service_rpc::RpcClient as _;
use wallet::WalletCore;

fn env_u128(key: &str, default: u128) -> u128 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<u128>().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let wallet = WalletCore::from_env()?;

    let program = msig_demo::msig_program()?;
    let program_id = program.id();

    let proposal_id = msig_demo::proposal_account_id()?;
    let treasury_id = msig_demo::treasury_account_id(&program_id);
    let recipient_id = msig_demo::recipient_account_id(&program_id);

    let expect_count = env_u128("EXPECT_COUNT", u128::from(msig_demo::THRESHOLD)) as u32;
    let expect_treasury = env_u128("EXPECT_TREASURY", 0);
    let expect_recipient = env_u128("EXPECT_RECIPIENT", 0);

    let client = &wallet.sequencer_client;

    // --- proposal approval_count -------------------------------------------------------------
    let proposal: Account = client
        .get_account(proposal_id)
        .await
        .map_err(|e| anyhow::anyhow!("get_account(proposal) failed: {e}"))?;
    let pdata = proposal.data.clone().into_inner();
    anyhow::ensure!(
        pdata.len() >= PROPOSAL_HEADER_LEN,
        "proposal data_len {} < header {} (create_proposal not landed?)",
        pdata.len(),
        PROPOSAL_HEADER_LEN
    );
    let count = u32::from_le_bytes(pdata[64..68].try_into().unwrap());
    println!("ASSERT proposal {proposal_id}: approval_count={count} (expect {expect_count})");
    anyhow::ensure!(
        count == expect_count,
        "FAIL approval_count {count} != expected {expect_count}"
    );

    // --- treasury balance (drained) ----------------------------------------------------------
    let treasury: Account = client
        .get_account(treasury_id)
        .await
        .map_err(|e| anyhow::anyhow!("get_account(treasury) failed: {e}"))?;
    println!(
        "ASSERT treasury {treasury_id}: balance={} (expect {expect_treasury})",
        treasury.balance
    );
    anyhow::ensure!(
        treasury.balance == expect_treasury,
        "FAIL treasury balance {} != expected {expect_treasury}",
        treasury.balance
    );

    // --- recipient balance (received the drain) ----------------------------------------------
    let recipient: Account = client
        .get_account(recipient_id)
        .await
        .map_err(|e| anyhow::anyhow!("get_account(recipient) failed: {e}"))?;
    println!(
        "ASSERT recipient {recipient_id}: balance={} (expect {expect_recipient})",
        recipient.balance
    );
    anyhow::ensure!(
        recipient.balance == expect_recipient,
        "FAIL recipient balance {} != expected {expect_recipient}",
        recipient.balance
    );

    println!("ALL ASSERTIONS PASSED");
    Ok(())
}
