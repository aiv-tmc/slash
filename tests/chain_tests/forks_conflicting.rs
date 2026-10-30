//! Conflicting-reorg tests: two branches spend the same Alice-funded
//! cells to different recipients, the heavier branch reorganizes the
//! tip, and a subsequent heavier branch restores the original owner.
//! Every test builds both branches from the same retained snapshot so
//! the fork competition is deterministic.

use super::*;

#[test]
fn conflicting_reorg_resolves_single_owner() {
    let _tmp = setup_test_dir("conflict_owner");
    reset_testnet();
    let miner_a = test_keypair(3).1;
    let miner_b = test_keypair(4).1;
    let bob = test_keypair(5).1;
    let carol = test_keypair(6).1;

    let mut chain = Chain::genesis();
    let b1 = mine_tip_block(&mut chain, miner_a);
    fund_alice(chain.state_mut());
    let b1_hash = chain.block_hash(&b1);
    let chain_id = chain.chain_id.clone();
    let base_state = b1_state(&chain);
    let spend_bob = alice_spend(&spend_outputs(bob), chain_id.as_bytes());
    let spend_carol = alice_spend(&spend_outputs(carol), chain_id.as_bytes());

    // Branch A is the main chain and pays Bob.
    let mut main_scratch = base_state.clone();
    let a2 = build_fork_block(
        &mut chain,
        &mut main_scratch,
        2,
        200,
        b1_hash,
        miner_a,
        vec![spend_bob],
        true,
        STAGE_MAIN,
    );
    chain.apply(a2.clone()).expect("branch A applies");

    // Branch B spends the same input to Carol and then extends by one
    // empty block, making it strictly heavier than branch A.
    let mut fork_scratch = base_state.clone();
    let b2 = build_fork_block(
        &mut chain,
        &mut fork_scratch,
        2,
        200,
        b1_hash,
        miner_b,
        vec![spend_carol],
        true,
        STAGE_BUILD,
    );
    let b2_hash = chain.block_hash(&b2);
    let mut s3 = fork_scratch.clone();
    let b3 = build_fork_block(
        &mut chain,
        &mut s3,
        3,
        300,
        b2_hash,
        miner_b,
        vec![],
        true,
        STAGE_BUILD,
    );
    chain.process_block(b2.clone()).expect("side fork parks");
    let parked_fingerprint = blocks_fingerprint(std::slice::from_ref(
        &chain
            .side_forks
            .get(&b2_hash)
            .map(|(block, _)| block.clone())
            .expect("parked side fork must be retained"),
    ));
    assert_eq!(
        parked_fingerprint,
        blocks_fingerprint(std::slice::from_ref(&b2)),
        "the retained side fork must be byte-identical to the submitted block"
    );
    dump_balances(&chain, STAGE_ASSERT, miner_a, miner_b, bob, carol);
    let result = chain.process_block(b3).expect("fork extends");
    match result {
        BlockProcessResult::Reorged {
            disconnected,
            connected,
        } => {
            assert_eq!(disconnected, vec![2]);
            assert_eq!(connected, vec![2, 3]);
        }
        other => panic!("expected Reorged, got {:?}", other),
    }
    dump_balances(&chain, STAGE_ASSERT, miner_a, miner_b, bob, carol);
    assert_eq!(chain.state.balance(carol), 180);
    assert_eq!(chain.state.balance(bob), 0);
}

