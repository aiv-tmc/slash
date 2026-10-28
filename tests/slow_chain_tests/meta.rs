//! The retarget test: the full interval is walked with synthetic headers
//! on a fast clock — the retarget formula reads only timestamps and
//! difficulties — and exactly one real RandomX proof is mined, at the
//! retargeted difficulty. That single real proof is the coverage the
//! mock-clock harness cannot give: the mining template takes its target
//! from `next_difficulty()`, so a proof that applies at difficulty four
//! proves the live mining loop and the live validation both drive the
//! retarget.

use super::*;

/// Across a full retarget interval mined faster than the target spacing,
/// the retarget fires: the measured window time falls under the target
/// interval time, so the difficulty rises from the floor of one to the
/// four-fold clamp. The interval headers are synthetic; the block at the
/// boundary is real.
#[test]
fn test_difficulty_retargets_every_interval() {
    let _tmp = setup_test_dir("retarget");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];
    let mut time = 1_700_000_000u64;
    for i in 0..chain::DIFFICULTY_ADJUSTMENT_INTERVAL {
        // Far faster than the 300-second target spacing, so the window
        // measures short and the rescale lands on the upper clamp.
        time += 2;
        let h = i + 1;
        chain.blocks.push(chain::Block {
            version: 1,
            prev: [0u8; 32],
            time,
            height: h,
            nonce: 0,
            miner,
            mined_cell: 0,
            txs: vec![],
            fee_claims: vec![],
            difficulty: 1,
            page_roots: std::collections::BTreeMap::new(),
        });
    }
    assert_eq!(chain.next_difficulty(), 4);
    // The template grinds against the chain's next difficulty: the block
    // carrying difficulty four, and applying it, proves the live loop
    // drives the retarget end to end.
    let real = mine_next_block(&chain, miner, vec![]);
    assert_eq!(real.difficulty, 4);
    chain.apply(real).unwrap();
    assert_eq!(chain.next_difficulty(), 4);
}
