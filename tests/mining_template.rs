//! Regression coverage for the node mining template path: a freshly
//! assembled template (no nonce yet) must pass `validate_block_template`
//! so the miner reaches the nonce search, while the full
//! `validate_block` keeps rejecting it until a valid proof exists.

use slash::chain;
use slash::test_fixtures::setup_test_dir;
use std::sync::Arc;

#[test]
fn mining_template_validates_before_the_nonce_exists() {
    let _tmp = setup_test_dir("mining_template");
    let mut chain = chain::Chain::genesis();
    let miner = [0x42u8; 32];
    let height = chain.tip_height() + 1;
    let difficulty = chain.next_difficulty();
    let coinbase = chain::Chain::build_coinbase(&chain.state, miner, height, 0)
        .expect("coinbase exists while the subsidy is positive");
    let block = chain::Block {
        version: 1,
        prev: chain.tip_hash(),
        time: height * chain::TARGET_BLOCK_TIME_SECS,
        height,
        nonce: 0,
        miner,
        mined_cell: 0,
        txs: vec![coinbase],
        fee_claims: Vec::new(),
        difficulty,
        page_roots: Default::default(),
    };
    let block = chain.prepare_block(block);

    // The template has no valid proof yet: the full validator must keep
    // rejecting it (the node relies on this after grinding).
    assert!(!chain.validate_block(&block));
    // But every nonce-independent gate must pass so mining can start.
    assert!(
        chain.validate_block_template(&block),
        "mining template must validate before the nonce exists"
    );

    // Complete the proof the way the miner does and confirm the full
    // validator now accepts the block.
    let prev = chain.tip_hash();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let solution =
        slash::mining::mine_light(prev, block.time, height, miner, difficulty, cancel, |_| {
            true
        })
        .expect("difficulty-1 search succeeds");
    let mut mined = block;
    mined.nonce = solution.nonce;
    mined.time = solution.time;
    mined.mined_cell = solution.value % slash::state::N;
    let mined = chain.prepare_block(mined);
    assert!(
        chain.validate_block(&mined),
        "mined template must pass the full validator"
    );
    chain.apply(mined).unwrap();
    assert_eq!(chain.tip_height(), 1);
}
