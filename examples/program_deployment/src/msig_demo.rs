//! Shared LP-0002 multisig DEMO fixture — the single source of truth for every runner.
//!
//! All five `run_*` bins read their inputs from here so that `enroll`, `create_proposal`,
//! `approve`, and `execute` compose into ONE valid on-chain chain:
//!   * `enroll` publishes the 3 demo member leaves → registry `member_root` == [`member_root`].
//!   * `create_proposal` freezes that same [`member_root`] into the ProposalState.
//!   * `approve` proves membership of [`approver_secret`] against that root via a depth-5
//!     [`approver_path`] (NOT a bare 2-leaf path), incrementing the count.
//!   * `execute` releases the treasury once `approval_count >= THRESHOLD`.
//!
//! The ProposalState account is a fixed demo-keypair-derived account (see [`proposal_account_id`]):
//! `create_proposal` CLAIMS it (signed by [`proposal_keypair`]); `approve` and `execute` merely
//! REFERENCE it by the same `AccountId`. The treasury/recipient/registry stay msig public PDAs.
//!
//! Every secret/key here is an obvious throwaway DEMO const. Do NOT reuse in production.
//!
//! The in-process compose test (`msig_full_flow_composes`, nssa/src/state.rs) hardcodes these
//! same values (it cannot import this crate — `program_deployment` depends on `nssa`, not the
//! reverse). Keep the two in sync.

use msig_core::MerkleProof;
use nssa::program::Program;
use nssa::{AccountId, PrivateKey, PublicKey};
use nssa_core::program::PdaSeed;

/// Deployable `msig` ELF produced by `cargo test -p nssa --release --no-run`.
pub const MSIG_BIN: &str =
    "/root/lez-v012/target/riscv-guest/test_program_methods/test_programs/riscv32im-risc0-zkvm-elf/release/msig.bin";

/// Three distinct DEMO member secrets. Only their leaves are ever published.
pub const MEMBER_SECRETS: [[u8; 32]; 3] = [[0xA7u8; 32], [0x42u8; 32], [0x5Cu8; 32]];

/// Index (into [`MEMBER_SECRETS`]) of the member who casts the demo approval.
pub const APPROVER_INDEX: usize = 0;

/// DEMO private key whose public-key-derived account becomes the ProposalState.
pub const PROPOSAL_KEY: [u8; 32] = [0xd7, 0xd1, 0xfa, 0x65, 0xa6, 0xa0, 0x99, 0x51, 0x84, 0xbe, 0x69, 0x65, 0x16, 0x56, 0x10, 0x49, 0x96, 0xc5, 0x99, 0x65, 0x9c, 0x36, 0xbd, 0x42, 0xf0, 0x7c, 0x56, 0xca, 0xca, 0xbd, 0x0d, 0x09];

/// DEMO private key whose public-key-derived account becomes the MembersRegistry.
///
/// BUG-1 FIX: the registry is a SIGNER-OWNED account (not a PDA). Each `Enroll` tx is signed by
/// this key so the guest's `Claim::Authorized` of the registry passes apply (the registry is a
/// signer). The guest does NOT require the registry to live at any specific PDA address.
pub const REGISTRY_KEY: [u8; 32] = [0xCCu8; 32];

/// Unique proposal identifier frozen into the ProposalState.
pub const PROPOSAL_ID: [u8; 32] = [0x32, 0xc3, 0x88, 0x2b, 0x1e, 0x00, 0xbf, 0x3f, 0xdc, 0x86, 0xbd, 0x64, 0xe6, 0xf1, 0x8d, 0xde, 0x0a, 0xe0, 0x55, 0x59, 0xe4, 0xcd, 0x8b, 0x06, 0xfb, 0x3f, 0xb4, 0x14, 0x24, 0x9c, 0xff, 0x9a];

/// Approvals required before the treasury releases. 1 → a single demo approval meets it.
pub const THRESHOLD: u32 = 1;

/// Treasury PDA seed. Also passed as `Execute.seed` so the chained drain authorises the PDA.
pub const TREASURY_SEED: [u8; 32] = [0u8; 32];

