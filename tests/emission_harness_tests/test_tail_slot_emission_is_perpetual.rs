//! `test_tail_slot_emission_is_perpetual` — one test case of the
//! `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*`; only this test's body lives here.

use super::*;

#[test]
fn test_tail_slot_emission_is_perpetual() {
    // Pin the perpetual tail: from era 9 onward every block pays the miner
    // exactly one treasury cell through the PoW slot, forever, regardless
    // of how far past the emission schedule the chain advances. See
    // PERPETUAL_TAIL_EMISSION.md for the decision record.
    for &h in &[9 * slash::chain::HALVING_INTERVAL, 100_000_000u64] {
        let chain = chain_with_next_height(h);
        let miner = [7u8; 32];
        let b = prepared_tail_block(&chain, miner, false);
        assert!(b.txs.is_empty());
        let mut chain = chain;
        chain.apply(b).expect("tail block must apply");
        assert_eq!(chain.state.balance(miner), 1);
        // Conservation: exactly one cell left the treasury entry. The
        // ownership ranges are keyed by start cell; each range records its
        // end and owner, so the width is `end - key`.
        let mut treasury_sum = 0u128;
        for id in 0..slash::state::page_count() {
            let page = chain.state.get_page(id);
            for (&start, r) in page.ranges.iter() {
                if r.o == TREASURY {
                    treasury_sum += (r.e - start) as u128;
                }
            }
        }
        assert_eq!(treasury_sum, N as u128 - 1);
    }
}
