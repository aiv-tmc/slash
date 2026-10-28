//! `test_retarget_intervals_on_mock_clock` — one test case of the
//! `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_retarget_intervals_on_mock_clock() {
    // Exercise the difficulty retarget across four consecutive intervals on
    // a mock clock by pushing synthetic headers (no proof-of-work is
    // required because the retarget formula reads only timestamps and
    // difficulties). The retarget rule is relative: each interval the
    // difficulty multiplies by the measured ratio (target interval time
    // over actual window time), clamped to one quarter and four times the
    // previous interval's difficulty, never below one. The step spacings
    // are chosen so one interval lands on the lower clamp, one stays on
    // target, one lands on the upper clamp, and one overshoots it.
    reset_testnet();
    let mut chain = slash::chain::Chain::genesis();
    let miner = [3u8; 32];
    let t0 = 1_700_000_000u64;
    // Fill the genesis window at the target spacing. The adjustment
    // interval is a u64 constant, so the loop index is a plain u64 and
    // multiplies the spacing directly.
    for i in 0..slash::chain::DIFFICULTY_ADJUSTMENT_INTERVAL {
        chain.blocks.push(Block {
            version: 1,
            prev: [0u8; 32],
            time: t0 + i * slash::chain::TARGET_BLOCK_TIME_SECS,
            height: i + 1,
            nonce: 0,
            miner,
            mined_cell: 0,
            txs: vec![],
            fee_claims: vec![],
            difficulty: 1,
            page_roots: BTreeMap::new(),
        });
    }
    // (spacing, expected difficulty after the interval).
    let steps = [
        (600u64, 1u64), // 2x slow -> ratio 1/2 -> lower clamp: 1
        (300, 1),       // on target -> ratio 1 -> unchanged
        (75, 4),        // 4x fast -> ratio 4 -> upper clamp: 4
        (1_500, 1),     // 5x slow -> ratio 1/5 -> lower clamp: 1
    ];
    let mut expected = 1u64;
    for (spacing, expected_after) in steps {
        let start_time = chain.blocks.last().unwrap().time;
        for i in 0..slash::chain::DIFFICULTY_ADJUSTMENT_INTERVAL {
            let h = chain.blocks.len() as u64 + 1;
            chain.blocks.push(Block {
                version: 1,
                prev: [0u8; 32],
                time: start_time + (i + 1) * spacing,
                height: h,
                nonce: 0,
                miner,
                mined_cell: 0,
                txs: vec![],
                fee_claims: vec![],
                difficulty: expected,
                page_roots: BTreeMap::new(),
            });
        }
        expected = chain.next_difficulty();
        assert_eq!(
            expected, expected_after,
            "spacing {spacing}s must retarget to {expected_after}"
        );
    }
    assert_eq!(chain.next_difficulty(), 1);
}
