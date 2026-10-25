//! Lock semantics through the transfer engine: a live lock refuses the
//! covered spend at every height below its expiry and admits the same
//! transaction once the executing height reaches the expiry.

use super::*;

#[test]
fn lock_prevents_spending() {
    let mut state = State::genesis();
    let (sk, pk) = crate::crypto::generate_keypair();
    let recipient = [2u8; 32];
    for i in 0..200 {
        state.mine(i, pk, 0);
    }
    // Lock cells 0..100 until height 10, paying cell 199 as the fee.
    let outputs = vec![
        Output {
            start: 0,
            end: 100,
            to: pk,
            lock: Some(10),
        },
        Output {
            start: 100,
            end: 199,
            to: recipient,
            lock: None,
        },
        Output {
            start: 199,
            end: 200,
            to: FEE_VAULT,
            lock: None,
        },
    ];
    let hash = crate::chain::Chain::tx_signature_hash(
        &pk,
        &[(0, 200)],
        &outputs,
        crate::chain::MAINNET_ID.as_bytes(),
        0,
        &None,
    );
    let sig = crate::crypto::sign(&sk, &hash);
    assert!(state.tx(
        pk,
        &[(0, 200)],
        &outputs,
        &sig,
        0,
        crate::chain::MAINNET_ID.as_bytes(),
        0,
        &None
    ));
    // Spending the locked range at height 5 must fail.
    let outputs2 = vec![
        Output {
            start: 0,
            end: 99,
            to: recipient,
            lock: None,
        },
        Output {
            start: 99,
            end: 100,
            to: FEE_VAULT,
            lock: None,
        },
    ];
    let hash2 = crate::chain::Chain::tx_signature_hash(
        &pk,
        &[(0, 100)],
        &outputs2,
        crate::chain::MAINNET_ID.as_bytes(),
        0,
        &None,
    );
    let sig2 = crate::crypto::sign(&sk, &hash2);
    assert!(!state.tx(
        pk,
        &[(0, 100)],
        &outputs2,
        &sig2,
        5,
        crate::chain::MAINNET_ID.as_bytes(),
        0,
        &None
    ));
    // After the lock expires the spend succeeds.
    assert!(state.tx(
        pk,
        &[(0, 100)],
        &outputs2,
        &sig2,
        10,
        crate::chain::MAINNET_ID.as_bytes(),
        0,
        &None
    ));
}
