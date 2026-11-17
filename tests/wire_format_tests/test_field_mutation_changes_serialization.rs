//! `test_field_mutation_changes_serialization` — one test case of the `wire_format_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_field_mutation_changes_serialization() {
    let tx_a = Tx {
        from: [1u8; 32],
        inputs: vec![(0, 10)],
        outputs: vec![chain::Output {
            start: 0,
            end: 10,
            to: [2u8; 32],
            lock: None,
        }],
        sig: vec![],
        scheme: 0,
        memo: None,
    };
    let mut tx_b = tx_a.clone();
    tx_b.scheme = 1;
    assert_ne!(
        bincode::serialize(&tx_a).unwrap(),
        bincode::serialize(&tx_b).unwrap(),
        "changing the scheme byte must alter the wire bytes"
    );
}
