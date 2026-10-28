//! `test_genesis_chain_id` — one test case of the `integration_tests` suite.
//!
//! The test is self-sufficient: it imports the `Chain` type explicitly
//! and addresses everything else through fully qualified library paths.

use slash::chain::Chain;

#[test]
fn test_genesis_chain_id() {
    // Explicit identifiers pin both replay domains without touching the
    // process-global network flag: parallel tests in this binary reset
    // the flag at will, so any test that flips it races them. The
    // flag-independent constructor is the race-free way to pin both
    // identifiers.
    let main_chain = Chain::with_chain_id(slash::chain::MAINNET_ID.to_string());
    assert_eq!(main_chain.chain_id, slash::chain::MAINNET_ID);
    let test_chain = Chain::with_chain_id(slash::chain::TESTNET_ID.to_string());
    assert_eq!(test_chain.chain_id, slash::chain::TESTNET_ID);
    assert_ne!(main_chain.chain_id, test_chain.chain_id);

    // Whatever the process-global flag currently holds, genesis agrees
    // with it: the flag and the genesis identifier move together, so the
    // assertion holds under any interleaving with the parallel resets.
    let expect = if slash::TESTNET_MODE.load(std::sync::atomic::Ordering::SeqCst) {
        slash::chain::TESTNET_ID
    } else {
        slash::chain::MAINNET_ID
    };
    assert_eq!(Chain::genesis().chain_id, expect);
}
