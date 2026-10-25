//! `test_dos_spam_rejection` — one test case of the `integration_tests` suite.
//!
//! The shared helpers come from the crate root via `use super::*;`; the
//! `Chain` type is imported explicitly so this file compiles on its own.

use super::*;
use slash::chain::Chain;

#[test]
fn test_dos_spam_rejection() {
    // A flood of unsigned garbage transactions must not enter the mempool:
    // every one of them fails signature validation at admission.
    let mut chain = Chain::genesis();
    let owner = [1u8; 32];
    for i in 0..100 {
        let inputs = vec![(i * 50, i * 50 + 50)];
        let outputs = vec![state::Output {
            start: i * 50,
            end: i * 50 + 50,
            to: owner,
            lock: None,
        }];
        // Unsigned: the signature is empty, so tx() must refuse.
        assert!(
            !chain.state.tx(
                owner,
                &inputs,
                &outputs,
                &[],
                1,
                chain.chain_id.as_bytes(),
                0,
                &None
            ),
            "unsigned spam transaction {} must be rejected",
            i
        );
    }
}
