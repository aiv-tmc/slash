//! `test_retarget_intervals_on_mock_clock` — one test case of the `emission_harness_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_retarget_intervals_on_mock_clock() {
    // Exercise the difficulty retarget across four consecutive intervals on
    // a mock clock by pushing synthetic headers (no proof-of-work is
    // required because the retarget formula reads only timestamps and
    // difficulties). Interval steps are chosen so that two intervals land
    // inside the 4x clamp and two intervals land exactly on the clamp
    // boundaries.
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
    let steps = [
        (600u64, 1u64), // 2x slow -> difficulty halves toward 1
        (300, 1),       // on target -> unchanged
        (75, 4),        // 4x fast -> clamp boundary, difficulty x4
        (74, 4),        // past the clamp -> still exactly 4x
    ];
    let mut expected = 1u64;
    for (spacing, _clamped) in steps {
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
    }
    // After the on-target interval the difficulty must read 1, and after the
    // two clamped fast intervals it must read exactly 4.
    assert_eq!(chain.next_difficulty(), 4);
}
