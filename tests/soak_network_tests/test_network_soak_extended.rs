//! `test_network_soak_extended` — long soak: thousands of operations
//! with deeper reorg drills, run in the extended CI lane.

use super::*;

#[test]
fn test_network_soak_extended() {
    let _tmp = setup_test_dir("network_soak_extended");
    reset_testnet();
    let (ops, fork_len) = (5_000, 8);
    run_network_soak(ops, fork_len);
}
