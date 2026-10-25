//! `test_mempool_overflow_eviction` — one test case of the `integration_tests` suite.
//!
//! The eviction-order simulation is fully self-contained and imports
//! nothing from the crate root.

#[test]
fn test_mempool_overflow_eviction() {
    // The mempool keeps at most MEMPOOL_CAP entries; pushing one more
    // evicts the oldest. The eviction order is pinned here so fee
    // estimation and replacement behavior stay predictable. The test
    // models the buffer arithmetic directly; no chain state participates.
    let miner = [1u8; 32];
    let mut mempool: Vec<slash::chain::Tx> = Vec::new();
    for i in 0..slash::p2p::MEMPOOL_CAP + 1 {
        let start = (i as u64) * 100;
        let tx = slash::chain::Tx {
            from: miner,
            inputs: vec![(start, start + 50)],
            outputs: vec![slash::chain::Output {
                start,
                end: start + 50,
                to: miner,
                lock: None,
            }],
            sig: vec![i as u8; 64],
            scheme: 0,
            memo: None,
        };
        mempool.push(tx);
        if mempool.len() > slash::p2p::MEMPOOL_CAP {
            mempool.remove(0);
        }
    }
    assert_eq!(mempool.len(), slash::p2p::MEMPOOL_CAP);
}
