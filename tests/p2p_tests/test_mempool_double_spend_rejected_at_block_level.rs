//! `test_mempool_double_spend_rejected_at_block_level` — one test case of
//! the `p2p_tests` suite.
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

    // Seed alice by direct mining: a treasury mint without a matching buy
    // payment is invalid by the pairing rules, so it cannot seed a test
    // that is not about pairing. Both transactions below are individually
    // well-formed and correctly signed.
    for i in 0..50 {
        chain.state_mut().mine(i, alice, 0);
    }

    let tx1 = make_tx(
        alice,
        &sk,
        &[(0, 50)],
        &[
            state::Output {
                start: 0,
                end: 49,
                to: bob,
                lock: None,
            },
            state::Output {
                start: 49,
                end: 50,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        &chain.chain_id,
    );
    let tx2 = make_tx(
        alice,
        &sk,
        &[(0, 50)],
        &[
            state::Output {
                start: 0,
                end: 49,
                to: charlie,
                lock: None,
            },
            state::Output {
                start: 49,
                end: 50,
                to: state::FEE_VAULT,
                lock: None,
            },
        ],
        &chain.chain_id,
    );

    // The first spend consumes the range during the state replay, so the
    // second one no longer finds it owned by alice and the block is
    // invalid. The pair is injected into a valid empty template because
    // the mining fixture panics on templates it cannot execute.
    let mut b2 = mine_next_block(&chain, [1u8; 32], vec![]);
    b2.txs.push(tx1);
    b2.txs.push(tx2);
    assert!(!chain.validate_block(&b2));
}
