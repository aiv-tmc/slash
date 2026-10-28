//! `test_reorg_succeeds_with_history` — one test case of the
//! `reorg_persistence_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_reorg_succeeds_with_history() {
    // Control test: the identical fork wins when the exact common-ancestor
    // state is available, which is the normal in-session posture. The
    // chain holds heights 1..=5; the fork replaces the height-5 tip.
    let _tmp = setup_test_dir("reorg_success");
    reset_testnet();
    let (mut chain, _main_miner) = chain_with_blocks(5);
    let state4 = chain.state_history[&4].clone();
    let fork_miner = [9u8; 32];
    let hash4 = chain.block_hash(&chain.blocks[3]);
    let a = rooted(
        &chain,
        &state4,
        fork_block(&state4, hash4, 5, fork_miner, 450),
    );
    let result = chain.process_block(a.clone()).unwrap();
    assert_eq!(result, BlockProcessResult::SideFork);

    let mut pre_b = state4.clone();
    assert!(pre_b.mint_slot(a.mined_cell, fork_miner, 5));
    assert!(pre_b.mint(&a.txs[0].inputs, &state_outputs(&a.txs[0].outputs), 5));
    let a_hash = chain.block_hash(&a);
    let b = rooted(
        &chain,
        &pre_b,
        fork_block(&pre_b, a_hash, 6, fork_miner, 550),
    );

    match chain.process_block(b.clone()).unwrap() {
        BlockProcessResult::Reorged {
            disconnected,
            connected,
        } => {
            assert_eq!(disconnected, vec![5]);
            assert_eq!(connected, vec![5, 6]);
        }
        other => panic!("expected reorg, got {:?}", other),
    }
    assert_eq!(chain.blocks.len(), 6);
    assert_eq!(chain.tip_hash(), chain.block_hash(&b));
}
