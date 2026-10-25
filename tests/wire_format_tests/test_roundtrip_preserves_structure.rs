//! `test_roundtrip_preserves_structure` — one test case of the `wire_format_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_roundtrip_preserves_structure() {
    let tx = Tx {
        from: [1u8; 32],
        inputs: vec![(10, 20), (30, 40)],
        outputs: vec![
            Output {
                start: 10,
                end: 25,
                to: [2u8; 32],
                lock: None,
            },
            Output {
                start: 25,
                end: 40,
                to: [1u8; 32],
                lock: Some(1000),
            },
        ],
        sig: vec![0xAB; 64],
        scheme: 0,
        memo: None,
    };
    let bytes = bincode::serialize(&tx).unwrap();
    let decoded: Tx = bincode::deserialize(&bytes).unwrap();
    assert_eq!(tx.from, decoded.from);
    assert_eq!(tx.inputs, decoded.inputs);
    assert_eq!(tx.outputs, decoded.outputs);
    assert_eq!(tx.sig, decoded.sig);
    assert_eq!(tx.scheme, decoded.scheme);
}
