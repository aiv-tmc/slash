//! Shared helper functions of the `soak_invariants_tests` suite: the
//! deterministic soak driver `run_soak`, which replays a fixed-seed
//! operation sequence and asserts the global invariants after every
//! mutation. The operation implementations live in `soak_ops`. The
//! module carries no `#[test]` items itself.

use super::*;

#[path = "soak_ops.rs"]
mod soak_ops;

/// Run the deterministic soak for the given number of iterations: every
/// iteration draws one operation against the fixed-seed generator, and
/// the global invariants are asserted after every completed operation —
/// including rejected transactions — because a well-behaved engine must
/// never corrupt balances even when a transaction rolls back. An
/// operation that early-outs before applying anything skips the
/// invariant block for that iteration, exactly as the inline loop did.
pub fn run_soak(iterations: usize) {
    let mut state = State::genesis();
    let mut rng = Rng(0x1234_5678_9ABC_DEF0);
    let mut current_height = 0u64;

    // A configured treasury signer allows buy/sell pairs to be signed.
    let (treasury_sk, treasury_pk) = crypto::generate_keypair();
    state.treasury_signer = treasury_pk;

    // Pre-mine a contiguous block of 5,000 cells per owner so transfer
    // operations have deterministic inputs without scanning the ledger.
    let mut owners: Vec<([u8; 32], [u8; 32])> = Vec::new();
    let mut bases: Vec<u64> = Vec::new();
    for i in 0..4u64 {
        let (sk, pk) = crypto::generate_keypair();
        let base = i * 20_000;
        for cell in base..base + 5_000 {
            state.mine(cell, pk, 0);
        }
        owners.push((sk, pk));
        bases.push(base);
    }

    let mut last_total_sold = 0u64;
    let mut last_fee_vault = 0u64;

    for _ in 0..iterations {
        let completed = soak_ops::soak_op(
            &mut state,
            &mut rng,
            &owners,
            &bases,
            &mut current_height,
            &treasury_sk,
        );
        if !completed {
            continue;
        }

        // Invariant checks after every completed operation, including
        // failed transactions: a well-behaved state engine must never
        // corrupt balances even when a transaction is rejected and
        // rolled back.
        let supply_sum: u64 = state.balance_cache.values().sum();
        assert_eq!(supply_sum, N, "supply conservation invariant violated");
        assert!(
            state.balance_cache.values().all(|&v| v > 0),
            "a zero balance entry must not remain in the cache after a soak operation"
        );
        assert!(
            state.total_sold >= last_total_sold,
            "total_sold must be monotonic"
        );
        last_total_sold = state.total_sold;
        assert!(
            state.total_bought_back <= state.total_sold,
            "total_bought_back must never exceed total_sold"
        );
        let fee_vault = state.balance(FEE_VAULT);
        assert!(
            fee_vault >= last_fee_vault,
            "fee vault balance must be non-decreasing"
        );
        last_fee_vault = fee_vault;
        assert!(
            state.locks.len() as u64 <= MAX_LOCKED_CELLS,
            "global lock cap exceeded"
        );
    }
}
