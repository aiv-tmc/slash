//! End-to-end tests for the cell-denominated bonding-curve treasury:
//! buy/sell pairing, price arithmetic, VRF-based cell selection,
//! and treasury state persistence.
//!
//! Cargo auto-discovers this file as the `treasury_tests` target through
//! the directory layout (`tests/treasury_tests/main.rs`), so the crate
//! root lives here and the shared imports are ordinary `use` items.
//!
//! Layout:
//! - `buy` — buy-side pairing: a vault payment funds exactly one mint at
//!   the curve price; underpayment, unpaired mints, and double-funded
//!   mints are rejected
//! - `sell_pair` — sell-side pairing: a treasury return is refunded at 98%
//!   of the curve price while the vault covers it, and the curve position
//!   recovers after the sale
//! - `sell_rejected` — sell-side rejections: overpriced payouts, double
//!   payouts, payouts without a sell, and unpaired sells burning their
//!   cells
//! - `pricing` — curve arithmetic and state: monotonic pricing,
//!   zero-amount price, deterministic cell selection, and treasury state
//!   persistence

use slash::test_fixtures::*;
use slash::*;

mod buy;
mod pricing;
mod sell_pair;
mod sell_rejected;

/// Build a signed treasury mint transaction with canonical inputs selected on
/// the given pre-block state.
fn mint_tx(chain: &chain::Chain, amount: u64, to: [u8; 32]) -> chain::Tx {
    let (s, e) = chain.state.select_single(state::TREASURY, amount).unwrap();
    make_tx(
        state::TREASURY,
        &TREASURY_SK,
        &[(s, e)],
        &[state::Output {
            start: s,
            end: e,
            to,
            lock: None,
        }],
        &chain.chain_id,
    )
}

/// Build a signed treasury payout transaction paying `payout` cells from
/// TREASURY_VAULT to `to`, using the canonical vault inputs on a state clone.
fn payout_tx_on(
    state: &state::State,
    chain_id: impl AsRef<[u8]>,
    payout: u64,
    to: [u8; 32],
) -> chain::Tx {
    let (s, e) = state.select_single(state::TREASURY_VAULT, payout).unwrap();
    make_tx(
        state::TREASURY,
        &TREASURY_SK,
        &[(s, e)],
        &[state::Output {
            start: s,
            end: e,
            to,
            lock: None,
        }],
        chain_id,
    )
}
