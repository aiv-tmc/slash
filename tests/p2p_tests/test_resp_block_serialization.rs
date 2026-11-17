//! `test_resp_block_serialization` — one test case of the `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_resp_block_serialization() {
    // The chain is only read by the block builder, so the binding stays
    // immutable.
    let chain = chain::Chain::genesis();
    let block = mine_next_block(&chain, [1u8; 32], vec![]);
    let resp = p2p::Resp::Block(Some(block));
    let bytes = bincode::serialize(&resp).unwrap();
    let restored: p2p::Resp = bincode::deserialize(&bytes).unwrap();
    assert!(matches!(restored, p2p::Resp::Block(Some(_))));
}
