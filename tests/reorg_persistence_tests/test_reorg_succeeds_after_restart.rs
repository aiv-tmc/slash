//! `test_reorg_succeeds_after_restart` — one test case of the
//! `reorg_persistence_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_reorg_succeeds_after_restart() {
    // The reorganization window rebuilt after a restart must be exact: a
    // fork that would win in-session must also win on a reloaded node.
    let _tmp = setup_test_dir("reorg_after_restart");
    reset_testnet();
    let (chain, _miner) = chain_with_blocks(5);
    slash::save(&chain);
    let mut loaded = slash::load_pruned();
    assert_eq!(loaded.state_history.len(), 6);

    let state4 = loaded.state_history[&4].clone();
    let fork_miner = [9u8; 32];
    let hash4 = loaded.block_hash(&loaded.blocks[3]);
    let a = rooted(
        &loaded,
        &state4,
        fork_block(&state4, hash4, 5, fork_miner, 450),
    );
    let result = loaded.process_block(a.clone()).unwrap();
    assert_eq!(result, BlockProcessResult::SideFork);

    let mut pre_b = state4.clone();
    assert!(pre_b.mint_slot(a.mined_cell, fork_miner, 5));
    assert!(pre_b.mint(&a.txs[0].inputs, &state_outputs(&a.txs[0].outputs), 5));
    let a_hash = loaded.block_hash(&a);
    let b = rooted(
        &loaded,
        &pre_b,
        fork_block(&pre_b, a_hash, 6, fork_miner, 550),
    );

    match loaded.process_block(b).unwrap() {
        BlockProcessResult::Reorged { .. } => {}
        other => panic!("expected reorg after restart, got {:?}", other),
    }
    assert_eq!(loaded.blocks.len(), 6);
}
