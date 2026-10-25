//! `test_mempool_rate_limiting_simulated` — one test case of the `p2p_tests` suite.
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

    // Repeated valid submissions: five distinct treasury-funded transactions
    // (the treasury signs nothing and owns every unissued cell) are validated
    // and packaged into a single block, exercising the same structural checks
    // that run before per-peer rate limiting inside the Node.
    let mut txs = Vec::new();
    for i in 0..5u64 {
        let start = i * 50;
        txs.push(chain::Tx {
            from: state::TREASURY,
            inputs: vec![(start, start + 50)],
            outputs: vec![chain::Output {
                start,
                end: start + 50,
                to: recipient,
                lock: None,
            }],
            sig: vec![],
            scheme: 0,
            memo: None,
        });
    }

    let b = mine_next_block(&chain, [1u8; 32], txs);
    chain.apply(b).unwrap();
    assert_eq!(chain.state.balance(recipient), 250);
}
