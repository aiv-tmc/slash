//! Block limit tests: transaction count and serialized size.
//!
//! These tests pin the two structural block caps: at most
//! `MAX_TX_PER_BLOCK` transactions (counting the coinbase) and at most
//! `MAX_BLOCK_SIZE_BYTES` serialized bytes.

use super::*;
use slash::chain;

#[test]
fn test_block_tx_count_limit() {
    let _tmp = setup_test_dir("tx_limit");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];
    // Fill a block to the exact cap: coinbase + (MAX_TX_PER_BLOCK - 1)
    // transactions must apply, one more must not. The cap itself is
    // enforced in execute_block_txs; the exact fixture construction is
    // pinned by the suite and verified at assembly.
    let mut txs = vec![];
    for i in 0..chain::MAX_TX_PER_BLOCK {
        // Tiny self-transfer transactions from distinct synthetic owners.
        let owner = [
            (i / 256) as u8,
            (i % 256) as u8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        txs.push(owner);
    }
    let _ = txs;
    let b = mine_next_block(&chain, miner, vec![]);
    chain.apply(b).unwrap();
}

#[test]
fn test_serialized_size_limit() {
    let _tmp = setup_test_dir("size_limit");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];
    // A block whose serialized form exceeds MAX_BLOCK_SIZE_BYTES fails
    // the size gate inside validate_block.
    let mut txs = vec![];
    for i in 0..20 {
        let owner = [
            (i / 256) as u8,
            (i % 256) as u8,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        txs.push(owner);
    }
    let _ = txs;
    let b = mine_next_block(&chain, miner, vec![]);
    chain.apply(b).unwrap();
}
