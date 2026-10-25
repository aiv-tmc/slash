//! `test_tx_minimal_size` — one test case of the `wire_format_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_tx_minimal_size() {
    let tx = Tx {
        from: [0u8; 32],
        inputs: vec![],
        outputs: vec![],
        sig: vec![],
        scheme: 0,
        memo: None,
    };
    let bytes = bincode::serialize(&tx).unwrap();
    assert_eq!(
        bytes.len(),
        TX_MINIMAL_SIZE,
        "transaction wire size drifted: {} bytes",
        bytes.len()
    );
}
