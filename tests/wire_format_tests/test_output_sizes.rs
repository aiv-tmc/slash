//! `test_output_sizes` — one test case of the `wire_format_tests` suite.
//!
//! The shared imports and helpers come from the crate root via
//! `use super::*;`; only this test's body lives here.

use super::*;

#[test]
fn test_output_sizes() {
    let locked = Output {
        start: 0,
        end: 1,
        to: [0u8; 32],
        lock: Some(100),
    };
    let unlocked = Output {
        start: 0,
        end: 1,
        to: [0u8; 32],
        lock: None,
    };
    assert_eq!(
        bincode::serialize(&locked).unwrap().len(),
        OUTPUT_LOCKED_SIZE
    );
    assert_eq!(
        bincode::serialize(&unlocked).unwrap().len(),
        OUTPUT_UNLOCKED_SIZE
    );
}
