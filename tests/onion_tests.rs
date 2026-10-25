//! End-to-end tests for the 3-hop onion routing envelope:
//! creation, layered decryption, constant wire size, construction errors,
//! low-order curve-point rejection, and failure modes.

use slash::test_fixtures::*;
use slash::*;

/// A minimal transaction used as the onion payload across these tests.
fn sample_payload_tx() -> chain::Tx {
    chain::Tx {
        from: [1u8; 32],
        inputs: vec![(0, 1)],
        outputs: vec![chain::Output {
            start: 0,
            end: 1,
            to: [2u8; 32],
            lock: None,
        }],
        sig: vec![0u8; 64],
        scheme: 0,
        memo: None,
    }
}

#[test]
fn test_onion_create_and_peel() {
    let _tmp = setup_test_dir("onion_peel");
    reset_testnet();

    let (r1_sec, r1_pub) = crypto::x25519_generate();
    let (r2_sec, r2_pub) = crypto::x25519_generate();
    let (r3_sec, r3_pub) = crypto::x25519_generate();

    let onion = onion::create_onion(&sample_payload_tx(), &[r1_pub, r2_pub, r3_pub]).unwrap();
    assert_eq!(onion.layers.len(), 3);

    let peel1 = onion::peel(&onion, &r1_sec.to_bytes()).unwrap();
    assert_eq!(peel1.next_relay, r2_pub);
    let peel2 = onion::peel(&peel1.remaining, &r2_sec.to_bytes()).unwrap();
    assert_eq!(peel2.next_relay, r3_pub);
    let peel3 = onion::peel(&peel2.remaining, &r3_sec.to_bytes()).unwrap();
    assert_eq!(peel3.next_relay, [0u8; 32]);
}

#[test]
fn test_onion_create_rejects_too_few_relays() {
    let _tmp = setup_test_dir("onion_few_relays");
    reset_testnet();

    let (_r1_sec, r1_pub) = crypto::x25519_generate();
    let (_r2_sec, r2_pub) = crypto::x25519_generate();

    // Construction is a fallible operation: two relays cannot form the
    // mandatory three-hop path, and the error carries enough context for a
    // wallet UI to explain the failure.
    let err = onion::create_onion(&sample_payload_tx(), &[r1_pub, r2_pub]).unwrap_err();
    assert_eq!(
        err,
        onion::OnionError::TooFewRelays {
            provided: 2,
            required: onion::MIN_RELAYS
        }
    );
}

#[test]
fn test_onion_create_rejects_low_order_relay_key() {
    let _tmp = setup_test_dir("onion_low_order_relay");
    reset_testnet();

    let (_r1_sec, r1_pub) = crypto::x25519_generate();
    let (_r2_sec, r2_pub) = crypto::x25519_generate();

    // A relay advertising the all-zero X25519 point forces every
    // Diffie-Hellman output to the all-zero string; the envelope would be
    // readable by anyone. Construction must refuse with a typed error
    // naming the offending relay.
    let err = onion::create_onion(&sample_payload_tx(), &[r1_pub, r2_pub, [0u8; 32]]).unwrap_err();
    assert_eq!(err, onion::OnionError::LowOrderRelayKey { index: 2 });
}

#[test]
fn test_onion_decrypt_inner() {
    let _tmp = setup_test_dir("onion_inner");
    reset_testnet();

    let (r1_sec, r1_pub) = crypto::x25519_generate();
    let (r2_sec, r2_pub) = crypto::x25519_generate();
    let (r3_sec, r3_pub) = crypto::x25519_generate();

    let tx = sample_payload_tx();

    let onion = onion::create_onion(&tx, &[r1_pub, r2_pub, r3_pub]).unwrap();

    let peel1 = onion::peel(&onion, &r1_sec.to_bytes()).unwrap();
    let peel2 = onion::peel(&peel1.remaining, &r2_sec.to_bytes()).unwrap();
    let peel3 = onion::peel(&peel2.remaining, &r3_sec.to_bytes()).unwrap();
    let decrypted = onion::decrypt_inner(&peel3.remaining, &r3_sec.to_bytes()).unwrap();

    assert_eq!(decrypted.from, tx.from);
    assert_eq!(decrypted.inputs, tx.inputs);
}

