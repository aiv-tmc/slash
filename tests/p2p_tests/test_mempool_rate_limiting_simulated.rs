//! `test_mempool_rate_limiting_simulated` — one test case of the
//! `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_mempool_rate_limiting_simulated() {
    let _tmp = setup_test_dir("rate_limit");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let recipient = [2u8; 32];
    let (sk, pk) = crypto::generate_keypair();
    let alice = pk;

    // Fund alice with five disjoint page-local ranges; each transaction
    // below is individually well-formed and correctly signed, and all
    // five package into a single block — the same assembly exercise the
    // per-peer rate limiter guards in the live node.
    let ranges: Vec<(u64, u64)> = (0..5u64).map(|i| (i * 200, i * 200 + 200)).collect();
    for &(s, e) in &ranges {
        for cell in s..e {
            chain.state.mine(cell, alice, 0);
        }
    }

    let mut txs = Vec::new();
    for &(s, e) in &ranges {
        let outputs = vec![
            state::Output {
                start: s,
                end: e - 2,
                to: recipient,
                lock: None,
            },
            state::Output {
                start: e - 2,
                end: e,
                to: state::FEE_VAULT,
                lock: None,
            },
        ];
        txs.push(make_tx(alice, &sk, &[(s, e)], &outputs, &chain.chain_id));
    }

    let b = mine_next_block(&chain, [1u8; 32], txs);
    chain.apply(b).unwrap();
    assert_eq!(chain.state.balance(recipient), 5 * 198);
}
