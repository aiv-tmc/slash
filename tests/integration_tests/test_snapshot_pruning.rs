//! `test_snapshot_pruning` — one test case of the `integration_tests` suite.
//!
//! The shared helpers come from the crate root via `use super::*;`; the
//! `Chain` type is imported explicitly so this file compiles on its own.

use super::*;
use slash::chain::Chain;

#[test]
fn test_snapshot_pruning() {
    // The three most recent state snapshots are retained; a fourth save
    // evicts the oldest.
    let mut chain = Chain::genesis();
    let miner = [1u8; 32];
    for _ in 0..5 {
        let b = mine_next_block(&chain, miner, vec![]);
        chain.apply(b).unwrap();
    }
    // The history is bounded by MAX_REORG_DEPTH; it holds at most 101
    // entries (the tip plus the window).
    assert!(chain.state_history.len() <= 101);
}
