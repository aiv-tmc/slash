//! `test_median_timestamp` — one test case of the `integration_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_median_timestamp() {
    // The median-time rule averages the two middle timestamps of the
    // eleven-block window for even window sizes.
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];

    for h in 1..=5 {
        let mut b = mine_valid_block(&chain, h, miner, chain.next_difficulty());
        b.time = h * 100;
        let b = chain.prepare_block(b);
        chain.apply(b).unwrap();
    }

    // The window covers genesis (time 0) and blocks at 100..=500:
    // sorted [0, 100, 200, 300, 400, 500] -> median (200 + 300) / 2 = 250.
    let median = chain.median_timestamp();
    assert_eq!(median, 250);
}
