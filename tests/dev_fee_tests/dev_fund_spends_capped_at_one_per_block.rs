//! `dev_fund_spends_capped_at_one_per_block` — one test case of the
//! `dev_fee_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn dev_fund_spends_capped_at_one_per_block() {
    // The claim path: only one spend from DEV_FUND may execute per block.
    // Two properly signed dev-fund spends in the same block are rejected,
    // a single spend is accepted, and an unsigned spend is always
    // rejected. The fund is seeded by direct mining: a treasury mint
    // without a matching buy payment is invalid by the pairing rules, so
    // it cannot seed a test that is not about pairing. Every spend pays
    // the mandatory fee like any non-treasury transaction.
    reset_testnet();
    let mut chain = treasury_genesis();
    let (dev_sk, dev_pk) = slash::crypto::generate_keypair();
    chain.state.dev_fund_signer = dev_pk;
    for cell in 0..200 {
        chain.state.mine(cell, DEV_FUND, 0);
    }
    assert_eq!(chain.state.balance(DEV_FUND), 200);

    let chain_id = chain.chain_id.clone();
    let mk_spend = |range: (u64, u64), to: [u8; 32]| {
        let outputs = vec![
            Output {
                start: range.0,
                end: range.1 - 1,
                to,
                lock: None,
            },
            Output {
                start: range.1 - 1,
                end: range.1,
                to: FEE_VAULT,
                lock: None,
            },
        ];
        make_tx(DEV_FUND, &dev_sk, &[range], &outputs, &chain_id)
    };
    let spend1 = mk_spend((0, 100), [8u8; 32]);
    let spend2 = mk_spend((100, 200), [8u8; 32]);

    // Single spend: accepted.
    let b = mine_next_block(&chain, [7u8; 32], vec![spend1.clone()]);
    chain
        .apply(b)
        .expect("one dev-fund spend per block is valid");

    // Two spends: the second one breaks the cap, the whole block is
    // invalid. Injected into an empty block because the mining fixture
    // panics on templates it cannot execute.
    let mut b = mine_next_block(&chain, [7u8; 32], vec![]);
    b.txs.push(spend1);
    b.txs.push(spend2);
    assert!(
        chain.apply(b).is_err(),
        "two dev-fund spends in one block must be rejected"
    );

    // Unsigned spend: always rejected, active rule or not.
    let mut unsigned = mk_spend((0, 100), [8u8; 32]);
    unsigned.sig = Vec::new();
    assert!(!chain.validate_tx(&unsigned));
}
