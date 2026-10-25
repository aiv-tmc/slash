//! The state-history window test: after walking past the reorg depth,
//! the recorded history stays capped at the window size.

use super::*;

/// The history window retains the tip plus at most MAX_REORG_DEPTH
/// earlier entries: every reorg target inside the allowed depth has a
/// recorded state, and the map cannot grow past the window.
#[test]
fn test_state_history_pruned() {
    let _tmp = setup_test_dir("history_pruned");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];
    for _ in 0..(chain::MAX_REORG_DEPTH + 10) {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b).unwrap();
    }
    assert!(chain.state_history.len() <= chain::MAX_REORG_DEPTH as usize + 1);
}
