//! A spend from DEV_FUND is authorized by the configured dev fund signer
//! key, never by the dev fund address itself: the address is a protocol
//! constant that no user key controls. Without a configured signer both
//! validation paths refuse the spend, so an all-zero configuration can
//! never move funds; after the signer is configured, only a signature
//! from that exact key is accepted. Every spend pays the mandatory fee
//! like any non-treasury transaction.

use super::*;

#[test]
fn dev_fund_spends_require_configured_signer() {
    reset_testnet();
    let mut chain = treasury_genesis();
    let (dev_sk, dev_pk) = slash::crypto::generate_keypair();

    // Seed the fund by direct mining: a treasury mint without a matching
    // buy payment is invalid by the pairing rules, so it cannot seed a
    // test that is not about pairing.
    for cell in 0..200 {
        chain.state.mine(cell, DEV_FUND, 0);
    }
    assert_eq!(chain.state.balance(DEV_FUND), 200);

    let chain_id = chain.chain_id.clone();
    let mk_spend = |range: (u64, u64), to: [u8; 32]| {
        let outputs = vec![
            Output {
                start: range.0,
                end: range.1 - 1,
                to,
                lock: None,
            },
            Output {
                start: range.1 - 1,
                end: range.1,
                to: FEE_VAULT,
                lock: None,
            },
        ];
        make_tx(DEV_FUND, &dev_sk, &[range], &outputs, &chain_id)
    };
    let spend = mk_spend((0, 100), [8u8; 32]);

    // No signer configured: mempool admission and block execution both
    // refuse, so a stale all-zero configuration never moves funds. The
    // block is injected into an empty template because the mining fixture
    // panics on templates it cannot execute.
    assert!(!chain.validate_tx(&spend));
    let mut block = mine_next_block(&chain, [7u8; 32], vec![]);
    block.txs.push(spend.clone());
    assert!(chain.apply(block).is_err());

    // Signer configured: a spend signed by that key passes both paths.
    chain.state.dev_fund_signer = dev_pk;
    assert!(chain.validate_tx(&spend));
    let block = mine_next_block(&chain, [7u8; 32], vec![spend]);
    chain
        .apply(block)
        .expect("a dev fund spend signed by the configured signer is valid");

    // A signature from any other key is refused even with a configured
    // signer, so key rotation pins authorization to the live signer.
    let (other_sk, _other_pk) = slash::crypto::generate_keypair();
    let wrong = {
        let range = (100, 200);
        let outputs = vec![
            Output {
                start: range.0,
                end: range.1 - 1,
                to: [8u8; 32],
                lock: None,
            },
            Output {
                start: range.1 - 1,
                end: range.1,
                to: FEE_VAULT,
                lock: None,
            },
        ];
        make_tx(DEV_FUND, &other_sk, &[range], &outputs, &chain_id)
    };
    assert!(!chain.validate_tx(&wrong));
    let mut block = mine_next_block(&chain, [7u8; 32], vec![]);
    block.txs.push(wrong);
    assert!(chain.apply(block).is_err());
}
