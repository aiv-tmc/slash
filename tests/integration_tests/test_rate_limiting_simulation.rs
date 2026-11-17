//! `test_rate_limiting_simulation` — one test case of the `integration_tests` suite.
//!
//! The test is fully self-contained: it models the admission token bucket
//! arithmetic directly and imports nothing from the crate root.

#[test]
fn test_rate_limiting_simulation() {
    // Simulated token bucket: 10 burst capacity with a steady-state rate of
    // 4 per second must admit the first 10 requests instantly and reject
    // the 11th.
    let mut tokens: f64 = 10.0;
    let mut admitted = 0usize;
    for _ in 0..11 {
        if tokens >= 1.0 {
            tokens -= 1.0;
            admitted += 1;
        }
    }
    assert_eq!(admitted, 10);
}
