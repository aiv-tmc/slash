//! `test_reorg_rejected_without_history` — one test case of the
//! `reorg_persistence_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_reorg_rejected_without_history() {
    // Simulates the post-restart fail-closed posture: only the tip state is
    // available. The fork attempt must be rejected with
    // CannotReorgBeyondHistory and the chain must remain completely intact.
    let _tmp = setup_test_dir("reorg_fail_closed");
    reset_testnet();
    let (mut chain, main_miner) = chain_with_blocks(5);
    let tip_hash = chain.tip_hash();
    let state4 = chain.state_history[&4].clone();

    // Adopt the fail-closed posture: drop all history except the tip state.
    chain.state_history.clear();
    chain.state_history.insert(5, chain.state.clone());

    // A fork extending height 4 cannot be validated as a side fork without
    // the height-4 state: the reorg machinery needs the common-ancestor
    // state to replay the fork, so the block is rejected outright — there
    // is no pooled side fork to strengthen later.
    let fork_miner = [9u8; 32];
    let hash4 = chain.block_hash(&chain.blocks[3]);
    let a = rooted(
        &chain,
        &state4,
        fork_block(&state4, hash4, 5, fork_miner, 450),
    );
    let err = chain.process_block(a).unwrap_err();
    assert_eq!(err, BlockError::CannotReorgBeyondHistory);

    // The chain, state, and balances are untouched by the failed reorg.
    assert_eq!(chain.blocks.len(), 5);
    assert_eq!(chain.tip_hash(), tip_hash);
    assert_eq!(chain.state.balance(main_miner), 5 * 501);
}
