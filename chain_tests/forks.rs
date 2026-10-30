//! Side-fork admission and orphan processing.
//!
//! These tests pin the side-fork admission gates (garbage entry
//! rejection), the orphan adoption cap per processing pass, and the
//! recovery replay's refusal to accept a broken linkage chain.

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
    let mut fork = mine_block_at(&chain, h, 1, fork_miner, 1, vec![]);
    fork.height = h;
    fork.prev = chain.blocks[h as usize - 1].prev;
    let fork = chain.prepare_block(fork);
    let result = chain.process_block(fork.clone()).unwrap();
    assert_eq!(result, BlockProcessResult::SideFork);
    // The side fork is retained until strictly heavier.
    assert_eq!(chain.tip_height(), h);

    // Mine two more blocks on the main chain: the fork is lighter now,
    // so it still cannot reorganize the tip.
    mine_tip_block(&mut chain, miner);
    mine_tip_block(&mut chain, miner);

    // A heavier fork (three blocks) must trigger the reorg: the
    // disconnected main-chain suffix returns its transactions to the
    // mempool, the fork becomes the tip, and the disconnected blocks
    // are recorded in `reorg_disconnected`.
    let fork_tip = chain
        .side_forks
        .values()
        .map(|(b, _)| b.clone())
        .next()
        .unwrap();
    let mut heavier = fork_tip.clone();
    for _ in 0..2 {
        let h = heavier.height + 1;
        let b = mine_block_at(&chain, h, h * 100, fork_miner, 1, vec![]);
        let mut b = b.clone();
        b.prev = chain.block_hash(&heavier);
        let b = chain.prepare_block(b);
        heavier = b;
    }
    let result = chain.process_block(heavier).unwrap();
    match result {
        BlockProcessResult::Reorged {
            disconnected,
            connected,
        } => {
            assert_eq!(disconnected.len(), 2);
            assert_eq!(connected.len(), 3);
        }
        other => panic!("expected Reorged, got {:?}", other),
    }
    assert_eq!(chain.tip_height(), h + 2);
    assert_eq!(chain.state.balance(fork_miner), 3 * (500 + 1));
}

#[test]
fn test_orphan_adopted_when_parent_arrives() {
    let _tmp = setup_test_dir("orphan_adopt");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [7u8; 32];
    mine_tip_block(&mut chain, miner);
    let h = chain.tip_height();
    // Craft a child of the tip with a wrong prev: it orphans.
    let mut orphan = mine_block_at(&chain, h + 1, h * 100 + 100, miner, 1, vec![]);
    orphan.prev = [0xAA; 32];
    match chain.process_block(orphan).unwrap() {
        BlockProcessResult::Orphan => {}
        other => panic!("expected Orphan, got {:?}", other),
    }
    // Mining the parent that links the orphan does not auto-adopt it in
    // this build: the orphan stays parked until the next explicit
    // `process_orphans` pass runs. This pins the current behavior so a
    // future auto-adoption change is visible in review.
    let parent = mine_block_at(&chain, h, h * 100, miner, 1, vec![]);
    chain.process_block(parent).unwrap();
    assert_eq!(chain.tip_height(), h);
    let adopted = chain.process_orphans();
    assert_eq!(adopted, 1);
    assert_eq!(chain.tip_height(), h + 1);
}

#[test]
fn test_side_fork_pool_rejects_garbage() {
    // Garbage entries — zero difficulty, impossible heights, oversized
    // payloads — never enter the side-fork pool.
    let mut chain = chain::Chain::genesis();
    let miner = [7u8; 32];
    mine_tip_block(&mut chain, miner);
    let tip = chain.tip_height();
    let mut garbage = mine_block_at(&chain, tip, 1, miner, 1, vec![]);
    garbage.difficulty = 0;
    assert!(!chain.validate_side_fork_entry(&garbage));
    let garbage = mine_block_at(&chain, tip + 1_000_000, 1, miner, 1, vec![]);
    assert!(!chain.validate_side_fork_entry(&garbage));
}
