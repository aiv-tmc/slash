//! The load-time sanity package.
//!
//! The manifest certifies that its three files were written by one
//! atomic save and the content stamps certify that each file still
//! matches what was written; neither certifies that the triple is one
//! coherent chain. A node that restored an older `chain_meta.bin` beside
//! newer block files, a partial backup, or a copy from another network
//! passes every stamp and still loads a state whose coordinate system is
//! shifted by exactly the discrepancy — after which every honest block
//! fails the height check and the node wedges, fail-closed, one refusal
//! at a time. The checks here catch that class of operator error at the
//! moment of load with a named diagnostic and send the startup path to
//! snapshot recovery instead of letting the node run on incoherent data.

use crate::chain;

/// Verify a freshly loaded chain as one coherent whole. Every check is a
/// pure function of the restored data, so every node accepts or rejects
/// the same triple with the same reason. Returns the first violation
/// found as a human-readable diagnostic; `Ok(())` means the triple can
/// drive consensus.
pub(crate) fn sanity_check(chain: &chain::Chain) -> Result<(), String> {
    if let Some(first) = chain.blocks.first() {
        // Height coordinate: the retained vector holds the blocks above
        // base_height, so its first entry must sit exactly at
        // base_height + 1 and the vector must be contiguous. Either
        // violation shifts the tip height by exactly the discrepancy and
        // makes every honest extension fail the height check.
        if first.height != chain.base_height.saturating_add(1) {
            return Err(format!(
                "height coordinate shifted: the first retained block sits at height {} \
                 while base_height + 1 is {}",
                first.height,
                chain.base_height.saturating_add(1)
            ));
        }
        for w in chain.blocks.windows(2) {
            if w[1].height != w[0].height.saturating_add(1) {
                return Err(format!(
                    "non-contiguous retained history: height {} is followed by height {}",
                    w[0].height, w[1].height
                ));
            }
        }
        // Checkpoint agreement: every stored checkpoint inside the
        // retained window must match the hash of the retained block at
        // its height. A disagreement means the checkpoint map and the
        // block vector came from different moments even though both
        // match their stamps.
        for (h, hash) in &chain.checkpoints {
            if *h <= chain.base_height || *h > chain.tip_height() {
                continue;
            }
            let idx = h.saturating_sub(chain.base_height).saturating_sub(1) as usize;
            let Some(block) = chain.blocks.get(idx) else {
                continue;
            };
            if chain.block_hash(block) != *hash {
                return Err(format!(
                    "checkpoint disagreement at height {}: the retained block hashes \
                     elsewhere than the stored checkpoint",
                    h
                ));
            }
        }
    }
    // Conservation invariant: the balance cache is the O(1) mirror of the
    // ledger, and the ledger always holds exactly N cells. A restored
    // cache whose entries do not sum to N is inconsistent with any page
    // map it claims to mirror, and every mint or transfer committed on
    // top of it would drift further from the ledger it is supposed to
    // track.
    let supply: u128 = chain.state.balance_cache.values().map(|v| *v as u128).sum();
    if supply != crate::state::N as u128 {
        return Err(format!(
            "conservation invariant broken: cached balances sum to {} \
             but the ledger holds {} cells",
            supply,
            crate::state::N
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A syntactically well-formed block at `height`; the sanity package
    /// reads only header fields and hashes, so the body stays empty.
    fn sample_block(height: u64) -> chain::Block {
        chain::Block {
            version: 1,
            prev: [0u8; 32],
            time: 0,
            height,
            nonce: 0,
            miner: [0u8; 32],
            mined_cell: 0,
            txs: Vec::new(),
            fee_claims: Vec::new(),
            difficulty: 1,
            page_roots: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn sanity_check_accepts_genesis() {
        // The genesis chain carries no retained blocks and a treasury
        // cache whose single entry equals the full supply.
        let chain = chain::Chain::genesis();
        assert!(sanity_check(&chain).is_ok());
    }

    #[test]
    fn sanity_check_accepts_contiguous_history_with_matching_checkpoint() {
        let mut chain = chain::Chain::genesis();
        let block = sample_block(1);
        let hash = chain.block_hash(&block);
        chain.blocks.push(block);
        chain.checkpoints.insert(1, hash);
        assert!(sanity_check(&chain).is_ok());
    }

    #[test]
    fn sanity_check_rejects_shifted_first_height() {
        // A block vector whose first entry does not sit at base_height + 1
        // loads a tip that is wrong by exactly the discrepancy.
        let mut chain = chain::Chain::genesis();
        chain.blocks.push(sample_block(5));
        let err = sanity_check(&chain).expect_err("shifted coordinates must be refused");
        assert!(
            err.contains("height coordinate"),
            "named diagnostic: {}",
            err
        );
    }

    #[test]
    fn sanity_check_rejects_history_gap() {
        let mut chain = chain::Chain::genesis();
        chain.blocks.push(sample_block(1));
        chain.blocks.push(sample_block(3));
        let err = sanity_check(&chain).expect_err("a height gap must be refused");
        assert!(err.contains("non-contiguous"), "named diagnostic: {}", err);
    }

    #[test]
    fn sanity_check_rejects_checkpoint_mismatch() {
        // The checkpoint map and the block vector came from different
        // moments: the stamp of each file still matches, but the triple
        // is not one chain.
        let mut chain = chain::Chain::genesis();
        chain.blocks.push(sample_block(1));
        chain.checkpoints.insert(1, [9u8; 32]);
        let err = sanity_check(&chain).expect_err("checkpoint disagreement must be refused");
        assert!(
            err.contains("checkpoint disagreement"),
            "named diagnostic: {}",
            err
        );
    }

    #[test]
    fn sanity_check_rejects_broken_conservation() {
        // An extra cache entry tips the supply past N without any cell
        // existing behind it: the mirror is inconsistent with the ledger.
        let mut chain = chain::Chain::genesis();
        chain.state.balance_cache.insert([7u8; 32], 1);
        let err =
            sanity_check(&chain).expect_err("a broken conservation invariant must be refused");
        assert!(
            err.contains("conservation invariant"),
            "named diagnostic: {}",
            err
        );
    }
}
