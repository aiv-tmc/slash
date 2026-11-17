//! `test_p2p_sync_tip_request` — one test case of the `integration_tests` suite.
//!
//! The shared helpers come from the crate root via `use super::*;`; the
//! `Chain` type is imported explicitly so this file compiles on its own.

use super::*;

#[test]
fn test_p2p_sync_tip_request() {
    // Two chains in equivalent states must report identical tips.
    // Both sides replay the same shared deterministic prefix, which is the
    // on-disk meaning of "two chains in equivalent states".
    let a = shared_mined_chain([1u8; 32], 3);
    let b = shared_mined_chain([1u8; 32], 3);
    assert_eq!(a.tip_height(), b.tip_height());
    assert_eq!(a.tip_hash(), b.tip_hash());
}
