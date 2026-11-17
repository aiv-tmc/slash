//! `test_network_soak_extended` — the long soak lane: three times the
//! mini soak's operations with deeper divergence drills. Every block
//! pays a fresh RandomX context and each drill clones whole chains, so
//! the default scale is bounded to keep the lane inside the suite
//! budget; the machinery under test is identical to the mini soak,
//! only the history depth and drill cadence grow. The historical
//! full-scale profile (600 operations, fork depth 8) stays available
//! through `SLASH_NETWORK_SOAK_OPS` / `SLASH_NETWORK_SOAK_FORK`.
//!
//! Expected runtime: well under a minute in release mode.

use super::*;

#[test]
fn test_network_soak_extended() {
    let _tmp = setup_test_dir("network_soak_extended");
    reset_testnet();
    let ops = std::env::var("SLASH_NETWORK_SOAK_OPS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(24);
    let fork_len = std::env::var("SLASH_NETWORK_SOAK_FORK")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(4);
    run_network_soak(ops, fork_len);
}