#[test]
fn conflicting_reorg_keeps_disconnected_transactions() {
    let _tmp = setup_test_dir("conflict_mempool");
    reset_testnet();
    let miner_a = test_keypair(3).1;
    let miner_b = test_keypair(4).1;
    let bob = test_keypair(5).1;
    let carol = test_keypair(6).1;

    let mut chain = Chain::genesis();
    let b1 = mine_tip_block(&mut chain, miner_a);
    fund_alice(chain.state_mut());
    let b1_hash = chain.block_hash(&b1);
    let chain_id = chain.chain_id.clone();
    let base_state = b1_state(&chain);
    let spend_bob = alice_spend(&spend_outputs(bob), chain_id.as_bytes());
    let spend_carol = alice_spend(&spend_outputs(carol), chain_id.as_bytes());

    let mut main_scratch = base_state.clone();
    let a2 = build_fork_block(
        &mut chain,
        &mut main_scratch,
        2,
        200,
        b1_hash,
        miner_a,
        vec![spend_bob],
        true,
        STAGE_MAIN,
    );
    chain.apply(a2.clone()).expect("branch A applies");

    // Branch B: block 2' pays Carol and is followed by an empty block 3'.
    // The history entries are shared snapshots; the working copy
    // detaches the value so the fork builder can advance it.
    let mut fork_scratch = base_state.clone();
    let b2 = build_fork_block(
        &mut chain,
        &mut fork_scratch,
        2,
        200,
        b1_hash,
        miner_b,
        vec![spend_carol],
        true,
        STAGE_BUILD,
    );
    let b2_hash = chain.block_hash(&b2);
    let mut s3 = fork_scratch.clone();
    let b3 = build_fork_block(
        &mut chain,
        &mut s3,
        3,
        300,
        b2_hash,
        miner_b,
        vec![],
        true,
        STAGE_BUILD,
    );
    chain.process_block(b2.clone()).expect("side fork parks");
    let parked_fingerprint = blocks_fingerprint(std::slice::from_ref(
        &chain
            .side_forks
            .get(&b2_hash)
            .map(|(block, _)| block.clone())
            .expect("parked side fork must be retained"),
    ));
    assert_eq!(
        parked_fingerprint,
        blocks_fingerprint(std::slice::from_ref(&b2)),
        "the retained side fork must be byte-identical to the submitted block"
    );
    dump_balances(&chain, STAGE_ASSERT, miner_a, miner_b, bob, carol);
    let result = chain.process_block(b3).expect("fork extends");
    match result {
        BlockProcessResult::Reorged {
            disconnected,
            connected,
        } => {
            assert_eq!(disconnected, vec![2]);
            assert_eq!(connected, vec![2, 3]);
        }
        other => panic!("expected Reorged, got {:?}", other),
    }
    dump_balances(&chain, STAGE_ASSERT, miner_a, miner_b, bob, carol);
    assert_eq!(chain.state.balance(carol), 180);
    assert_eq!(chain.state.balance(bob), 0);
}

#[test]
fn conflicting_reorg_rolls_back_and_restores() {
    let _tmp = setup_test_dir("conflict_restore");
    reset_testnet();
    let miner_a = test_keypair(3).1;
    let miner_b = test_keypair(4).1;
    let bob = test_keypair(5).1;
    let carol = test_keypair(6).1;

    let mut chain = Chain::genesis();
    let b1 = mine_tip_block(&mut chain, miner_a);
    fund_alice(chain.state_mut());
    let b1_hash = chain.block_hash(&b1);
    let chain_id = chain.chain_id.clone();
    let base_state = b1_state(&chain);
    let spend_bob = alice_spend(&spend_outputs(bob), chain_id.as_bytes());
    let spend_carol = alice_spend(&spend_outputs(carol), chain_id.as_bytes());

    let mut main_scratch = base_state.clone();
    let a2 = build_fork_block(
        &mut chain,
        &mut main_scratch,
        2,
        200,
        b1_hash,
        miner_a,
        vec![spend_bob],
        true,
        STAGE_MAIN,
    );
    chain.apply(a2.clone()).expect("branch A applies");
    let a2_hash = chain.block_hash(&a2);

    // Branch B is heavier and reorgs branch A away.
    // The history entries are shared snapshots; the working copy
    // detaches the value so the fork builder can advance it.
    let mut fork_scratch = base_state.clone();
    let b2 = build_fork_block(
        &mut chain,
        &mut fork_scratch,
        2,
        200,
        b1_hash,
        miner_b,
        vec![spend_carol],
        true,
        STAGE_BUILD,
    );
    let b2_hash = chain.block_hash(&b2);
    let mut s3 = fork_scratch.clone();
    let b3 = build_fork_block(
        &mut chain,
        &mut s3,
        3,
        300,
        b2_hash,
        miner_b,
        vec![],
        true,
        STAGE_BUILD,
    );
    chain.process_block(b2).expect("side fork parks");
    chain.process_block(b3).expect("fork extends");
    dump_balances(&chain, STAGE_ADOPT, miner_a, miner_b, bob, carol);

    // Branch B state after the reorg.
    assert_eq!(chain.state.balance(carol), 180);
    assert_eq!(chain.state.balance(bob), 0);

    // Branch A becomes heavier again and reorgs branch B away: the
    // original owner is restored exactly.
    let mut restore_scratch = base_state.clone();
    let a3 = build_fork_block(
        &mut chain,
        &mut restore_scratch,
        3,
        250,
        a2_hash,
        miner_a,
        vec![],
        true,
        STAGE_ADOPT,
    );
    chain.process_block(a3).expect("branch A extends");
    let a3_hash = chain.tip_hash();
    let a4 = build_fork_block(
        &mut chain,
        &mut restore_scratch,
        4,
        350,
        a3_hash,
        miner_a,
        vec![],
        true,
        STAGE_ADOPT,
    );
    chain.process_block(a4).expect("branch A extends");
    dump_balances(&chain, STAGE_ASSERT, miner_a, miner_b, bob, carol);
    assert_eq!(chain.state.balance(bob), 180);
    assert_eq!(chain.state.balance(carol), 0);
}
