//! LP-0002 LOCAL-demo helper: make members 0 & 1 fundable + wallet-tracked, and import the
//! genesis funder, so `run_approve`'s review-item-#6 pre-check
//! (`check_private_account_initialized(voting_id)`) has a LIVE, OWNED voting account to ride.
//!
//! This is the #6-funding solution for a LOCAL rc5 sequencer (RISC0_DEV_MODE=1):
//!   1. Import the genesis-funded public FUNDER (`testnet_initial_state` public account 0, balance
//!      10000, owned by `authenticated_transfer`) into THIS wallet so it can pay shielded dust to
//!      the member voting accounts and bulk-fund the treasury. (The local sequencer's genesis is
//!      `testnet_initial_state::initial_state()`, so this account is funded + directly spendable.)
//!   2. Import member 0 & 1 FULL KeyChains (`msig_demo::member_key_chain`, the SAME HD derivation
//!      as the membership `nsk`/`vpk`) as PRIVATE accounts at identifier `VOTE_IDENTIFIER` with a
//!      default/uninitialised state. After a shielded transfer to `Private/<voting_id>` the wallet
//!      decodes + tracks the EXACT funded state (matching on-chain commitment), so the voting
//!      account is OWNED (`AccountIdentity::PrivateOwned`) and its membership proof is fetchable —
//!      exactly what `run_approve` needs.
//!   3. Write `<VOTERS_DIR>/member<i>.keys` (npk\nvpk hex) and PRINT the FUNDER_ID + per-member
//!      voting ids so the orchestration script can drive the funding/approve steps.
//!
//! LOCAL only. Env: `LEE_WALLET_HOME_DIR` (wallet home), `VOTERS_DIR` (output dir for keys files).
//!
//!   VOTERS_DIR=<dir> LEE_WALLET_HOME_DIR=<home> \
//!     cargo run --release -p program_deployment --bin run_setup_voters

use std::io::Write as _;

use lee::{Account, AccountId};
use program_deployment::msig_demo;
use wallet::WalletCore;

/// Members that participate in the 2-of-3 demo approval (indices into `msig_demo::member_*`).
const VOTER_INDICES: [usize; 2] = [0, 1];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut wallet = WalletCore::from_env().await?;
    let voters_dir = std::env::var("VOTERS_DIR").unwrap_or_else(|_| ".".to_owned());

    // --- 1. import the genesis funder (public account 0 from testnet_initial_state) -------------
    let funder = testnet_initial_state::initial_pub_accounts_private_keys()
        .into_iter()
        .next()
        .expect("testnet_initial_state seeds at least one public account");
    let funder_id = funder.account_id;
    wallet
        .storage_mut()
        .key_chain_mut()
        .add_imported_public_account(funder.pub_sign_key);

    // --- 2. import member voting KeyChains as OWNED private accounts (default/uninit state) ------
    for index in VOTER_INDICES {
        let key_chain = msig_demo::member_key_chain(index);
        let npk = key_chain.nullifier_public_key;
        let vpk = key_chain.viewing_public_key.clone();
        let voting_id = AccountId::for_regular_private_account(&npk, &vpk, msig_demo::VOTE_IDENTIFIER);

        wallet
            .storage_mut()
            .key_chain_mut()
            .add_imported_private_account(
                key_chain,
                None,
                msig_demo::VOTE_IDENTIFIER,
                Account::default(),
            );

        // keys file (npk line, vpk line) — same format as `wallet account show-keys`.
        let keys_path = format!("{voters_dir}/member{index}.keys");
        let mut f = std::fs::File::create(&keys_path)?;
        writeln!(f, "{}", hex::encode(npk.0))?;
        writeln!(f, "{}", hex::encode(vpk.to_bytes()))?;

        println!("MEMBER{index}_VOTING_ID={voting_id}");
        println!("MEMBER{index}_NPK={}", hex::encode(npk.0));
    }

    wallet.store_persistent_data()?;
    println!("FUNDER_ID={funder_id}");
    Ok(())
}
