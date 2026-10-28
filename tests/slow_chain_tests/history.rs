//! The state-history window test: applying blocks records the bounded
//! history pair, and the retention bound itself is pinned directly
//! against `record_history` in the library unit tests — a long mining
//! walk would pay a RandomX context initialization per block for no
//! additional coverage of the bound.

use super::*;

/// Applying blocks records the state and deployment tables at every
/// height, and the recorded window stays within the reorg depth.
#[test]
fn test_state_history_pruned() {
    let _tmp = setup_test_dir("history_pruned");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];
    for _ in 0..12 {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b).unwrap();
    }
    // Every applied height recorded its pair, in lockstep.
    for h in 1..=12 {
        assert!(
            chain.state_history.contains_key(&h),
            "state history missing height {h}"
        );
        assert!(
            chain.deployments_history.contains_key(&h),
            "deployment history missing height {h}"
        );
    }
    assert!(chain.state_history.len() <= chain::MAX_REORG_DEPTH as usize + 1);
    assert!(chain.deployments_history.len() <= chain::MAX_REORG_DEPTH as usize + 1);
}
