//! The rule requires every user transaction to carry an output to DEV_FUND
//! of at least max(total_out * dev_fee_bps / 10_000, 1) cells while the
//! `dev_fund` deployment is active. Treasury transactions, treasury pairing
//! payments, and dev fund spends are exempt; before activation nothing is
//! required. Mempool admission (Chain::validate_tx) and block execution
//! (Chain::apply) must agree in both states.

use slash::chain::{Chain, DeploymentStatus};
use slash::state::{Output, DEV_FUND, FEE_VAULT, TREASURY};
use slash::test_fixtures::*;

/// Activate the dev_fund deployment immediately: the deployment vector is
/// public consensus state, so tests pin the active rule by flipping the
/// deployment to Active with an activation height in the past.
fn activate_dev_fund(chain: &mut Chain) {
    let d = chain
        .version_bits
        .deployments
        .iter_mut()
        .find(|d| d.name == "dev_fund")
        .expect("dev_fund deployment exists");
    d.status = DeploymentStatus::Active;
    d.activation_height = Some(0);
    chain.state_mut().dev_fee_bps = 1;
}

/// Fund an address with `cells` contiguous cells via the test backfill.
fn fund(chain: &mut Chain, owner: [u8; 32], cells: u64) -> (u64, u64) {
    for i in 0..cells {
        chain.state_mut().mine(i, owner, 0);
    }
    (0, cells)
}

/// The exact dev-fund requirement for a transfer of `total_out` cells.
fn required(total_out: u64, bps: u64) -> u64 {
    std::cmp::max(total_out.saturating_mul(bps) / 10_000, 1)
}

// Test modules: every `#[test]` function of this crate lives
// in its own small file under this directory for readability.
mod dev_fee_enforced_by_block_execution_when_active;
mod dev_fee_not_required_before_activation;
mod dev_fee_required_by_mempool_when_active;
mod dev_fee_rounding_pins_minimum_one_cell;
mod dev_fund_spends_capped_at_one_per_block;
mod dev_fund_spends_require_configured_signer;
mod treasury_and_pairing_paths_exempt_from_dev_fee;
