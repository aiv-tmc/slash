//! Encrypted treasury-secret fixtures: persist and reload a signer key
//! through the wallet encryption format inside the current working
//! directory.

use super::crypto;

/// Persist an encrypted treasury secret to a file inside the current
/// working directory. The on-disk layout is salt, nonce, ciphertext — the
/// same triple `encrypt_wallet` produces.
pub fn save_treasury_secret(sk: &[u8; 32], password: &str, path: &str) {
    let (salt, nonce, ciphertext) = crypto::encrypt_wallet(sk, password);
    let blob = [salt.as_slice(), &nonce, &ciphertext].concat();
    std::fs::write(path, blob).expect("write treasury secret");
}

/// Load and decrypt a treasury secret previously written by
/// `save_treasury_secret`. Returns `None` on a missing file, a truncated
/// layout, or a wrong password.
pub fn load_treasury_secret(password: &str, path: &str) -> Option<[u8; 32]> {
    let blob = std::fs::read(path).ok()?;
    if blob.len() < 16 + 12 + 32 {
        return None;
    }
    let salt: [u8; 16] = blob[..16].try_into().ok()?;
    let nonce: [u8; 12] = blob[16..28].try_into().ok()?;
    let plaintext = crypto::decrypt_wallet(&salt, &nonce, &blob[28..], password)?;
    plaintext.as_slice().try_into().ok()
}
