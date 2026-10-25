//! `test_block_gossip_payload_size` — one test case of the `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_block_gossip_payload_size() {
    let _tmp = setup_test_dir("gossip_size");
    reset_testnet();
    // The chain is only read by the block builder below.
    let chain = chain::Chain::genesis();
    let block = mine_next_block(&chain, [1u8; 32], vec![]);
    let bytes = bincode::serialize(&block).unwrap();
    assert!(!bytes.is_empty());
    assert!(bytes.len() <= 4 * 1024 * 1024);
}
