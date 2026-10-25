//! Cryptographic primitives: Ed25519 signatures, X25519 key exchange,
//! AES-256-GCM symmetric encryption, and Argon2id password-based key
//! derivation. The module also carries the transaction signature scheme
//! dispatcher: consensus verification routes every signature through
//! [`verify_scheme`], which is fail-closed for scheme identifiers that
//! have no implementation in this build, so a soft-fork policy can never
//! admit a scheme the consensus code cannot verify.

use aes_gcm::{aead::Aead, Aes256Gcm, Key, KeyInit, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use rand::RngCore;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};
use zeroize::Zeroize;

/// Argon2id work factors for wallet key derivation: 64 MiB of memory,
/// 3 iterations, 4 parallel lanes, 32-byte output. The factors are
/// compile-time constants; `Params::new` validates them once per call and
/// cannot fail for these values.
const ARGON2_M_COST: u32 = 65_536;
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 4;

/// Transaction signature scheme identifiers. Scheme 0 is plain Ed25519;
/// higher values name upgraded schemes that the version-bit soft-fork
/// gate admits one at a time as implementations land.
pub const SCHEME_ED25519: u8 = 0;

/// Generate a new Ed25519 signing keypair.
/// Returns (secret_bytes, public_bytes). The caller should wrap the secret in Zeroizing.
pub fn generate_keypair() -> ([u8; 32], [u8; 32]) {
    let mut csprng = OsRng;
    let sk = SigningKey::generate(&mut csprng);
    (sk.to_bytes(), sk.verifying_key().to_bytes())
}

/// Derive the 32-byte Ed25519 public key from a 32-byte secret seed.
/// This makes deterministic keypairs possible, which the integration test
/// suite uses to pin a fixed treasury signer key.
pub fn public_from_secret(secret: &[u8; 32]) -> [u8; 32] {
    let sk = SigningKey::from_bytes(secret);
    sk.verifying_key().to_bytes()
}

/// Sign `msg` with the provided 32-byte Ed25519 secret key.
pub fn sign(secret: &[u8; 32], msg: &[u8]) -> Vec<u8> {
    let sk = SigningKey::from_bytes(secret);
    sk.sign(msg).to_bytes().to_vec()
}

/// Verify an Ed25519 signature.
pub fn verify(public: &[u8; 32], msg: &[u8], sig_bytes: &[u8]) -> bool {
    let vk = match VerifyingKey::from_bytes(public) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let sig = match ed25519_dalek::Signature::try_from(sig_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };
    vk.verify(msg, &sig).is_ok()
}

/// Report whether this build implements the scheme. A scheme admitted by
/// soft-fork policy still fails verification while it has no
/// implementation here, so the policy layer can never switch on a scheme
/// the consensus code cannot verify.
pub fn scheme_supported(scheme: u8) -> bool {
    scheme == SCHEME_ED25519
}

/// Verify a signature against its declared scheme. The dispatch is
/// fail-closed: only implemented schemes reach a verifier, every other
/// identifier is rejected outright rather than reinterpreted under the
/// default algorithm, so a transaction can never pass verification by
/// borrowing the cryptography of another scheme.
pub fn verify_scheme(scheme: u8, public: &[u8; 32], msg: &[u8], sig: &[u8]) -> bool {
    if scheme != SCHEME_ED25519 {
        return false;
    }
    verify(public, msg, sig)
}

/// Sign under the declared scheme. Returns None for schemes this build
/// does not implement, mirroring the fail-closed verification dispatch.
pub fn sign_scheme(scheme: u8, secret: &[u8; 32], msg: &[u8]) -> Option<Vec<u8>> {
    if scheme != SCHEME_ED25519 {
        return None;
    }
    Some(sign(secret, msg))
}

/// Generate a fresh 32-byte X25519 secret seed for onion routing.
/// The seed is persisted as raw bytes (x25519.bin) and expanded into a
/// StaticSecret whenever a shared secret is computed; returning the raw
/// bytes keeps the persistence layer independent of key type wrappers.
pub fn generate_x25519_secret() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

/// Generate a new X25519 static keypair for onion routing.
/// Returns (StaticSecret, public_bytes). The caller should wrap the secret in Zeroizing.
pub fn x25519_generate() -> (StaticSecret, [u8; 32]) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = X25519PublicKey::from(&secret);
    (secret, public.to_bytes())
}

/// Compute a shared X25519 secret from a local static secret and a remote
/// public key. Returns None when the peer's point is low order: the
/// Diffie-Hellman output is then the all-zero string, which RFC 7748
/// section 6.1 requires rejecting. Without the check every envelope layer
/// built against such a key would be encrypted under a publicly computable
/// key, and a crafted envelope carrying a low-order layer ephemeral would
/// "decrypt" under the same degenerate key at any relay.
pub fn x25519_shared(secret: &StaticSecret, public_bytes: &[u8; 32]) -> Option<[u8; 32]> {
    let public = X25519PublicKey::from(*public_bytes);
    let shared = *secret.diffie_hellman(&public).as_bytes();
    if shared.iter().all(|b| *b == 0) {
        return None;
    }
    Some(shared)
}

