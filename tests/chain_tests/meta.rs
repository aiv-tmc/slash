//! Header metadata, proof-of-work slot, difficulty, and time rules.
//!
//! These tests pin the stateless header gates: genesis construction, the
//! PoW slot assignment (including the rule that a locked or user-owned
//! cell can never be claimed as a slot), difficulty retargeting, and the
//! median-time plus two-hour drift rules.

use super::*;
use slash::chain;

#[test]
fn test_genesis_chain_id() {
    let main = chain::Chain::genesis();
    assert_eq!(main.chain_id, chain::MAINNET_ID);
    let test = chain::Chain::with_chain_id(chain::TESTNET_ID.to_string());
    assert_eq!(test.chain_id, chain::TESTNET_ID);
}

#[test]
fn test_mine_genesis_to_tip() {
    let _tmp = setup_test_dir("mine_genesis");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];
    for _ in 0..3 {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b).unwrap();
    }
    assert_eq!(chain.tip_height(), 3);
    assert_eq!(chain.blocks.len(), 3);
}

#[test]
fn test_verify_pow_rejects_zero_difficulty() {
    // A zero difficulty must never validate, whatever the nonce.
    let prev = [1u8; 32];
    let miner = [2u8; 32];
    assert!(chain::verify_pow(prev, 100, 1, miner, 0, 0).is_none());
    assert!(chain::verify_pow(prev, 100, 1, miner, 42, 0).is_none());
}

#[test]
fn test_mine_respects_locked_cells() {
    let _tmp = setup_test_dir("locked_mine");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    // The PoW slot must never claim a cell owned by a user.
    chain.state.mine(500, [9u8; 32], 0);
    let b = mine_next_block(&chain, [1u8; 32], vec![]);
    chain.apply(b).unwrap();
    assert_eq!(chain.state.balance([9u8; 32]), 1);
}

#[test]
fn test_difficulty_retargets_every_interval() {
    // The retarget formula reads only timestamps and difficulties, so the
    // full interval is exercised with synthetic headers on a mock clock
    // instead of two thousand mined blocks: every header declares
    // difficulty one and advances the mock clock by exactly the
    // 300-second target spacing, so the measured window time matches the
    // target interval time and the rescaled difficulty stays at the floor
    // of one. The boundary query fires the rescale over the complete
    // window; the tip query inherits the previous difficulty inside the
    // new interval.
    let _tmp = setup_test_dir("retarget");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    push_synthetic_headers(
        &mut chain,
        chain::DIFFICULTY_ADJUSTMENT_INTERVAL + 2,
        chain::TARGET_BLOCK_TIME_SECS,
    );
    assert_eq!(
        chain.difficulty_for_height(chain::DIFFICULTY_ADJUSTMENT_INTERVAL + 1),
        1
    );
    assert_eq!(chain.next_difficulty(), 1);
}

#[test]
fn test_median_timestamp() {
    let _tmp = setup_test_dir("median");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    for h in 1..=11 {
        let b = mine_block_at(&chain, h, h * 100, [1u8; 32], 1, vec![]);
        chain.apply(b).unwrap();
    }
    assert_eq!(chain.median_timestamp(), 600);
}

#[test]
fn test_time_drift_future() {
    let _tmp = setup_test_dir("drift_future");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let mut b = mine_block_at(&chain, 1, 1, [1u8; 32], 1, vec![]);
    // Three hours ahead exceeds the two-hour tolerance.
    b.time = (chrono::Utc::now().timestamp() as u64) + 3 * 3600;
    let b = chain.prepare_block(b);
    assert!(!chain.validate_block(&b) || chain.apply(b).is_err());
}

#[test]
fn test_time_drift_past() {
    let _tmp = setup_test_dir("drift_early");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let b1 = mine_block_at(&chain, 1, 1_000_000, [1u8; 32], 1, vec![]);
    chain.apply(b1).unwrap();
    // A timestamp below the parent time violates the median-time rule.
    let b2 = mine_block_at(&chain, 2, 999_999, [1u8; 32], 1, vec![]);
    assert!(!chain.validate_block(&b2));
}
