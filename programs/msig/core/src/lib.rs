//! LP-0002 shared types + the Semaphore-style member Merkle scheme.
//!
//! Used by the on-chain `msig` guests (registry + approve), the in-circuit tests, and the
//! client runners, so every party computes `member_root`, leaves, and vote nullifiers the
//! same way. Hashing uses `risc0_zkvm::sha` so it is byte-identical in-guest and host-side.

use risc0_zkvm::sha::{Impl, Sha256 as _};
use serde::{Deserialize, Serialize};

/// A Merkle membership proof: `(leaf_index, sibling_path bottom-up)`.
pub type MerkleProof = (u32, Vec<[u8; 32]>);

/// Fixed tree depth (2^DEPTH member slots). Depth 5 = 32 members; ample for a demo multisig.
pub const TREE_DEPTH: usize = 5;
/// Unused leaf slots are this empty value.
pub const EMPTY_LEAF: [u8; 32] = [0u8; 32];

/// Domain separators.
pub const LEAF_DOMAIN: &[u8] = b"/lp0002/leaf/\x00";
pub const NULL_DOMAIN: &[u8] = b"/lp0002/null/\x00";

/// ProposalState `data` layout (little-endian):
///   [0..32]   member_root
///   [32..64]  proposal_id
///   [64..68]  approval_count: u32
///   [68..]    approval_count * 32-byte vote nullifiers
pub const PROPOSAL_HEADER_LEN: usize = 68;

/// MembersRegistry `data` layout (little-endian):
///   [0..32]   member_root
///   [32..36]  leaf_count: u32
///   [36..]    leaf_count * 32-byte member leaves
pub const REGISTRY_HEADER_LEN: usize = 36;

/// The shared instruction set of the unified `msig` program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MsigInstruction {
    /// Initialize (claim) a ProposalState, freezing `member_root` for this proposal.
    CreateProposal {
        member_root: [u8; 32],
        proposal_id: [u8; 32],
    },
    /// Anonymous approval: prove membership in `member_root` and record a proposal-bound nullifier.
    Approve {
        secret: [u8; 32],
        merkle_path: MerkleProof,
        proposal_id: [u8; 32],
    },
    /// Append a member's public leaf to the MembersRegistry and recompute `member_root`.
    Enroll { leaf: [u8; 32] },
    /// Threshold-gated treasury release: once approval_count >= threshold, drain the proposal's
    /// treasury (a PDA of this program owned by authenticated_transfer, authorized by `seed`) to
    /// the recipient via a chained call.
    Execute { threshold: u32, seed: [u8; 32] },
}

#[must_use]
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Impl::hash_bytes(bytes).as_bytes().try_into().unwrap()
}

#[must_use]
pub fn hash_pair(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(left);
    buf[32..].copy_from_slice(right);
    sha256(&buf)
}

/// A member's public leaf = SHA256(LEAF_DOMAIN || secret). Only the leaf is ever published.
#[must_use]
pub fn member_leaf(secret: &[u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(LEAF_DOMAIN.len() + 32);
    buf.extend_from_slice(LEAF_DOMAIN);
    buf.extend_from_slice(secret);
    sha256(&buf)
}

/// A proposal-bound vote nullifier = SHA256(NULL_DOMAIN || secret || proposal_id).
#[must_use]
pub fn vote_nullifier(secret: &[u8; 32], proposal_id: &[u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(NULL_DOMAIN.len() + 64);
    buf.extend_from_slice(NULL_DOMAIN);
    buf.extend_from_slice(secret);
    buf.extend_from_slice(proposal_id);
    sha256(&buf)
}

/// Builds the fixed-depth Merkle root over `leaves` (padded with EMPTY_LEAF to 2^TREE_DEPTH).
#[must_use]
pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    level.resize(1 << TREE_DEPTH, EMPTY_LEAF);
    for _ in 0..TREE_DEPTH {
        level = level.chunks(2).map(|c| hash_pair(&c[0], &c[1])).collect();
    }
    level[0]
}

/// Generates the bottom-up sibling path for the leaf at `index`.
#[must_use]
pub fn merkle_path(leaves: &[[u8; 32]], index: usize) -> MerkleProof {
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
    level.resize(1 << TREE_DEPTH, EMPTY_LEAF);
    let mut idx = index;
    let mut path = Vec::with_capacity(TREE_DEPTH);
    for _ in 0..TREE_DEPTH {
        path.push(level[idx ^ 1]);
        level = level.chunks(2).map(|c| hash_pair(&c[0], &c[1])).collect();
        idx >>= 1;
    }
    (index as u32, path)
}

/// Recomputes the root from a leaf + its membership proof (mirrors `merkle_root`'s ordering).
#[must_use]
pub fn root_from_path(leaf: [u8; 32], proof: &MerkleProof) -> [u8; 32] {
    let mut result = leaf;
    let mut idx = proof.0;
    for sibling in &proof.1 {
        result = if idx & 1 == 0 {
            hash_pair(&result, sibling)
        } else {
            hash_pair(sibling, &result)
        };
        idx >>= 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // DEMO fixture values — kept in sync with
    // examples/program_deployment/src/msig_demo.rs (which cannot be imported here:
    // program_deployment depends on this crate, not the reverse).
    const MEMBER_SECRETS: [[u8; 32]; 3] = [[0xA7u8; 32], [0x42u8; 32], [0x5Cu8; 32]];
    const APPROVER_INDEX: usize = 0;

    /// The reconciliation invariant: the approver's depth-5 membership path, replayed from their
    /// leaf, reproduces EXACTLY the padded `merkle_root` of the enrolled member set — which is the
    /// root `create_proposal` freezes and the `approve` guest checks against. This is the precise
    /// consistency the runner reconciliation must guarantee (the old 2-leaf path broke it).
    #[test]
    fn approver_path_reproduces_member_root() {
        let leaves: Vec<[u8; 32]> = MEMBER_SECRETS.iter().map(member_leaf).collect();
        let root = merkle_root(&leaves);
        let proof = merkle_path(&leaves, APPROVER_INDEX);
        let approver_leaf = member_leaf(&MEMBER_SECRETS[APPROVER_INDEX]);
        assert_eq!(
            root_from_path(approver_leaf, &proof),
            root,
            "approver depth-5 path must reproduce the enrolled member_root"
        );
    }
}
