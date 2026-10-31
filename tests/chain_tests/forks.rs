//! Side-fork admission and orphan processing.
//!
//! These tests pin the side-fork admission gates (garbage entry
//! rejection), orphan adoption after the parent becomes known, and the
//! reorg behavior of a strictly heavier competing branch.

use super::*;
use slash::chain::BlockProcessResult;

#[test]
fn test_side_fork_admitted_and_reorged() {
    let _tmp = setup_test_dir("side_fork");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [7u8; 32];
    mine_tip_block(&mut chain, miner);
    mine_tip_block(&mut chain, miner);
    let fork_miner = [9u8; 32];
    let h = chain.tip_height();

    // A side block at the current height competes with the main-chain
    // block at the same height. Its parent is the previous main-chain
    // block: for block height h stored at index h-1, that parent hash is
    // the stored block's `prev` field.
    let fork_parent_hash = chain.blocks[(h - 1) as usize].prev;
    let mut fork_scratch = (*chain.state_history[&(h - 1)]).clone();
    let fork = build_fork_block(
        &mut chain,
        &mut fork_scratch,
        h,
        h * 100,
        fork_parent_hash,
        fork_miner,
        vec![],
        true,
        "[forks:initial-side-fork]",
    );
    let result = chain.process_block(fork.clone()).unwrap();
    assert_eq!(result, BlockProcessResult::SideFork);
    // The side fork is retained until strictly heavier.
    assert_eq!(chain.tip_height(), h);

    // Mine two more blocks on the main chain: the fork is lighter now,
    // so it still cannot reorganize the tip.
    mine_tip_block(&mut chain, miner);
    mine_tip_block(&mut chain, miner);

    // Three extensions make the fork strictly heavier than the main
    // chain. The first two extensions remain parked; the third forces a
    // reorg, disconnects main-chain blocks h..=h+2, and connects the
    // four-block fork h..=h+3.
    let mut extension_scratch = fork_scratch.clone();
    let mut extension_tip = fork.clone();
    for height in h + 1..=h + 3 {
        let extension_parent_hash = chain.block_hash(&extension_tip);
        let block = build_fork_block(
            &mut chain,
            &mut extension_scratch,
            height,
            height * 100,
            extension_parent_hash,
            fork_miner,
            vec![],
            true,
            "[forks:heavier-extension]",
        );
        extension_tip = block.clone();
        match chain.process_block(block).unwrap() {
            BlockProcessResult::SideFork if height < h + 3 => {}
            BlockProcessResult::Reorged {
                disconnected,
                connected,
            } if height == h + 3 => {
                assert_eq!(disconnected.len(), 3);
                assert_eq!(connected.len(), 4);
            }
            other => panic!("unexpected block result: {:?}", other),
        }
    }
    assert_eq!(chain.tip_height(), h + 3);
    assert_eq!(chain.state.balance(fork_miner), 4 * (500 + 1));
}

#[test]
fn test_orphan_adopted_when_parent_arrives() {
    let _tmp = setup_test_dir("orphan_adopt");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [7u8; 32];
    mine_tip_block(&mut chain, miner);
    let h = chain.tip_height();

    // Build the parent first so the child can reference its exact hash,
    // but do not admit the parent yet: the child must enter the orphan
    // pool while its parent is still unknown to the chain.
    let parent_parent_hash = chain.blocks[(h - 1) as usize].prev;
    let mut parent_scratch = (*chain.state_history[&(h - 1)]).clone();
    let parent = build_fork_block(
        &mut chain,
        &mut parent_scratch,
        h,
        h * 100,
        parent_parent_hash,
        miner,
        vec![],
        true,
        "[forks:orphan-parent]",
    );
    let parent_hash = chain.block_hash(&parent);
    let mut orphan_scratch = parent_scratch.clone();
    let orphan = build_fork_block(
        &mut chain,
        &mut orphan_scratch,
        h + 1,
        (h + 1) * 100,
        parent_hash,
        miner,
        vec![],
        true,
        "[forks:orphan-child]",
    );
    match chain.process_block(orphan).unwrap() {
        BlockProcessResult::Orphan => {}
        other => panic!("expected Orphan, got {:?}", other),
    }

    // Admitting the parent parks it as a side fork; the explicit orphan
    // pass then follows the newly known parent and adopts the child.
    assert_eq!(
        chain.process_block(parent).unwrap(),
        BlockProcessResult::SideFork
    );
    assert_eq!(chain.tip_height(), h);
    let adopted = chain.process_orphans();
    assert_eq!(adopted, 1);
    assert_eq!(chain.tip_height(), h + 1);
}

#[test]
fn test_side_fork_pool_rejects_garbage() {
    // Garbage entries — zero difficulty and impossible heights — never
    // enter the side-fork pool.
    let _tmp = setup_test_dir("side_fork_garbage");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [7u8; 32];
    mine_tip_block(&mut chain, miner);
    let tip = chain.tip_height();

    // Zero difficulty can never satisfy proof of work.
    let mut garbage = mine_block_at(&chain, tip + 1, 1, miner, 1, vec![]);
    garbage.difficulty = 0;
    assert!(!chain.validate_side_fork_entry(&garbage));

    // An impossible height can never extend the retained chain.
    let mut garbage = mine_block_at(&chain, tip + 1, 1, miner, 1, vec![]);
    garbage.height = tip + 1_000_000;
    assert!(!chain.validate_side_fork_entry(&garbage));
}
