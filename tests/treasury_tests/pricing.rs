//! Curve arithmetic and state: monotonic pricing, zero-amount price,
//! and treasury state persistence.

use super::*;

#[test]
fn test_treasury_curve_monotonic_in_cells() {
    // buy_price is strictly increasing in amount at every slope regime.
    for sold in [0u64, treasury::K, 2 * treasury::K] {
        for a in 1..1000u64 {
            assert!(treasury::buy_price(a + 1, sold) > treasury::buy_price(a, sold));
        }
    }
    // buy_price never decreases as total_sold grows.
    for s in 0..1000u64 {
        assert!(treasury::buy_price(10, s + 1) >= treasury::buy_price(10, s));
    }
    // The sell refund is exactly 98 percent of the buy price.
    let buy = treasury::buy_price(100, 1000);
    let sell = treasury::sell_price(100, 1000);
    assert!(sell < buy);
    assert_eq!(sell, (buy as u128 * 98 / 100) as u64);
}

#[test]
fn test_treasury_buy_price_zero_amount() {
    assert_eq!(treasury::buy_price(0, 100), 0);
    assert_eq!(treasury::sell_price(0, 100), 0);
}

#[test]
fn test_treasury_state_save_and_load() {
    let _tmp = setup_test_dir("treasury_persist");
    reset_testnet();

    let ts = treasury::TreasuryState {
        reserve_fiat: 5000,
        total_sold: 100,
        total_bought_back: 10,
        signer_public: [2u8; 32],
        onion_public: [3u8; 32],
    };
    ts.save();

    let loaded = treasury::TreasuryState::load().unwrap();
    assert_eq!(loaded.reserve_fiat, 5000);
    assert_eq!(loaded.total_sold, 100);
    assert_eq!(loaded.total_bought_back, 10);
    assert_eq!(loaded.signer_public, [2u8; 32]);
    assert_eq!(loaded.onion_public, [3u8; 32]);
}
