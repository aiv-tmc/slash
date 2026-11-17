//! `test_difficulty_clamp` — one test case of the `integration_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_difficulty_clamp() {
    // The retarget formula reads only timestamps and difficulties, so the
    // two clamp directions are exercised with synthetic headers on a mock
    // clock: a full interval mined ten times faster than the target
    // spacing demands ten times the difficulty but must cap at four, and a
    // full interval mined eight times slower produces a fractional
    // difficulty that must floor at one. The rescaled difficulty applies
    // from the first block of the new interval onward, so the queries
    // target interval boundaries rather than mid-window heights (mid-window
    // heights inherit whatever difficulty the previous header declares,
    // which the synthetic headers pin to one). Grinding two real windows
    // of proofs would only re-test what the header arithmetic already pins.
    let _tmp = setup_test_dir("difficulty_clamp");
    reset_testnet();
    let mut chain = chain::Chain::genesis();

    // Tenfold-fast window: the raw retarget is 10x, the upward clamp caps
    // it at 4x, and the first block of the new interval carries 4.
    let fast_step = chain::TARGET_BLOCK_TIME_SECS / 10;
    push_synthetic_headers(&mut chain, chain::DIFFICULTY_ADJUSTMENT_INTERVAL, fast_step);
    assert_eq!(
        chain.difficulty_for_height(chain::DIFFICULTY_ADJUSTMENT_INTERVAL + 1),
        4
    );

    // Eightfold-slow window: the raw retarget falls below one, and the
    // difficulty floor raises the first block of the next interval back
    // to the minimum of one.
    let slow_step = chain::TARGET_BLOCK_TIME_SECS * 8;
    push_synthetic_headers(&mut chain, chain::DIFFICULTY_ADJUSTMENT_INTERVAL, slow_step);
    assert_eq!(
        chain.difficulty_for_height(2 * chain::DIFFICULTY_ADJUSTMENT_INTERVAL + 1),
        1
    );
}
