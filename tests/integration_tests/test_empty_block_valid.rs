//! `test_empty_block_valid` — one test case of the `integration_tests` suite.
//!
//! The shared helpers come from the crate root via `use super::*;`; the
//! `Chain` type is imported explicitly so this file compiles on its own.

use super::*;

#[test]
fn test_empty_block_valid() {
    // A block carrying no transactions beyond the mandatory coinbase is
    // valid: the PoW slot mint alone completes the payload. The coinbase
    // is mandatory while the subsidy is positive, so the block is built
    // through the mining fixture, which attaches it.
    let miner = [1u8; 32];
    let chain = shared_mined_chain(miner, 1);
    assert_eq!(chain.tip_height(), 1);
}
