//! `test_dos_future_timestamp_rejected` — one test case of the `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; this test's body adds its own collection import for
//! the empty page-root map.

use super::*;
use std::collections::BTreeMap;

#[test]
fn test_dos_future_timestamp_rejected() {
    let _tmp = setup_test_dir("dos_future");
    reset_testnet();
    let chain = chain::Chain::genesis();

    let mut block = chain::Block {
        version: 1,
        prev: chain.tip_hash(),
        time: chrono::Utc::now().timestamp() as u64 + 7201,
        height: 1,
        nonce: 0,
        miner: [1u8; 32],
        mined_cell: 0,
        txs: vec![],
        fee_claims: vec![],
        difficulty: chain.next_difficulty(),
        page_roots: BTreeMap::new(),
    };
    // The nonce search only makes the PoW valid; the timestamp rule must
    // still reject the block afterwards.
    loop {
        if chain::verify_pow(
            block.prev,
            block.time,
            block.height,
            block.miner,
            block.nonce,
            block.difficulty,
        )
        .is_some()
        {
            break;
        }
        block.nonce += 1;
    }

    assert!(!chain.validate_block(&block));
}
