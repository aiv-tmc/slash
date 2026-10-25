//! `dev_fund_spends_capped_at_one_per_block` — one test case of the `dev_fee_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn dev_fund_spends_capped_at_one_per_block() {
    // The claim path: only one spend from DEV_FUND may execute per block.
    // Two properly signed dev-fund spends in the same block are rejected,
    // a single spend is accepted, and an unsigned spend is always rejected.
    reset_testnet();
    let mut chain = treasury_genesis();
    let (dev_sk, dev_pk) = slash::crypto::generate_keypair();
    chain.state.dev_fund_signer = dev_pk;
    // Seed the dev fund through a treasury mint.
    let (s, e) = chain.state.select_single(TREASURY, 200).unwrap();
    let seed = vec![Output {
        start: s,
        end: e,
        to: DEV_FUND,
        lock: None,
    }];
    let seed_tx = make_tx(TREASURY, &TREASURY_SK, &[(s, e)], &seed, &chain.chain_id);
    let b = mine_next_block(&chain, [7u8; 32], vec![seed_tx]);
    chain.apply(b).unwrap();
    assert_eq!(chain.state.balance(DEV_FUND), 200);

    let chain_id = chain.chain_id.clone();
    let mk_spend = |range: (u64, u64), to: [u8; 32]| {
        let outputs = vec![Output {
            start: range.0,
            end: range.1,
            to,
            lock: None,
        }];
        make_tx(DEV_FUND, &dev_sk, &[range], &outputs, &chain_id)
    };
    let spend1 = mk_spend((s, s + 50), [8u8; 32]);
    let spend2 = mk_spend((s + 50, s + 100), [8u8; 32]);

    // Single spend: accepted.
    let b = mine_next_block(&chain, [7u8; 32], vec![spend1.clone()]);
    chain
        .apply(b)
        .expect("one dev-fund spend per block is valid");

    // Two spends: the second one breaks the cap, the whole block is invalid.
    let b = mine_next_block(&chain, [7u8; 32], vec![spend1, spend2]);
    assert!(
        chain.apply(b).is_err(),
        "two dev-fund spends in one block must be rejected"
    );

    // Unsigned spend: always rejected, active rule or not.
    let mut unsigned = mk_spend((s, s + 50), [8u8; 32]);
    unsigned.sig = Vec::new();
    assert!(!chain.validate_tx(&unsigned));
}
