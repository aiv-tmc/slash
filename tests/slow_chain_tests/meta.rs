//! The retarget test: mining past a full difficulty interval with
//! fast timestamps must leave the difficulty at the floor.

use super::*;

/// Across a full retarget interval mined faster than the target spacing,
/// the retargeted difficulty clamps at the floor of one, never zero.
#[test]
fn test_difficulty_retargets_every_interval() {
    let _tmp = setup_test_dir("retarget");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];
    for _ in 0..(chain::DIFFICULTY_ADJUSTMENT_INTERVAL + 2) {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b).unwrap();
    }
    assert_eq!(chain.next_difficulty(), 1);
}
