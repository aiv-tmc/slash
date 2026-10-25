//! `test_wire_format_contract` — one test case of the `wire_format_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_wire_format_contract() {
    // Single place that pins every consensus-critical size. A failure here
    // is a serialization fork: update the constants and the layout comments
    // deliberately, and bump the network protocol identifier.
    let tx = Tx {
        from: [0u8; 32],
        inputs: vec![],
        outputs: vec![],
        sig: vec![],
        scheme: 0,
        memo: None,
    };
    assert_eq!(bincode::serialize(&tx).unwrap().len(), TX_MINIMAL_SIZE);

    let block = Block {
        version: 1,
        prev: [0u8; 32],
        time: 0,
        height: 0,
        nonce: 0,
        miner: [0u8; 32],
        mined_cell: 0,
        txs: vec![],
        fee_claims: vec![],
        difficulty: 1,
        page_roots: BTreeMap::new(),
    };
    assert_eq!(bincode::serialize(&block).unwrap().len(), BLOCK_EMPTY_SIZE);

    let locked = Output {
        start: 0,
        end: 1,
        to: [0u8; 32],
        lock: Some(1),
    };
    assert_eq!(
        bincode::serialize(&locked).unwrap().len(),
        OUTPUT_LOCKED_SIZE
    );

    let unlocked = Output {
        start: 0,
        end: 1,
        to: [0u8; 32],
        lock: None,
    };
    assert_eq!(
        bincode::serialize(&unlocked).unwrap().len(),
        OUTPUT_UNLOCKED_SIZE
    );
}