#[test]
fn test_onion_peel_rejects_low_order_layer_ephemeral() {
    let _tmp = setup_test_dir("onion_low_order_ephemeral");
    reset_testnet();

    let (r1_sec, r1_pub) = crypto::x25519_generate();
    let (_r2_sec, r2_pub) = crypto::x25519_generate();
    let (_r3_sec, r3_pub) = crypto::x25519_generate();

    // An attacker-crafted envelope can put a low-order ephemeral into any
    // layer. Peeling must refuse the envelope instead of decrypting under
    // the publicly known degenerate key, which would otherwise let the
    // attacker route chosen payloads through honest relays.
    let mut onion = onion::create_onion(&sample_payload_tx(), &[r1_pub, r2_pub, r3_pub]).unwrap();
    onion.layers[0].ephemeral = [0u8; 32];

    assert!(onion::peel(&onion, &r1_sec.to_bytes()).is_none());
}

#[test]
fn test_onion_constant_size() {
    let _tmp = setup_test_dir("onion_size");
    reset_testnet();

    let (_r1_sec, r1_pub) = crypto::x25519_generate();
    let (_r2_sec, r2_pub) = crypto::x25519_generate();
    let (_r3_sec, r3_pub) = crypto::x25519_generate();

    let onion = onion::create_onion(&sample_payload_tx(), &[r1_pub, r2_pub, r3_pub]).unwrap();
    for layer in &onion.layers {
        assert_eq!(layer.ciphertext.len(), 1024);
    }
    assert_eq!(onion.inner.len(), 2048);
}

#[test]
fn test_onion_wrong_relay_fails() {
    let _tmp = setup_test_dir("onion_wrong");
    reset_testnet();

    let (_r1_sec, r1_pub) = crypto::x25519_generate();
    let (r2_sec, r2_pub) = crypto::x25519_generate();

    let onion = onion::create_onion(&sample_payload_tx(), &[r1_pub, r2_pub, r1_pub]).unwrap();
    // Attempt to peel with the second relay's secret instead of the first.
    // The AES-GCM authentication will fail because the key is wrong.
    let result = onion::peel(&onion, &r2_sec.to_bytes());
    assert!(result.is_none());
}

#[test]
fn test_onion_peel_empty_fails() {
    let _tmp = setup_test_dir("onion_empty");
    reset_testnet();

    let empty = onion::OnionTx {
        layers: vec![],
        inner: vec![],
        inner_nonce: [0u8; 12],
        final_ephemeral: [0u8; 32],
    };
    // Peeling requires at least one layer to derive a shared secret.
    let result = onion::peel(&empty, &[0u8; 32]);
    assert!(result.is_none());
}

#[test]
fn test_onion_serialization_roundtrip() {
    let _tmp = setup_test_dir("onion_roundtrip");
    reset_testnet();

    let (_r1_sec, r1_pub) = crypto::x25519_generate();
    let (_r2_sec, r2_pub) = crypto::x25519_generate();
    let (_r3_sec, r3_pub) = crypto::x25519_generate();

    let original = onion::create_onion(&sample_payload_tx(), &[r1_pub, r2_pub, r3_pub]).unwrap();
    let bytes = bincode::serialize(&original).unwrap();
    let restored: onion::OnionTx = bincode::deserialize(&bytes).unwrap();

    assert_eq!(restored.layers.len(), original.layers.len());
    assert_eq!(restored.inner.len(), original.inner.len());
    assert_eq!(restored.final_ephemeral, original.final_ephemeral);
}
