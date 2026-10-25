//! Simulated multi-node network soak: three in-memory chains exchange
//! payloads through the shared fixtures for thousands of operations,
//! with periodic divergence drills and per-operation invariant checks.
//!
//! The crate root re-exports every helper module so the test bodies and
//! the builders reach one shared namespace.

pub use slash::test_fixtures::*;

mod support;
pub use support::*;

mod builders;
pub use builders::*;

mod driver;
pub use driver::*;

mod test_network_soak_extended;
mod test_network_soak_mini;
