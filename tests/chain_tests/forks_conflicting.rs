//! Conflicting reorg tests: a fork whose transactions spend cells that
//! the abandoned main suffix also spent is validated against the fork
//! point state, so the conflict resolves by branch choice rather than by
//! a validation failure that would wedge the node; a fork that fails
//! validation is never adopted and leaves the main chain identical.
//!
//! Balance dumps at the marked points make any divergence name the stage
//! that introduced it. The fixtures live in `forks_conflicting_support`.

use super::forks_conflicting_support::*;
use super::*;

#[test]
fn test_reorg_accepts_fork_with_conflicting_transactions() {
    // The fork spends the same cells the abandoned main suffix spent to
    // a different recipient. Fork validation replays from the fork point
    // state, where the cells are still unspent on this branch, so the
    // branch validates and adoption resolves the conflict by work.
    let _tmp = setup_test_dir("reorg_conflicting");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    // miner_a deliberately avoids the well-known addresses: [1u8; 32] is
    // the fee vault, and the conflicting transaction's mandatory fee pays
    // the vault — aliasing the two would credit the fee to miner_a and
    // mask exactly the rollback this test pins.
    let miner_a = [0x1A; 32];
    let miner_b = [2u8; 32];
    let bob = [0xB0; 32];
    let carol = [0xC0; 32];
    fund_alice(&mut chain.state);
    let chain_id = chain.chain_id.clone();

    // Main chain: block 1 without user activity, then blocks 2 and 3
    // spending Alice's cells to Carol.
    let b1 = mine_next_block(&chain, miner_a, vec![]);
    chain.apply(b1).unwrap();
    dump_balances(&chain, STAGE_MAIN, miner_a, miner_b, bob, carol);
    let to_carol = alice_spend(&spend_outputs(carol), chain_id.as_bytes());
    let b2 = mine_next_block(&chain, miner_a, vec![to_carol]);
    chain.apply(b2).unwrap();
    let b3 = mine_next_block(&chain, miner_a, vec![]);
    chain.apply(b3).unwrap();
    assert_eq!(
        chain.tip_height(),
        3,
        "{STAGE_MAIN} unexpected tip after main chain"
    );
    assert_eq!(
        chain.state.balance(carol),
        180,
        "{STAGE_MAIN} carol must hold the main-branch payment"
    );
    assert!(
        chain.state_history.contains_key(&1),
        "{STAGE_MAIN} fork point state missing from history"
    );
    dump_balances(
        &chain,
        "[forks_conflicting:pre-fork]",
        miner_a,
        miner_b,
        bob,
        carol,
    );

    // Fork from height 1: three blocks paying miner_b, the first of
    // which spends Alice's cells to Bob instead.
    let mut scratch = chain.state_history[&1].clone();
    let fork_base_time = chain.blocks[0].time;
    let mut prev = chain.block_hash(&chain.blocks[0]);
    let to_bob = alice_spend(&spend_outputs(bob), chain_id.as_bytes());
    let fork_txs = vec![vec![to_bob], vec![], vec![]];
    for (i, txs) in fork_txs.into_iter().enumerate() {
        let height = 2 + i as u64;
        let b = build_fork_block(
            &mut chain,
            &mut scratch,
            height,
            fork_base_time + height - 1,
            prev,
            miner_b,
            txs,
            true,
            STAGE_BUILD,
        );
        prev = chain.block_hash(&b);
        chain.process_block(b).unwrap_or_else(|e| {
            panic!("{STAGE_ADOPT} fork block at height {height} rejected: {e:?}")
        });
        dump_balances(
            &chain,
            &format!("{STAGE_ADOPT} after h{height}"),
            miner_a,
            miner_b,
            bob,
            carol,
        );
    }
    // Three fork blocks outweigh the two-block main suffix: the fork is
    // canonical and its view of Alice's cells is the surviving one.
    assert_eq!(
        chain.tip_height(),
        4,
        "{STAGE_ASSERT} fork must become the tip"
    );
    assert_eq!(
        chain.state.balance(miner_b),
        3 * (500 + 1),
        "{STAGE_ASSERT} miner_b must hold the three fork rewards"
    );
    assert_eq!(
        chain.state.balance(miner_a),
        500 + 1,
        "{STAGE_ASSERT} miner_a must keep only the pre-fork reward"
    );
    assert_eq!(
        chain.state.balance(bob),
        180,
        "{STAGE_ASSERT} bob must hold the fork payment"
    );
    assert_eq!(
        chain.state.balance(carol),
        0,
        "{STAGE_ASSERT} carol's payment must be disconnected"
    );
    assert_eq!(
        chain.state.balance(alice_pk()),
        0,
        "{STAGE_ASSERT} alice's cells are fully spent on the fork"
    );
}

