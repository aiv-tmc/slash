//! Shared helper functions of the `emission_harness_tests` suite.
//!
//! These helpers build blocks, transactions, and fixtures for the
//! test modules of this crate; they carry no `#[test]` items themselves.

use super::*;

/// Position the retained window so that the next expected height equals
/// `next_height` without replaying millions of blocks: `base_height` is
/// moved while the block vector keeps a single genesis block, so
/// `blocks.len() + base_height` lands on the requested height. Every
/// consensus rule that depends on the absolute height (subsidy era,
/// coinbase requirement) reads exactly this sum.
pub fn chain_with_next_height(next_height: u64) -> chain::Chain {
    let mut chain = chain::Chain::genesis();
    chain.base_height = next_height - 1;
    chain
}

/// Build and fully prepare a block extending the given chain at its
/// expected next height. When `with_coinbase` is set, the mandatory coinbase
/// is attached first; the nonce is ground until the proof-of-work slot
/// lands on a treasury-owned cell, mirroring the consensus slot rule.
pub fn prepared_tail_block(chain: &chain::Chain, miner: [u8; 32], with_coinbase: bool) -> Block {
    let height = chain.blocks.len() as u64 + chain.base_height;
    let coinbase = if with_coinbase {
        chain::Chain::build_coinbase(&chain.state, miner, height, 0)
    } else {
        None
    };
    let mut txs: Vec<Tx> = Vec::new();
    if let Some(cb) = coinbase {
        txs.push(cb);
    }
    let mut block = Block {
        version: 1,
        prev: chain.tip_hash(),
        time: chrono::Utc::now().timestamp() as u64,
        height,
        nonce: 0,
        miner,
        mined_cell: 0,
        txs,
        fee_claims: vec![],
        difficulty: chain.next_difficulty(),
        page_roots: BTreeMap::new(),
    };
    loop {
        if let Some(cell) = chain::verify_pow(
            block.prev,
            block.time,
            block.height,
            block.miner,
            block.nonce,
            block.difficulty,
        ) {
            let treasury_owned = chain
                .state
                .get(cell)
                .is_some_and(|(_, _, owner)| owner == TREASURY);
            if treasury_owned {
                block.mined_cell = cell;
                break;
            }
        }
        block.nonce += 1;
    }
    chain.prepare_block(block)
}
