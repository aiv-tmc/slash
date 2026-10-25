//! Consensus and chain-level integration tests.
//!
//! The modules of this crate each cover one consensus area: transfers,
//! multi-input pages, fee claims, staking locks, reorg handling,
//! conflicting-branch adoption, block metadata rules, and structural
//! limits. Shared fixtures come from the library's `test_fixtures`
//! module (public API, so unused helpers in this crate carry no
//! dead-code lint); the crate root re-exports them together with the
//! `Chain` type so every submodule reaches the whole fixture namespace
//! through one glob import. The conflicting-reorg fixtures live in
//! `forks_conflicting_support` and reach only the module that uses them.

pub use slash::chain::Chain;
pub use slash::test_fixtures::*;

mod fees;
mod forks;
mod forks_conflicting;
mod forks_conflicting_support;
mod history;
mod limits;
mod meta;
mod multi_input;
mod staking;
mod transfers;
