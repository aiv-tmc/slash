//! `test_mempool_cap_eviction_simulated` — one test case of the `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_mempool_cap_eviction_simulated() {
    let _tmp = setup_test_dir("mempool_cap");
    reset_testnet();
    // The chain is only read by the block builder below.
    let chain = chain::Chain::genesis();

    // One transaction more than the mempool capacity is also more than the
    // per-block transaction cap, so the block must be invalid either way.
    let mut txs = Vec::new();
    for i in 0..=p2p::MEMPOOL_CAP {
        let idx = i as u64;
        let start = idx * 50;
        txs.push(chain::Tx {
            from: state::TREASURY,
            inputs: vec![(start, start + 50)],
            outputs: vec![chain::Output {
                start,
                end: start + 50,
                to: [1u8; 32],
                lock: None,
            }],
            sig: vec![],
            scheme: 0,
            memo: None,
        });
    }

    let b = mine_next_block(&chain, [1u8; 32], txs);
    assert!(!chain.validate_block(&b));
    assert!(b.txs.len() > chain::MAX_TX_PER_BLOCK);
}
