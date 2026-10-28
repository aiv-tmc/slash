//! `test_network_soak_mini` — the CI soak: a bounded number of
//! operations across three nodes with periodic divergence drills and the
//! conservation invariant checked after every round, under the driver's
//! wall-clock watchdog.
//!
//! The size is a budget: the runtime of a soak grows with the operation
//! count, and the parameters below keep the whole target inside the CI
//! time budget while exercising every path the soak exists for:
//! round-robin mining on divergent views, payload transfers, buy-pair
//! pairing, reorg drills, and the per-node conservation checks. The
//! extended target drives the same machinery at a length meant for the
//! manual lane.

use super::*;

#[test]
fn test_network_soak_mini() {
    let _tmp = setup_test_dir("network_soak_mini");
    reset_testnet();
    let (ops, fork_len) = (60, 6);
    run_network_soak(ops, fork_len);
}
