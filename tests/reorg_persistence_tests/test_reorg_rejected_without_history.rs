//! `test_reorg_rejected_without_history` — one test case of the `reorg_persistence_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_reorg_rejected_without_history() {
    // Simulates the post-restart fail-closed posture: only the tip state is
    // available. The reorg attempt must be rejected with
    // CannotReorgBeyondHistory and the chain must remain completely intact.
    let _tmp = setup_test_dir("reorg_fail_closed");
    reset_testnet();
    let (mut chain, main_miner) = chain_with_blocks(5);
    let tip_hash = chain.tip_hash();
    let state4 = chain.state_history[&4].clone();

    // Adopt the fail-closed posture: drop all history except the tip state.
    chain.state_history.clear();
    chain.state_history.insert(5, chain.state.clone());

    // Fork of two blocks extending height 4 must overtake the one-block tail.
    let fork_miner = [9u8; 32];
    let hash4 = chain.block_hash(&chain.blocks[4]);
    let a = fork_block(&state4, hash4, 5, fork_miner, 450);
    let result = chain.process_block(a.clone()).unwrap();
    assert_eq!(result, BlockProcessResult::SideFork);

    // Pre-state for the second fork block: apply block A's transition to the
    // common-ancestor state so the coinbase selection matches consensus.
    let mut pre_b = state4.clone();
    assert!(pre_b.mint_slot(a.mined_cell, fork_miner, 5));
    assert!(pre_b.mint(&a.txs[0].inputs, &state_outputs(&a.txs[0].outputs), 5));
    let a_hash = chain.block_hash(&a);
    let b = fork_block(&pre_b, a_hash, 6, fork_miner, 550);

    let err = chain.process_block(b).unwrap_err();
    assert_eq!(err, BlockError::CannotReorgBeyondHistory);
    // The chain, state, and balances are untouched by the failed reorg.
    assert_eq!(chain.blocks.len(), 6);
    assert_eq!(chain.tip_hash(), tip_hash);
    assert_eq!(chain.state.balance(main_miner), 5);
}
