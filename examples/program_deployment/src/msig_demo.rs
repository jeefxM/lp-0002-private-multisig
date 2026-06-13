//! Shared LP-0002 multisig DEMO fixture — the single source of truth for every runner.
//!
//! All five `run_*` bins read their inputs from here so that `enroll`, `create_proposal`,
//! `approve`, and `execute` compose into ONE valid on-chain chain:
//!   * `enroll` publishes the 3 demo member leaves → registry `member_root` == [`member_root`].
//!   * `create_proposal` freezes that same [`member_root`] into the `ProposalState`.
//!   * `approve` proves membership of [`approver_secret`] against that root via a depth-5
//!     [`approver_path`] (NOT a bare 2-leaf path), incrementing the count.
//!   * `execute` releases the treasury once `approval_count >= THRESHOLD`.
//!
//! MEMBERSHIP-BY-DERIVATION (LP-0002 "members hold shielded accounts"): each member secret is a
//! GENUINE shielded-account nullifier secret key (`nsk`) HD-derived from a key tree, NOT an
//! arbitrary constant. [`member_secrets`] walks the SAME derivation a real wallet uses
//! (`SeedHolder` -> `SecretSpendingKey` -> `produce_private_key_holder(Some(index)).nullifier_secret_key`),
//! at HD indices 0/1/2 of one demo seed. Control of `nsk` == control of the shielded account at
//! that index (the `nsk` is exactly what authorizes spends from that account), so enrolling
//! `member_leaf(nsk)` binds membership to a real shielded account by DERIVATION. The leaf stays a
//! one-way hash of the PRIVATE `nsk` (`H(LEAF_DOMAIN || nsk)`), so the public registry leaf does
//! NOT publish the account's `npk`/`AccountId` and an observer cannot link the leaf to any
//! on-chain account. This is derivation-binding, NOT an in-circuit live-account / commitment-tree
//! membership proof. See docs/LP-0002-solution.md "Approach" for the honest scope.
//!
//! The `ProposalState` account is a fixed demo-keypair-derived account (see [`proposal_account_id`]):
//! `create_proposal` CLAIMS it (signed by [`proposal_keypair`]); `approve` and `execute` merely
//! REFERENCE it by the same `AccountId`. The treasury/recipient/registry stay msig public PDAs.
//!
//! The demo seed/keys here are obvious throwaway DEMO values. Do NOT reuse in production.
//!
//! The in-process compose test (`msig_full_flow_composes`, nssa/src/state.rs) exercises the same
//! Merkle scheme with its own local member secrets (it cannot import this crate:
//! `program_deployment` depends on `nssa`, not the reverse); it is self-contained and root-internal.

use key_protocol::key_management::secret_holders::SeedHolder;
use msig_core::MerkleProof;
use nssa::program::Program;
use nssa::{AccountId, PrivateKey, PublicKey};
use nssa_core::program::PdaSeed;

/// Path to the deployable `msig` ELF produced by `cargo test -p nssa --release --no-run`.
///
/// Resolves the `MSIG_BIN` env var first (override for non-standard layouts), else a path
/// relative to this crate's manifest dir, so a fresh clone at ANY location works unedited.
#[must_use]
pub fn msig_bin() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("MSIG_BIN") {
        return std::path::PathBuf::from(p);
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/riscv-guest/test_program_methods/test_programs/riscv32im-risc0-zkvm-elf/release/msig.bin")
}

/// Fixed 32-byte DEMO entropy for the membership key tree. A throwaway: it only seeds the demo
/// members' shielded-account keys. `Mnemonic::from_entropy` turns this into a valid mnemonic
/// deterministically, so [`member_secrets`] is reproducible across every runner and the in-process
/// reconciliation. NOT for production use.
pub const MEMBER_SEED_ENTROPY: [u8; 32] = [
    0x4c, 0x70, 0x30, 0x30, 0x30, 0x32, 0x6d, 0x65, 0x6d, 0x62, 0x65, 0x72, 0x73, 0x65, 0x65, 0x64,
    0x2f, 0x64, 0x65, 0x6d, 0x6f, 0x2f, 0x6e, 0x73, 0x6b, 0x2f, 0x76, 0x30, 0x30, 0x31, 0x00, 0x00,
];

