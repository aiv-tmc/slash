//! Fork and orphan tests: side-fork admission, orphan parking, and the
//! heavier-fork reorg. Fork blocks go through `process_block`, which
//! validates them against the fork base state and adopts the fork when
//! its work exceeds the main chain.

use super::*;
use slash::test_fixtures::grind_nonce;

#[test]
fn test_duplicate_block_rejected() {
    let _tmp = setup_test_dir("duplicate");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [4u8; 32];
    let b1 = mine_next_block(&chain, miner, vec![]);
    chain.apply(b1.clone()).unwrap();
    // Re-applying an already-applied block is a structural duplicate and
    // must be refused.
    assert!(chain.apply(b1).is_err());
}

#[test]
fn test_orphan_waits_for_parent() {
    let _tmp = setup_test_dir("orphan");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [3u8; 32];
    let b1 = mine_next_block(&chain, miner, vec![]);
    let h1 = b1.height;
    chain.apply(b1).unwrap();
    // Construct a child at h1 + 2 with no known parent at h1 + 1. The
    // unchecked builder is required here: the height deliberately skips
    // a slot, so the contiguity assertion of the checked builder would
    // reject the template before the test ever reaches the chain.
    let mut b3 = mine_block_at_unchecked(&chain, h1 + 2, h1 + 2, miner, 1, vec![]);
    b3.prev = [0xAA; 32];
    // Re-grind for the rewritten parent (the proof input commits to it).
    grind_nonce(&chain, &mut b3);
    let b3 = chain.prepare_block(b3);
    // The parent is unknown, so application fails and the block parks in
    // the orphan pool instead.
    assert!(chain.apply(b3).is_err());
    // A block at h1 + 1 with a known parent applies and links.
    let b2 = mine_block_at(&chain, h1 + 1, h1 + 1, miner, 1, vec![]);
    chain.apply(b2).unwrap();
}

#[test]
fn test_checkpoint_rejects_divergence() {
    let _tmp = setup_test_dir("checkpoint");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [6u8; 32];
    let b1 = mine_next_block(&chain, miner, vec![]);
    chain.apply(b1.clone()).unwrap();
    let h = chain.tip_height();
    let hash = chain.block_hash(&b1);
    chain.checkpoints.insert(h, hash);
    let b2 = mine_next_block(&chain, miner, vec![]);
    assert!(chain.apply(b2).is_ok());
    // A fork at the checkpointed height with a different hash must be
    // rejected. The unchecked builder is required here: the template
    // targets a height behind the tip, which the checked builder's
    // contiguity assertion would refuse before the checkpoint logic is
    // ever exercised.
    let mut alt = mine_block_at_unchecked(&chain, h, h, miner, 1, vec![]);
    alt.prev = [0xBB; 32];
    grind_nonce(&chain, &mut alt);
    let alt = chain.prepare_block(alt);
    assert!(chain.apply(alt).is_err());
}

#[test]
fn test_reorg_to_heavier_fork() {
    // A side fork with more work replaces the main suffix: the
    // disconnected blocks leave the main chain, the fork becomes
    // canonical, and both miners' balances reflect the final chain —
    // coinbase plus the perpetual one-cell slot mint per included block.
    // Every fork block is built against the fork base state (coinbase
    // selection, nonce ownership, and page roots), because side-fork
    // validation rewinds to the fork point and re-executes there.
    let _tmp = setup_test_dir("reorg_heavier");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner_a = [1u8; 32];
    let miner_b = [2u8; 32];
    for _ in 0..3 {
        let b = mine_next_block(&chain, miner_a, vec![]);
        chain.apply(b).unwrap();
    }
    // Scratch state at the fork point (after block 1) and its clock.
    let mut scratch = chain.state_history[&1].clone();
    let fork_base_time = chain.blocks[0].time;
    let mut prev = chain.block_hash(&chain.blocks[0]);
    for h in 2..=4u64 {
        let mut b = mine_valid_block(&chain, fork_base_time + h - 1, miner_b, 1);
        b.height = h;
        b.prev = prev;
        // Coinbase selected against the fork base state, not the tip.
        let coinbase = chain::Chain::build_coinbase(&scratch, miner_b, h, 0).unwrap();
        b.txs = vec![coinbase];
        // Install the fork base state so the nonce ownership check and
        // preparation execute against exactly the state the fork
        // validation replays, then restore the tip state.
        let saved = std::mem::replace(&mut chain.state, scratch.clone());
        grind_nonce(&chain, &mut b);
        b = chain.prepare_block(b);
        chain.state = saved;
        // Advance the scratch state through the block for the next sibling
        // before the block is handed to the pipeline (process_block takes
        // ownership, and the scratch advance still needs to borrow it).
        scratch = chain
            .execute_block_state(&scratch, &b)
            .expect("fork block executes against the fork base");
        prev = chain.block_hash(&b);
        chain.process_block(b).expect("fork block processes");
    }
    // The fork (four blocks total) outweighs the main chain (three).
    assert_eq!(chain.tip_height(), 4);
    assert_eq!(chain.state.balance(miner_a), 500 + 1);
    assert_eq!(chain.state.balance(miner_b), 3 * (500 + 1));
}
