//! `test_block_empty_size` — one test case of the `wire_format_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_block_empty_size() {
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
    let bytes = bincode::serialize(&block).unwrap();
    assert_eq!(
        bytes.len(),
        BLOCK_EMPTY_SIZE,
        "block wire size drifted: {} bytes",
        bytes.len()
    );
}
