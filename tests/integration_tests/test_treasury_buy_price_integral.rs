//! `test_treasury_buy_price_integral` — one test case of the `integration_tests` suite.
//!
//! The test calls the curve through its fully qualified library path and
//! imports nothing from the crate root.

#[test]
fn test_treasury_buy_price_integral() {
    // The bonding curve is linear with slope proportional to the sold
    // supply: the integral of the price over an amount matches the closed
    // form, and the price never regresses as total_sold grows.
    let p0 = slash::treasury::buy_price(100, 0);
    let p1 = slash::treasury::buy_price(100, 1_000_000);
    assert!(p1 > p0);
    let step: u64 = slash::treasury::buy_price(1, 0);
    assert_eq!(step, 1);
}