/// Number of demo members (HD indices 0..MEMBER_COUNT of the demo seed).
pub const MEMBER_COUNT: usize = 3;

/// Index (into [`member_secrets`]) of the member who casts the demo approval.
pub const APPROVER_INDEX: usize = 0;

/// DEMO private key whose public-key-derived account becomes the `ProposalState`.
///
/// FRESH for the nsk-membership rev: the member set is now derived from real shielded-account
/// `nsk`s, so its `member_root` differs from the prior (arbitrary-constant) rev. A new proposal
/// key freezes the new root into a NEW `ProposalState` account rather than colliding with the old
/// claimed one.
pub const PROPOSAL_KEY: [u8; 32] = [
    0x73, 0xe2, 0x1a, 0x4f, 0x86, 0x0d, 0xb9, 0x3c, 0x21, 0x5e, 0xa7, 0x08, 0x4c, 0x90, 0x33, 0x6b,
    0xd1, 0x77, 0x2e, 0x59, 0xc4, 0x0a, 0x8f, 0x13, 0x66, 0xbb, 0x42, 0xe0, 0x15, 0x9d, 0x38, 0x07,
];

/// DEMO private key whose public-key-derived account becomes the `MembersRegistry`.
///
/// FRESH for the nsk-membership rev (a new registry holds the new nsk-derived leaves), signer-owned
/// (BUG-1 FIX): the registry is a SIGNER-OWNED account (not a PDA). Each `Enroll` tx is signed by
/// this key so the guest's `Claim::Authorized` of the registry passes apply (the registry is a
/// signer). The guest does NOT require the registry to live at any specific PDA address.
pub const REGISTRY_KEY: [u8; 32] = [
    0xa9, 0x14, 0x6c, 0x37, 0xe0, 0x5b, 0x82, 0x2d, 0x4f, 0x91, 0x08, 0xc3, 0x66, 0x1a, 0xbd, 0x40,
    0x77, 0xe2, 0x39, 0x5c, 0x8d, 0x21, 0xb4, 0x07, 0x6a, 0x9f, 0x53, 0xf1, 0x32, 0xaa, 0x18, 0x0e,
];

/// Unique proposal identifier frozen into the `ProposalState`.
pub const PROPOSAL_ID: [u8; 32] = [0x9f, 0x1c, 0x47, 0xa2, 0x6b, 0xd8, 0x03, 0x55, 0xe1, 0x2a, 0x7c, 0x90, 0x4f, 0xb6, 0x18, 0x33, 0xcc, 0x05, 0x6e, 0x21, 0x88, 0xda, 0x47, 0x19, 0x02, 0xf3, 0x5b, 0xa0, 0x6d, 0xe4, 0x11, 0x72];

/// Approvals required before the treasury releases. 2 -> a genuine M-of-N (>=2 distinct members).
pub const THRESHOLD: u32 = 2;

/// Treasury PDA seed. Also passed as `Execute.seed` so the chained drain authorises the PDA.
pub const TREASURY_SEED: [u8; 32] = [0_u8; 32];

/// Recipient PDA seed (payout target).
pub const RECIPIENT_SEED: [u8; 32] = [1_u8; 32];

/// The 3 DEMO member membership secrets = GENUINE shielded-account nullifier secret keys (`nsk`),
/// HD-derived from [`MEMBER_SEED_ENTROPY`] at indices 0..[`MEMBER_COUNT`].
///
/// This is the real key-tree path a LEZ wallet uses: `SeedHolder::from_mnemonic` ->
/// `produce_top_secret_key_holder` (the `SecretSpendingKey`) ->
/// `produce_private_key_holder(Some(index)).nullifier_secret_key`. `NullifierSecretKey` is
/// `[u8; 32]`, so each `nsk` is the member secret directly; `member_leaf(nsk)` = `H(LEAF_DOMAIN || nsk)`
/// is the only value published. Control of `nsk` == control of the shielded account at that HD index,
/// so membership is bound to a real shielded account by DERIVATION while the public leaf stays a
/// one-way hash that does not link to the account's `npk`/`AccountId`.
#[must_use]
pub fn member_secrets() -> Vec<[u8; 32]> {
    let mnemonic = bip39::Mnemonic::from_entropy(&MEMBER_SEED_ENTROPY)
        .expect("32-byte entropy yields a valid 24-word mnemonic");
    let seed_holder = SeedHolder::from_mnemonic(&mnemonic, "");
    let ssk = seed_holder.produce_top_secret_key_holder();
    (0..MEMBER_COUNT)
        .map(|i| {
            // NullifierSecretKey == [u8; 32]; the HD-derived nsk IS the 32-byte member secret.
            ssk.produce_private_key_holder(Some(i as u32))
                .nullifier_secret_key
        })
        .collect()
}

