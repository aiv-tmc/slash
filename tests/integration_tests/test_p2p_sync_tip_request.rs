//! `test_p2p_sync_tip_request` — one test case of the `integration_tests` suite.
//!
//! The shared helpers come from the crate root via `use super::*;`; the
//! `Chain` type is imported explicitly so this file compiles on its own.

use super::*;
use slash::chain::Chain;

#[test]
fn test_p2p_sync_tip_request() {
    // Two chains in equivalent states must report identical tips.
    let mut a = Chain::genesis();
    let mut b = Chain::genesis();
    for _ in 0..3 {
        // Block assembly reads the chain without mutating it; the mut
        // bindings stay on the apply calls.
        let blk = mine_next_block(&a, [1u8; 32], vec![]);
        a.apply(blk.clone()).unwrap();
        b.apply(blk).unwrap();
    }
    assert_eq!(a.tip_height(), b.tip_height());
    assert_eq!(a.tip_hash(), b.tip_hash());
}
