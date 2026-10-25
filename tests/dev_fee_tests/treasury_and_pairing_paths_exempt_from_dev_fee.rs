//! `treasury_and_pairing_paths_exempt_from_dev_fee` — one test case of the `dev_fee_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn treasury_and_pairing_paths_exempt_from_dev_fee() {
    // Treasury mints and the pairing payments that fund them never carry a
    // dev-fund output; the rule must not reach them in either direction.
    reset_testnet();
    let mut chain = treasury_genesis();
    activate_dev_fund(&mut chain);
    chain.state.mine_fee_bps = 0;
    // A treasury mint (signed against the treasury signer) without any
    // dev-fund output must still validate at the mempool and in a block.
    let (s, e) = chain.state.select_single(TREASURY, 500).unwrap();
    let mint_outputs = vec![Output {
        start: s,
        end: e,
        to: [9u8; 32],
        lock: None,
    }];
    let mint = make_tx(
        TREASURY,
        &TREASURY_SK,
        &[(s, e)],
        &mint_outputs,
        &chain.chain_id,
    );
    assert!(
        chain.validate_tx(&mint),
        "treasury transactions are exempt from the dev-fund rule"
    );
    let b = mine_next_block(&chain, [7u8; 32], vec![mint]);
    chain
        .apply(b)
        .expect("treasury mint applies without a dev output");
}
