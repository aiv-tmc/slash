//! `test_coinbase_amount_matches_schedule` — one test case of the
//! `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*`; only this test's body lives here.

use super::*;

#[test]
fn test_coinbase_amount_matches_schedule() {
    // Every era's first block pays the exact era subsidy, and the first
    // era-9 block omits the coinbase entirely.
    for era in 0..9u64 {
        let h = era * slash::chain::HALVING_INTERVAL;
        let chain = chain_with_next_height(h);
        let miner = [7u8; 32];
        let block = prepared_tail_block(&chain, miner, true);
        let paid: u64 = block.txs[0].outputs.iter().map(|o| o.end - o.start).sum();
        assert_eq!(paid, ERA_SUBSIDIES[era as usize]);
    }
    let chain = chain_with_next_height(9 * slash::chain::HALVING_INTERVAL);
    let block = prepared_tail_block(&chain, [7u8; 32], false);
    assert!(block.txs.is_empty());
}
