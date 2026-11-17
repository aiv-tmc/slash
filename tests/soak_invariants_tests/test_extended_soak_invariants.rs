//! `test_extended_soak_invariants` — the long invariant profile:
//! 100,000 deterministic operations by default (override with
//! `SLASH_SOAK_OPS`) with full invariant checks after every mutation.
//! Pure state work with no proof-of-work, so the runtime stays in the
//! tens of seconds even in the dev profile. The randomized operation mix
//! covers the same transfer, lock, treasury, and mint paths as the
//! mini soak but exercises far longer sequences — history eviction,
//! lock pruning, and cache-parity bugs typically need thousands of
//! steps to surface.

use super::*;

#[test]
fn test_extended_soak_invariants() {
    let ops = std::env::var("SLASH_SOAK_OPS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(100_000);
    run_soak(ops);
}
