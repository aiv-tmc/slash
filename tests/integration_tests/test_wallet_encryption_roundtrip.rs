//! `test_wallet_encryption_roundtrip` — one test case of the `integration_tests` suite.
//!
//! The test exercises the wallet-level encryption helpers through their
//! fully qualified library path and imports nothing from the crate root.

#[test]
fn test_wallet_encryption_roundtrip() {
    // A wallet secret encrypted with a password must decrypt to the same
    // bytes with the same password and fail with a different one. The
    // encryption yields the persisted triple: salt, nonce, ciphertext.
    let (sk, _pk) = slash::crypto::generate_keypair();
    let password = "correct horse battery staple";
    let (salt, nonce, ciphertext) = slash::crypto::encrypt_wallet(&sk, password);
    let recovered = slash::crypto::decrypt_wallet(&salt, &nonce, &ciphertext, password)
        .expect("correct password must decrypt");
    assert_eq!(sk.to_vec(), recovered);
    assert!(slash::crypto::decrypt_wallet(&salt, &nonce, &ciphertext, "wrong password").is_none());
}