/// Recipient PDA seed (payout target).
pub const RECIPIENT_SEED: [u8; 32] = [1u8; 32];

/// The DEMO member leaves = `member_leaf(secret)` for each secret in [`MEMBER_SECRETS`].
#[must_use]
pub fn member_leaves() -> Vec<[u8; 32]> {
    MEMBER_SECRETS.iter().map(msig_core::member_leaf).collect()
}

/// The depth-5 padded Merkle root over [`member_leaves`] (== `msig_core::merkle_root`).
#[must_use]
pub fn member_root() -> [u8; 32] {
    msig_core::merkle_root(&member_leaves())
}

/// The approving member's secret.
#[must_use]
pub fn approver_secret() -> [u8; 32] {
    MEMBER_SECRETS[APPROVER_INDEX]
}

/// The approving member's depth-5 membership path against [`member_leaves`].
#[must_use]
pub fn approver_path() -> MerkleProof {
    msig_core::merkle_path(&member_leaves(), APPROVER_INDEX)
}

/// The DEMO proposal signing keypair (claims the ProposalState in `create_proposal`).
///
/// # Errors
/// Fails if [`PROPOSAL_KEY`] is not a valid private key scalar.
pub fn proposal_keypair() -> anyhow::Result<PrivateKey> {
    PrivateKey::try_new(PROPOSAL_KEY).map_err(|e| anyhow::anyhow!("invalid demo proposal key: {e}"))
}

/// The unified ProposalState `AccountId` = public key derived from [`proposal_keypair`].
///
/// `create_proposal` claims this id; `approve` and `execute` reference the SAME id.
///
/// # Errors
/// Fails if [`proposal_keypair`] fails.
pub fn proposal_account_id() -> anyhow::Result<AccountId> {
    Ok(AccountId::from(&PublicKey::new_from_private_key(
        &proposal_keypair()?,
    )))
}

/// The DEMO registry signing keypair (signs every `Enroll`, so the guest's `Claim::Authorized`
/// of the registry passes apply).
///
/// # Errors
/// Fails if [`REGISTRY_KEY`] is not a valid private key scalar.
pub fn registry_keypair() -> anyhow::Result<PrivateKey> {
    PrivateKey::try_new(REGISTRY_KEY).map_err(|e| anyhow::anyhow!("invalid demo registry key: {e}"))
}

/// The MembersRegistry account id = the registry keypair's public-key-derived id (BUG-1 FIX:
/// signer-owned, NOT a PDA). Shared by all enrollers; each `Enroll` signs with [`registry_keypair`].
///
/// # Errors
/// Fails if [`registry_keypair`] fails.
pub fn registry_account_id() -> anyhow::Result<AccountId> {
    Ok(AccountId::from(&PublicKey::new_from_private_key(
        &registry_keypair()?,
    )))
}

/// The on-chain `authenticated_transfer` program id — the treasury's eventual owner. Passed to
/// `InitTreasury` so the chained init claims the treasury PDA under that program.
#[must_use]
pub fn transfer_program_id() -> nssa_core::program::ProgramId {
    nssa::program_methods::AUTHENTICATED_TRANSFER_ID
}

/// The treasury account id (a public PDA of msig); funds drain from here on execute.
#[must_use]
pub fn treasury_account_id(program_id: &nssa_core::program::ProgramId) -> AccountId {
    AccountId::for_public_pda(program_id, &PdaSeed::new(TREASURY_SEED))
}

/// The recipient account id (a public PDA of msig); the payout target.
#[must_use]
pub fn recipient_account_id(program_id: &nssa_core::program::ProgramId) -> AccountId {
    AccountId::for_public_pda(program_id, &PdaSeed::new(RECIPIENT_SEED))
}

/// Loads the deployable `msig` program from [`MSIG_BIN`]; its id equals the on-chain MSIG_ID.
///
/// # Errors
/// Fails if [`MSIG_BIN`] cannot be read or is not a valid program ELF.
pub fn msig_program() -> anyhow::Result<Program> {
    let bytecode = std::fs::read(MSIG_BIN)?;
    Program::new(bytecode).map_err(|e| anyhow::anyhow!("load msig program: {e}"))
}
