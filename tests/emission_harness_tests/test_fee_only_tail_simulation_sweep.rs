//! `test_fee_only_tail_simulation_sweep` — one test case of the
//! `emission_harness_tests` suite.
//!
//! The subsidy boundary and the perpetual slot mint are pinned at
//! pure-function and state level by the neighboring tests; this sweep
//! pins the economic tail end to end: real signed fee payments land in
//! the vault, the miner sweeps them through fee claims, and every block
//! closes with the vault drained, the miner revenue equal to subsidy
//! plus fees plus the slot cell, and the balance cache in lockstep with
//! the ledger.

use super::*;

#[test]
fn test_fee_only_tail_simulation_sweep() {
    // The sweep drives a live chain: signed fee payments, miner vault
    // sweeps, and per-block assertions on revenue, vault drain, and cache
    // parity live inside `simulate_fee_only_tail`, so the test fails the
    // moment any tail invariant regresses. The miner address is a fresh
    // random keypair: the sweep only needs an address to pay, never the
    // secret, and the per-block assertions do not depend on the key.
    reset_testnet();
    let (_, miner) = crypto::generate_keypair();
    let config = slash::simulation::SimulationConfig {
        miners: vec![miner],
        target_block_time_secs: 300,
        initial_blocks: 3,
    };
    assert!(
        slash::simulation::simulate_fee_only_tail(&config, 20),
        "the fee-only tail simulation must survive every per-block invariant"
    );
}