#[test]
fn test_failed_fork_adoption_leaves_main_chain_identical() {
    // A fork whose later block spends cells the fork itself already
    // spent fails validation and is never adopted; the main chain is
    // exactly what it was before the attempt.
    let _tmp = setup_test_dir("reorg_failed_intact");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner_a = [1u8; 32];
    let miner_b = [2u8; 32];
    let bob = [0xB0; 32];
    fund_alice(&mut chain.state);
    let chain_id = chain.chain_id.clone();

    let b1 = mine_next_block(&chain, miner_a, vec![]);
    chain.apply(b1).unwrap();
    let to_bob = alice_spend(&spend_outputs(bob), chain_id.as_bytes());
    let b2 = mine_next_block(&chain, miner_a, vec![to_bob.clone()]);
    chain.apply(b2).unwrap();
    let b3 = mine_next_block(&chain, miner_a, vec![]);
    chain.apply(b3).unwrap();

    let tip_hash_before = chain.tip_hash();
    let blocks_before = blocks_fingerprint(&chain.blocks);
    let a_before = chain.state.balance(miner_a);
    let bob_before = chain.state.balance(bob);

    let mut scratch = chain.state_history[&1].clone();
    let fork_base_time = chain.blocks[0].time;
    let mut prev = chain.block_hash(&chain.blocks[0]);
    let f2 = build_fork_block(
        &mut chain,
        &mut scratch,
        2,
        fork_base_time + 1,
        prev,
        miner_b,
        vec![to_bob.clone()],
        true,
        "[forks_conflicting:failed-build-f2]",
    );
    prev = chain.block_hash(&f2);
    let f3 = build_fork_block(
        &mut chain,
        &mut scratch,
        3,
        fork_base_time + 2,
        prev,
        miner_b,
        vec![to_bob],
        false,
        "[forks_conflicting:failed-build-f3]",
    );
    let r2 = chain.process_block(f2);
    eprintln!("[forks_conflicting:failed-adopt] f2 -> {r2:?}");
    let r3 = chain.process_block(f3);
    eprintln!("[forks_conflicting:failed-adopt] f3 -> {r3:?}");

    assert_eq!(
        chain.tip_height(),
        3,
        "[forks_conflicting:failed-assert] tip moved"
    );
    assert_eq!(
        chain.tip_hash(),
        tip_hash_before,
        "[forks_conflicting:failed-assert] tip hash moved"
    );
    assert_eq!(
        blocks_fingerprint(&chain.blocks),
        blocks_before,
        "[forks_conflicting:failed-assert] block list moved"
    );
    assert_eq!(
        chain.state.balance(miner_a),
        a_before,
        "[forks_conflicting:failed-assert] miner_a balance moved"
    );
    assert_eq!(
        chain.state.balance(miner_b),
        0,
        "[forks_conflicting:failed-assert] miner_b must hold nothing"
    );
    assert_eq!(
        chain.state.balance(bob),
        bob_before,
        "[forks_conflicting:failed-assert] bob balance moved"
    );
}
