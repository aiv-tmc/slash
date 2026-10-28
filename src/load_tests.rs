//! The enforcement unit tests for the startup recovery path. The module
//! is wired into `load.rs` through a path attribute, so it reads the
//! private gates — the chain-identifier enforcement and the
//! deployment-table validation — exactly as an inline module would, while
//! the recovery path itself stays focused on load logic.

use super::*;

/// The built-in deployment table, used as the baseline the
/// enforcement tests mutate.
fn built_in_table() -> Vec<chain::Deployment> {
    chain::VersionBits::default().deployments
}

#[test]
fn enforce_chain_id_panics_on_mismatch() {
    let result = std::panic::catch_unwind(|| {
        enforce_chain_id(b"/slash/other", chain::MAINNET_ID.as_bytes());
    });
    assert!(result.is_err());
}

#[test]
fn enforce_chain_id_accepts_empty_persisted() {
    enforce_chain_id(b"", chain::MAINNET_ID.as_bytes());
}

#[test]
fn enforce_chain_id_accepts_matching() {
    enforce_chain_id(chain::MAINNET_ID.as_bytes(), chain::MAINNET_ID.as_bytes());
}

#[test]
fn deployment_table_enforcement_rejects_bad_bit() {
    let mut table = built_in_table();
    table[0].bit = 32;
    let result = std::panic::catch_unwind(|| enforce_deployment_table(&table));
    assert!(result.is_err());
}

#[test]
fn deployment_table_enforcement_rejects_bad_threshold() {
    let mut table = built_in_table();
    table[0].threshold = chain::DIFFICULTY_ADJUSTMENT_INTERVAL as u32 + 1;
    let result = std::panic::catch_unwind(|| enforce_deployment_table(&table));
    assert!(result.is_err());
}

#[test]
fn deployment_table_enforcement_accepts_built_in() {
    enforce_deployment_table(&built_in_table());
}
