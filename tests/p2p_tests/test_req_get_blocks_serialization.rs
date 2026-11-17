//! `test_req_get_blocks_serialization` — one test case of the `p2p_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_req_get_blocks_serialization() {
    let req = p2p::Req::GetBlocks(10, 20);
    let bytes = bincode::serialize(&req).unwrap();
    let restored: p2p::Req = bincode::deserialize(&bytes).unwrap();
    assert!(matches!(restored, p2p::Req::GetBlocks(10, 20)));
}
