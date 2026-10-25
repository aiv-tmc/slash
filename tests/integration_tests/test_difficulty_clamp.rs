//! `test_difficulty_clamp` — one test case of the `integration_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_difficulty_clamp() {
    // Tenfold-fast blocks over a full retarget interval must raise the
    // difficulty by exactly the 4x clamp factor, not more.
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];

    for h in 1..=chain::DIFFICULTY_ADJUSTMENT_INTERVAL {
        let mut b = mine_valid_block(&chain, h, miner, chain.next_difficulty());
        b.time = h * chain::TARGET_BLOCK_TIME_SECS / 10;
        let b = chain.prepare_block(b);
        chain.apply(b).unwrap();
    }

    let next = chain.next_difficulty();
    let last = chain.blocks.last().unwrap().difficulty;
    assert!(next <= last.saturating_mul(4));
    assert!(next >= last.saturating_div(4).max(1));
}
