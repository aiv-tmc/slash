//! Deterministic soak test with per-operation invariant checks.
//!
//! A multi-day multi-node soak is a deployment activity, but its core value
//! — the invariants that must hold after every single mutation — can be
//! exercised continuously in CI at a bounded cost. This module runs a
//! seeded pseudo-random sequence of mining, transfers, stake locks, lock
//! pruning, and treasury buy/sell pairs against one state, checking the
//! global invariants after every operation, including operations that fail
//! and roll back.
//!
//! The randomness is a fixed-seed 64-bit linear congruential generator, so
//! every CI run replays the identical operation sequence; a failure is
//! therefore reproducible locally. The extended variant multiplies the
//! iteration count twentyfold and is marked `#[ignore]` for manual runs.

use slash::chain;
use slash::crypto;
use slash::state::{
    Output, State, FEE_VAULT, MAX_LOCKED_CELLS, MIN_FEE_DENOMINATOR, N, TREASURY, TREASURY_VAULT,
};
use slash::treasury;

/// Fixed-seed LCG. The multiplier is the constant of the MMIX generator;
/// the seed is fixed so the operation sequence is identical on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn range(&mut self, max: u64) -> u64 {
        self.next() % max
    }
}

// Test modules: every `#[test]` function of this crate lives
// in its own small file under this directory for readability.
mod test_extended_soak_invariants;
mod test_mini_soak_invariants;

mod support;
pub use support::*;
