//! The public page proof: the structure a light client receives and the
//! verification rule it runs. Verification folds the leaf hash with the
//! sibling path, ordering every pair by the running index parity —
//! exactly the pairing the cached tree was built with — and accepts when
//! the folded value equals the claimed root.

use serde::{Deserialize, Serialize};

/// A Merkle proof for one leaf of a page tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageProof {
    /// Leaf hash: blake3(start || end || owner).
    pub leaf_hash: [u8; 32],
    /// Sibling hashes from the leaf up to the root.
    pub siblings: Vec<[u8; 32]>,
    /// Position of the leaf in the sorted range list.
    pub leaf_index: usize,
}

/// Verify a page proof against a claimed root. Each step orders the pair
/// by the running index parity, exactly mirroring how the tree was built.
pub fn verify_page_proof(root: &[u8; 32], proof: &PageProof) -> bool {
    let mut h = proof.leaf_hash;
    let mut idx = proof.leaf_index;
    for sibling in &proof.siblings {
        let mut hasher = blake3::Hasher::new();
        if idx.is_multiple_of(2) {
            hasher.update(&h);
            hasher.update(sibling);
        } else {
            hasher.update(sibling);
            hasher.update(&h);
        }
        h = *hasher.finalize().as_bytes();
        idx /= 2;
    }
    &h == root
}
