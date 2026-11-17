//! `test_header_first_validation_chain` — one test case of the `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_header_first_validation_chain() {
    let _tmp = setup_test_dir("header_sync");
    reset_testnet();
    let miner = [1u8; 32];
    let chain = shared_mined_chain(miner, 5);
    let headers: Vec<_> = chain
        .blocks
        .iter()
        .map(chain::BlockHeader::from_block)
        .collect();

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
