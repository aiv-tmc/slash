//! `test_mempool_cap_eviction_simulated` — one test case of the
//! `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_mempool_cap_eviction_simulated() {
    let _tmp = setup_test_dir("mempool_cap");
    reset_testnet();
    let chain = chain::Chain::genesis();

    // One transaction more than the mempool capacity is also more than the
    // per-block transaction cap, so the block must be invalid either way.
    // The transactions are injected into a valid empty template: the mining
    // fixture panics on templates it cannot execute, and a block this large
    // is refused by the size gate before any transaction work.
    let mut b = mine_next_block(&chain, [1u8; 32], vec![]);
    b.txs = (0..=p2p::MEMPOOL_CAP)
        .map(|i| {
            let start = i as u64 * 50;
            chain::Tx {
                from: state::TREASURY,
                inputs: vec![(start, start + 50)],
                outputs: vec![chain::Output {
                    start,
                    end: start + 50,
                    to: [1u8; 32],
                    lock: None,
                }],
                sig: vec![0u8; 64],
                scheme: 0,
                memo: None,
            }
        })
        .collect();
    assert!(b.txs.len() > chain::MAX_TX_PER_BLOCK);
    assert!(!chain.validate_block(&b));
}
