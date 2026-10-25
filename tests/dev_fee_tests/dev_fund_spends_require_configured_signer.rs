//! A spend from DEV_FUND is authorized by the configured dev fund signer
//! key, never by the dev fund address itself: the address is a protocol
//! constant that no user key controls. Without a configured signer both
//! validation paths refuse the spend, so an all-zero configuration can
//! never move funds; after the signer is configured, only a signature
//! from that exact key is accepted.

use super::*;

#[test]
fn dev_fund_spends_require_configured_signer() {
    reset_testnet();
    let mut chain = treasury_genesis();
    let (dev_sk, dev_pk) = slash::crypto::generate_keypair();

    // Seed the dev fund with a treasury mint so a spend has inputs.
    let (s, e) = chain.state.select_single(TREASURY, 200).unwrap();
    let seed_outputs = vec![Output {
        start: s,
        end: e,
        to: DEV_FUND,
        lock: None,
    }];
    let seed_tx = make_tx(
        TREASURY,
        &TREASURY_SK,
        &[(s, e)],
        &seed_outputs,
        &chain.chain_id,
    );
    let block = mine_next_block(&chain, [7u8; 32], vec![seed_tx]);
    chain.apply(block).unwrap();
    assert_eq!(chain.state.balance(DEV_FUND), 200);

    let chain_id = chain.chain_id.clone();
    let outputs = vec![Output {
        start: s,
        end: s + 50,
        to: [8u8; 32],
        lock: None,
    }];
    let spend = make_tx(DEV_FUND, &dev_sk, &[(s, s + 50)], &outputs, &chain_id);

    // No signer configured: mempool admission and block execution both
    // refuse, so a stale all-zero configuration never moves funds.
    assert!(!chain.validate_tx(&spend));
    let block = mine_next_block(&chain, [7u8; 32], vec![spend.clone()]);
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
    let wrong_outputs = vec![Output {
        start: s + 50,
        end: s + 100,
        to: [8u8; 32],
        lock: None,
    }];
    let wrong = make_tx(
        DEV_FUND,
        &other_sk,
        &[(s + 50, s + 100)],
        &wrong_outputs,
        &chain_id,
    );
    assert!(!chain.validate_tx(&wrong));
    let block = mine_next_block(&chain, [7u8; 32], vec![wrong]);
    assert!(chain.apply(block).is_err());
}
