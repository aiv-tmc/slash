//! `test_header_first_validation_chain` — one test case of the `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_header_first_validation_chain() {
    let _tmp = setup_test_dir("header_sync");
    reset_testnet();
    let mut chain = chain::Chain::genesis();
    let miner = [1u8; 32];

    let mut headers = Vec::new();
    for _ in 0..5 {
        let block = mine_next_block(&chain, miner, vec![]);
        let header = chain::BlockHeader::from_block(&block);
        headers.push(header);
        chain.apply(block).unwrap();
    }

    for header in &headers {
        assert!(header.height > 0);
        // Every header after the first names a real parent hash; the
        // first header's parent is the all-zero genesis anchor, which is
        // the one legitimate all-zero prev in the chain.
        assert!(
            header.prev != [0u8; 32] || header.height == 1,
            "only the first header may point at the genesis anchor"
        );
    }
}
