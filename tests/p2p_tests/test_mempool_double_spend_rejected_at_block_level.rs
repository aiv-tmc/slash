//! `test_mempool_double_spend_rejected_at_block_level` — one test case of the `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_mempool_double_spend_rejected_at_block_level() {
    let _tmp = setup_test_dir("ds_block");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let bob = [2u8; 32];
    let charlie = [3u8; 32];
    // Alice is the public key matching the signing secret used for both spends.
    let (sk, pk) = crypto::generate_keypair();
    let alice = pk;

    // Seed alice with 50 cells; both transactions below are individually
    // well-formed and correctly signed.
    let seed = chain::Output {
        start: 0,
        end: 50,
        to: alice,
        lock: None,
    };
    let seed_tx = chain::Tx {
        from: state::TREASURY,
        inputs: vec![(0, 50)],
        outputs: vec![seed],
        sig: vec![],
        scheme: 0,
        memo: None,
    };
    let b1 = mine_next_block(&chain, state::TREASURY, vec![seed_tx]);
    chain.apply(b1).unwrap();

    let tx1 = make_tx(
        alice,
        &sk,
        &[(0, 50)],
        &[state::Output {
            start: 0,
            end: 50,
            to: bob,
            lock: None,
        }],
        &chain.chain_id,
    );
    let tx2 = make_tx(
        alice,
        &sk,
        &[(0, 50)],
        &[state::Output {
            start: 0,
            end: 50,
            to: charlie,
            lock: None,
        }],
        &chain.chain_id,
    );

    // The first spend consumes the range during the state replay, so the
    // second one no longer finds it owned by alice and the block is invalid.
    let b2 = mine_next_block(&chain, state::TREASURY, vec![tx1, tx2]);
    assert!(!chain.validate_block(&b2));
}
