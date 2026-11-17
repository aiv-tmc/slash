//! `test_dos_invalid_pow_rejected` — one test case of the `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; this test's body adds its own collection import for
//! the empty page-root map.

use super::*;
use std::collections::BTreeMap;

#[test]
fn test_dos_invalid_pow_rejected() {
    let _tmp = setup_test_dir("dos_pow");
    reset_testnet();
    let chain = chain::Chain::genesis();

    let mut block = chain::Block {
        version: 1,
        prev: chain.tip_hash(),
        time: 1,
        height: 1,
        nonce: 0,
        miner: [1u8; 32],
        mined_cell: 0,
        txs: vec![],
        fee_claims: vec![],
        difficulty: chain.next_difficulty(),
        page_roots: BTreeMap::new(),
    };
    // A nonce with no matching proof output must never validate, whatever
    // the rest of the header looks like.
    block.nonce = 12345;

    assert!(!chain.validate_block(&block));
}