/// Derive a purpose-bound AES-256 key from an X25519 shared secret by
/// hashing the domain tag together with the secret. Each plaintext domain
/// — routing layer, inner transaction, final marker — receives its own
/// tag, so keys never serve two domains: a future change to nonce
/// derivation or a new domain cannot silently introduce cross-domain
/// nonce reuse under one key.
pub fn kdf_domain(shared: &[u8; 32], domain: u8) -> [u8; 32] {
    let mut input = Vec::with_capacity(33);
    input.push(domain);
    input.extend_from_slice(shared);
    *blake3::hash(&input).as_bytes()
}

/// Encrypt `plaintext` with AES-256-GCM using the provided 32-byte key.
/// Returns (ciphertext, nonce).
pub fn aes_encrypt(key: &[u8; 32], plaintext: &[u8]) -> (Vec<u8>, [u8; 12]) {
    let key = Key::<Aes256Gcm>::from_slice(key);
    let cipher = Aes256Gcm::new(key);
    let nonce = rand::random::<[u8; 12]>();
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .unwrap();
    (ciphertext, nonce)
}

/// Decrypt AES-256-GCM ciphertext.
/// Returns None if authentication fails.
pub fn aes_decrypt(key: &[u8; 32], nonce: &[u8; 12], ciphertext: &[u8]) -> Option<Vec<u8>> {
    let key = Key::<Aes256Gcm>::from_slice(key);
    let cipher = Aes256Gcm::new(key);
    cipher.decrypt(Nonce::from_slice(nonce), ciphertext).ok()
}

/// Derive a 32-byte encryption key from a user password using Argon2id
/// with the protocol's fixed work factors (64 MiB, 3 iterations, 4 lanes,
/// RFC 9106 first-recommendation class parameters). The salt must be
/// unique per encryption and stored alongside the ciphertext.
pub fn derive_key(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    let params = Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
        .expect("compile-time Argon2id parameters are valid");
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .expect("argon2id hashing with valid parameters cannot fail");
    key
}

/// Encrypt wallet data with AES-256-GCM using a key derived from the user password.
/// Returns (salt, nonce, ciphertext). The salt and nonce must be persisted with the ciphertext.
pub fn encrypt_wallet(plaintext: &[u8], password: &str) -> ([u8; 16], [u8; 12], Vec<u8>) {
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let mut key_bytes = derive_key(password, &salt);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .unwrap();
    key_bytes.zeroize();
    (salt, nonce, ciphertext)
}

/// Decrypt wallet data with AES-256-GCM using a key derived from the user password.
/// Returns None if the password is wrong or the ciphertext has been tampered with.
pub fn decrypt_wallet(
    salt: &[u8; 16],
    nonce: &[u8; 12],
    ciphertext: &[u8],
    password: &str,
) -> Option<Vec<u8>> {
    let mut key_bytes = derive_key(password, salt);
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let result = cipher.decrypt(Nonce::from_slice(nonce), ciphertext).ok();
    key_bytes.zeroize();
    result
}

/// Derive the public half of a keypair from its 32-byte secret seed.
/// The derivation is the standard Ed25519 key expansion, so the result
/// verifies signatures produced by `sign` with the same secret.
pub fn pk_from_sk(sk: &[u8; 32]) -> [u8; 32] {
    ed25519_dalek::SigningKey::from_bytes(sk)
        .verifying_key()
        .to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_dispatch_verifies_ed25519_roundtrip() {
        let (sk, pk) = generate_keypair();
        let msg = b"agility probe";
        let sig = sign_scheme(SCHEME_ED25519, &sk, msg).expect("ed25519 is implemented");
        assert!(verify_scheme(SCHEME_ED25519, &pk, msg, &sig));
        assert!(scheme_supported(SCHEME_ED25519));
    }

    #[test]
    fn scheme_dispatch_rejects_every_unimplemented_scheme() {
        // A perfectly valid Ed25519 signature must not verify under any
        // other scheme id, and no unimplemented scheme may sign: the
        // dispatch stays fail-closed across the whole identifier space.
        let (sk, pk) = generate_keypair();
        let msg = b"agility probe";
        let sig = sign(&sk, msg);
        for scheme in 1..=255u8 {
            assert!(
                !verify_scheme(scheme, &pk, msg, &sig),
                "scheme {scheme} must stay unimplemented"
            );
            assert!(!scheme_supported(scheme));
            assert!(sign_scheme(scheme, &sk, msg).is_none());
        }
    }
}
