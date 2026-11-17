//! CPU-heavy consensus walks: retarget arithmetic across a full
//! difficulty interval, state-history window growth, and stake-lock
//! expiry over a hundred blocks.
//!
//! Every test here mines real RandomX proofs and executes the full state
//! transition per block, so the suite is compute-bound by design: block
//! counts follow consensus constants (2,016-block retarget interval,
//! 100-block reorg window) and cannot be shrunk without changing what
//! the tests prove. Kept in their own integration target so the main
//! `chain_tests` suite stays fast under the default test-thread pool;
//! run this target with limited parallelism, e.g.
//! `cargo test --release --test slow_chain_tests -- --test-threads=2`.
//!
//! The crate root re-exports the fixture namespace and the `chain` and
//! `state` modules so every submodule reaches constants, types, and
//! helpers through one glob import.

pub use slash::chain;
pub use slash::state;
pub use slash::test_fixtures::*;

mod history;
mod meta;
mod staking;
