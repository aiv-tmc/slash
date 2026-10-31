//! `test_fee_only_tail_simulation_sweep` — one case of the
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
    // moment any tail invariant regresses.
    //
    // The block count is deliberately small: every block introduces a
    // fresh parent hash, and the consensus-mandated RandomX cache
    // derivation behind each fresh parent costs real seconds, so a long
    // sweep spends its runtime on cryptography setup rather than on the
    // invariants under test. The sweep claims the whole vault every
    // block, so per-block coverage saturates after the first few
    // iterations: accrual across blocks never happens by construction.
    reset_testnet();
    let (_, miner) = crypto::generate_keypair();
    let config = slash::simulation::SimulationConfig {
        miners: vec![miner],
        target_block_time_secs: 300,
        initial_blocks: 2,
    };
    assert!(
        slash::simulation::simulate_fee_only_tail(&config, 8),
        "the fee-only tail simulation must survive every per-block invariant"
    );
}
