//! End-to-end tests for cryptographic primitives:
//! Ed25519 signatures, X25519 key exchange (including the RFC 7748
//! low-order rejection), AES-GCM encryption, and wallet encryption.

use slash::*;

#[test]
fn test_ed25519_sign_and_verify() {
    let (sk, pk) = crypto::generate_keypair();
    let msg = b"hello world";
    let sig = crypto::sign(&sk, msg);
    assert!(crypto::verify(&pk, msg, &sig));
}

#[test]
fn test_ed25519_rejects_tampered_signature() {
    let (sk, pk) = crypto::generate_keypair();
    let msg = b"hello world";
    let mut sig = crypto::sign(&sk, msg);
    sig[0] ^= 0xFF;
    assert!(!crypto::verify(&pk, msg, &sig));
}

#[test]
fn test_x25519_shared_secret_symmetric() {
    let (a_sec, a_pub) = crypto::x25519_generate();
    let (b_sec, b_pub) = crypto::x25519_generate();

    // Honest keys never produce the all-zero Diffie-Hellman output, so
    // the checked shared-secret API always yields a value for them.
    let shared_a = crypto::x25519_shared(&a_sec, &b_pub)
        .expect("honest X25519 keys never produce the all-zero output");
    let shared_b = crypto::x25519_shared(&b_sec, &a_pub)
        .expect("honest X25519 keys never produce the all-zero output");

    assert_eq!(shared_a, shared_b);
}

#[test]
fn test_x25519_shared_rejects_low_order_public_key() {
    // RFC 7748 section 6.1: Diffie-Hellman with a low-order peer point
    // yields the all-zero output and must be rejected, because the
    // "shared" secret is then publicly computable. The all-zero encoded
    // point is the canonical low-order test input.
    let (a_sec, _a_pub) = crypto::x25519_generate();
    let low_order = [0u8; 32];
    assert!(crypto::x25519_shared(&a_sec, &low_order).is_none());
}

#[test]
fn test_aes_encrypt_decrypt_roundtrip() {
    let key = [42u8; 32];
    let plaintext = b"secret message";
    let (ciphertext, nonce) = crypto::aes_encrypt(&key, plaintext);
    let decrypted = crypto::aes_decrypt(&key, &nonce, &ciphertext).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_aes_decrypt_rejects_tampered_ciphertext() {
    let key = [42u8; 32];
    let plaintext = b"secret message";
    let (mut ciphertext, nonce) = crypto::aes_encrypt(&key, plaintext);
    ciphertext[0] ^= 0xFF;
    assert!(crypto::aes_decrypt(&key, &nonce, &ciphertext).is_none());
}

#[test]
fn test_wallet_encryption_roundtrip() {
    let password = "super_secret_password";
    let plaintext = b"wallet data here";

    let (salt, nonce, ciphertext) = crypto::encrypt_wallet(plaintext, password);
    let decrypted = crypto::decrypt_wallet(&salt, &nonce, &ciphertext, password).unwrap();

    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_wallet_encryption_rejects_wrong_password() {
    let password = "correct_horse_battery_staple";
    let plaintext = b"wallet data";

    let (salt, nonce, ciphertext) = crypto::encrypt_wallet(plaintext, password);
    assert!(crypto::decrypt_wallet(&salt, &nonce, &ciphertext, "wrong").is_none());
}

#[test]
fn test_key_derivation_is_deterministic() {
    let password = "my_password";
    let salt = [1u8; 16];

    let k1 = crypto::derive_key(password, &salt);
    let k2 = crypto::derive_key(password, &salt);

    assert_eq!(k1, k2);
}

#[test]
fn test_key_derivation_differs_with_different_salts() {
    let password = "my_password";
    let salt1 = [1u8; 16];
    let salt2 = [2u8; 16];

    let k1 = crypto::derive_key(password, &salt1);
    let k2 = crypto::derive_key(password, &salt2);

    assert_ne!(k1, k2);
}