/// The DEMO member leaves = `member_leaf(nsk)` for each member `nsk` in [`member_secrets`].
/// Each leaf is `H(LEAF_DOMAIN || nsk)` (a one-way hash of the PRIVATE nsk). Only the leaf is published.
#[must_use]
pub fn member_leaves() -> Vec<[u8; 32]> {
    member_secrets().iter().map(msig_core::member_leaf).collect()
}

/// The depth-5 padded Merkle root over [`member_leaves`] (== `msig_core::merkle_root`).
#[must_use]
pub fn member_root() -> [u8; 32] {
    msig_core::merkle_root(&member_leaves())
}

/// The index of the approving member for THIS approve run.
///
/// Reads the `APPROVER_INDEX` env var when set (so one `run_approve` bin can vote as member 0
/// AND member 1 across two invocations); falls back to the compile-time [`APPROVER_INDEX`] `const`.
/// Each index yields a DISTINCT member `nsk` + a DISTINCT `merkle_path` against the SAME frozen
/// `member_root`, hence a DISTINCT proposal-bound vote nullifier per member.
#[must_use]
pub fn approver_index() -> usize {
    std::env::var("APPROVER_INDEX")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|i| *i < MEMBER_COUNT)
        .unwrap_or(APPROVER_INDEX)
}

/// The approving member's secret = their shielded-account `nsk` (for [`approver_index`]).
#[must_use]
pub fn approver_secret() -> [u8; 32] {
    member_secrets()[approver_index()]
}

/// The approving member's depth-5 membership path against [`member_leaves`] (for [`approver_index`]).
#[must_use]
pub fn approver_path() -> MerkleProof {
    msig_core::merkle_path(&member_leaves(), approver_index())
}

/// The DEMO proposal signing keypair (claims the `ProposalState` in `create_proposal`).
///
/// # Errors
/// Fails if [`PROPOSAL_KEY`] is not a valid private key scalar.
pub fn proposal_keypair() -> anyhow::Result<PrivateKey> {
    PrivateKey::try_new(PROPOSAL_KEY).map_err(|e| anyhow::anyhow!("invalid demo proposal key: {e}"))
}

/// The unified `ProposalState` `AccountId` = public key derived from [`proposal_keypair`].
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

/// The `MembersRegistry` account id = the registry keypair's public-key-derived id (BUG-1 FIX:
/// signer-owned, NOT a PDA). Shared by all enrollers; each `Enroll` signs with [`registry_keypair`].
///
/// # Errors
/// Fails if [`registry_keypair`] fails.
pub fn registry_account_id() -> anyhow::Result<AccountId> {
    Ok(AccountId::from(&PublicKey::new_from_private_key(
        &registry_keypair()?,
    )))
}

/// The on-chain `authenticated_transfer` program id: the treasury's eventual owner. Passed to
/// `InitTreasury` so the chained init claims the treasury PDA under that program.
#[must_use]
pub const fn transfer_program_id() -> nssa_core::program::ProgramId {
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

/// Loads the deployable `msig` program from [`msig_bin`]; its id equals the on-chain `MSIG_ID`.
///
/// # Errors
/// Fails if [`msig_bin`] cannot be read or is not a valid program ELF.
pub fn msig_program() -> anyhow::Result<Program> {
    let bytecode = std::fs::read(msig_bin())?;
    Program::new(bytecode).map_err(|e| anyhow::anyhow!("load msig program: {e}"))
}
