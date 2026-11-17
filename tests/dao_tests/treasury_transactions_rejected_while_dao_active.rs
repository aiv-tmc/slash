//! `treasury_transactions_rejected_while_dao_active` — one case of the `dao_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn treasury_transactions_rejected_while_dao_active() {
    // Sunset: with dao_v1 active, a signed non-coinbase treasury transaction
    // is refused by the mempool and by block execution, while the same
    // transaction is accepted before activation.
    reset_testnet();
    let mut chain = treasury_genesis();
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
        "before activation the legacy signer keeps its authority"
    );
    activate_dao(&mut chain);
    assert!(
        !chain.validate_tx(&mint),
        "mempool must refuse treasury transactions once governance is live"
    );
    // The block is built by hand: the ordinary fixture refuses to produce
    // an unexecutable block, while this test's whole point is a block the
    // consensus must reject. A normally prepared empty template carries
    // the refused transaction into apply, where the sunset fires. The
    // fixture stamps the template itself, so only the time, the parent
    // hash, and the difficulty are supplied here.
    let mut b = mine_valid_block(
        &chain,
        chain.blocks.last().map(|b| b.time + 1).unwrap_or(1),
        chain.tip_hash(),
        1,
    );
    b.txs.push(mint);
    assert!(
        chain.apply(b).is_err(),
        "consensus must refuse treasury transactions once governance is live"
    );
}
