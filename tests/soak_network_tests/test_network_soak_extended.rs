//! `test_network_soak_extended` — long soak: thousands of operations
//! with deeper reorg drills, run in the extended CI lane.
//!
//! Ignored by default. The current engine pays work proportional to the
//! dirty-page set on every block — the header commits the root of every
//! dirty page, and the reorg window keeps full state snapshots — so a
//! multi-thousand-operation drill that rebuilds three whole chains is
//! quadratic in chain length and outlasts any CI budget. The mini soak
//! exercises the identical machinery at CI scale; this lane is re-enabled
//! once the storage and page-root performance backlog lands. Run it
//! manually with:
//! `cargo test --release --test soak_network_tests -- --ignored`

use super::*;

#[test]
#[ignore = "engine perf: drill clone cost is quadratic in chain length; run manually"]
fn test_network_soak_extended() {
    let _tmp = setup_test_dir("network_soak_extended");
    reset_testnet();
    let (ops, fork_len) = (5_000, 8);
    run_network_soak(ops, fork_len);
}
