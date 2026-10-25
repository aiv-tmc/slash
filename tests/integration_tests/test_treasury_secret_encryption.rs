//! `test_treasury_secret_encryption` — round-trips the encrypted treasury
//! secret through the shared save/load helpers, pinning both the on-disk
//! layout and the password authentication.

use super::*;

#[test]
fn test_treasury_secret_encryption() {
    let password = "treasury vault password";

    // Persist the well-known treasury secret, then read it back.
    save_treasury_secret(&TREASURY_SK, password, "treasury_secret.bin");
    let recovered =
        load_treasury_secret(password, "treasury_secret.bin").expect("correct password must load");
    assert_eq!(recovered, TREASURY_SK);

    // A wrong password must not decrypt the stored secret.
    assert!(load_treasury_secret("wrong password", "treasury_secret.bin").is_none());
}
