//! `test_genesis_chain_id` — one test case of the `integration_tests` suite.
//!
//! The test is self-sufficient: it imports the `Chain` type explicitly
//! and addresses everything else through fully qualified library paths.

use slash::chain::Chain;

#[test]
fn test_genesis_chain_id() {
    // Mainnet genesis carries the production identifier.
    let main_chain = Chain::genesis();
    assert_eq!(main_chain.chain_id, slash::chain::MAINNET_ID);

    // Flipping the process-wide network flag switches the genesis
    // identifier to the testnet domain; signing domains stay disjoint.
    slash::TESTNET_MODE.store(true, std::sync::atomic::Ordering::SeqCst);
    let test_chain = Chain::genesis();
    assert_eq!(test_chain.chain_id, slash::chain::TESTNET_ID);
    assert_ne!(main_chain.chain_id, test_chain.chain_id);

    // Restore mainnet for every other test in this binary.
    slash::TESTNET_MODE.store(false, std::sync::atomic::Ordering::SeqCst);
}
