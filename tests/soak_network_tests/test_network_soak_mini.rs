//! `test_network_soak_mini` — short soak: hundreds of operations with
//! frequent shallow reorgs, run on every CI build.

use super::*;

#[test]
fn test_network_soak_mini() {
    let _tmp = setup_test_dir("network_soak_mini");
    reset_testnet();
    let (ops, fork_len) = (300, 3);
    run_network_soak(ops, fork_len);
}
