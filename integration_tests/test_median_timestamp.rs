//! `test_median_timestamp` — one test case of the `integration_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_median_timestamp() {
    // The median-time rule takes the element at index len/2 of the sorted
    // window of the most recent blocks: for an even-sized window that is
    // the upper of the two middle timestamps, matching the reference
    // convention. The implicit genesis block never enters the block
    // vector, so the window here holds exactly the five applied blocks.
    // Block times are chosen before the nonce search so the proof binds
    // the stamped time.
    let _tmp = setup_test_dir("median_time");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];

    for h in 1..=5 {
        let b = mine_valid_block(&chain, h * 100, miner, chain.next_difficulty());
        let b = chain.prepare_block(b);
        chain.apply(b).unwrap();
    }

    // Sorted window [100, 200, 300, 400, 500]: index 5 / 2 = 2 -> 300.
    assert_eq!(chain.median_timestamp(), 300);
}
