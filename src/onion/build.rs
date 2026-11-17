//! Envelope construction. Every layer is encrypted under a key derived
//! from an ephemeral-static Diffie-Hellman shared secret with the relay's
//! X25519 key, so only the intended relay can remove its layer. The
//! constructor validates the complete relay list before any envelope
//! material exists and returns typed errors for every malformed input.

use super::{
    clear_ephemeral_secret_copy, pad_payload, Layer, OnionError, OnionTx, INNER_KEY_DOMAIN,
    INNER_SIZE, LAYER_KEY_DOMAIN, LAYER_SIZE, MARKER_KEY_DOMAIN, MIN_RELAYS,
};
use zeroize::Zeroizing;

/// Wrap a transaction in a 3-hop onion envelope addressed to the given
/// relay keys. Returns a typed error when the relay list is shorter than
/// the mandatory path, when any relay key is low order, or when the
/// transaction does not fit the fixed-size inner payload.
pub fn create_onion(tx: &crate::chain::Tx, relays: &[[u8; 32]]) -> Result<OnionTx, OnionError> {
    if relays.len() < MIN_RELAYS {
        return Err(OnionError::TooFewRelays {
            provided: relays.len(),
            required: MIN_RELAYS,
        });
    }
    let tx_bytes = bincode::serialize(tx).expect("transaction serialization is infallible");
    let padded_tx = pad_payload(&tx_bytes, INNER_SIZE).ok_or(OnionError::PayloadTooLarge {
        size: tx_bytes.len(),
        limit: INNER_SIZE - 2,
    })?;

    // Relay 3 is the exit relay: the inner payload is encrypted under a
    // key bound to the exit relay's identity, and the final marker layer
    // is encrypted under a separate domain-specific key from the same
    // shared secret, so the two plaintext domains never share a key.
    let eph2_sec = crate::crypto::x25519_generate().0;
    let eph2_pub = x25519_dalek::PublicKey::from(&eph2_sec).to_bytes();
    let shared2 = crate::crypto::x25519_shared(&eph2_sec, &relays[2])
        .ok_or(OnionError::LowOrderRelayKey { index: 2 })?;
    let key2_inner = Zeroizing::new(crate::crypto::kdf_domain(&shared2, INNER_KEY_DOMAIN));
    let key2_marker = Zeroizing::new(crate::crypto::kdf_domain(&shared2, MARKER_KEY_DOMAIN));
    let (inner_ct, inner_nonce) = crate::crypto::aes_encrypt(&key2_inner, &padded_tx);
    // The all-zero next-hop address marks the exit condition for the last relay.
    let final_marker = [0u8; 32];
    let padded_final = pad_payload(&final_marker, LAYER_SIZE).expect("marker fits the layer size");
    let (layer2_ct, layer2_nonce) = crate::crypto::aes_encrypt(&key2_marker, &padded_final);
    clear_ephemeral_secret_copy(&eph2_sec);
    let layer2 = Layer {
        ephemeral: eph2_pub,
        nonce: layer2_nonce,
        ciphertext: layer2_ct,
    };

    // Relay 2 learns the exit relay's address and carries the inner
    // payload and the final layer onward.
    let eph1_sec = crate::crypto::x25519_generate().0;
    let eph1_pub = x25519_dalek::PublicKey::from(&eph1_sec).to_bytes();
    let shared1 = crate::crypto::x25519_shared(&eph1_sec, &relays[1])
        .ok_or(OnionError::LowOrderRelayKey { index: 1 })?;
    let key1 = Zeroizing::new(crate::crypto::kdf_domain(&shared1, LAYER_KEY_DOMAIN));
    let mut payload1 = relays[2].to_vec();
    payload1.extend_from_slice(&eph2_pub);
    let padded1 = pad_payload(&payload1, LAYER_SIZE).expect("routing payload fits the layer size");
    let (layer1_ct, layer1_nonce) = crate::crypto::aes_encrypt(&key1, &padded1);
    clear_ephemeral_secret_copy(&eph1_sec);
    let layer1 = Layer {
        ephemeral: eph1_pub,
        nonce: layer1_nonce,
        ciphertext: layer1_ct,
    };

    // Relay 1 is the entry relay: it learns relay 2's address and carries
    // the rest of the envelope onward.
    let eph0_sec = crate::crypto::x25519_generate().0;
    let eph0_pub = x25519_dalek::PublicKey::from(&eph0_sec).to_bytes();
    let shared0 = crate::crypto::x25519_shared(&eph0_sec, &relays[0])
        .ok_or(OnionError::LowOrderRelayKey { index: 0 })?;
    let key0 = Zeroizing::new(crate::crypto::kdf_domain(&shared0, LAYER_KEY_DOMAIN));
    let mut payload0 = relays[1].to_vec();
    payload0.extend_from_slice(&eph1_pub);
    let padded0 = pad_payload(&payload0, LAYER_SIZE).expect("routing payload fits the layer size");
    let (layer0_ct, layer0_nonce) = crate::crypto::aes_encrypt(&key0, &padded0);
    clear_ephemeral_secret_copy(&eph0_sec);
    let layer0 = Layer {
        ephemeral: eph0_pub,
        nonce: layer0_nonce,
        ciphertext: layer0_ct,
    };

    Ok(OnionTx {
        layers: vec![layer0, layer1, layer2],
        inner: inner_ct,
        inner_nonce,
        final_ephemeral: eph2_pub,
    })
}
