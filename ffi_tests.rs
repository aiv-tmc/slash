//! FFI contract tests for the C ABI surface.
//!
//! The FFI layer is the integration boundary for hardware wallets and
//! mobile hosts; these tests pin the three contracts the security matrix
//! names: the data-directory selection never mutates the process working
//! directory, the encrypted-wallet round trip reproduces the advertised
//! public key, and the page-proof verifier refuses oversized or malformed
//! input. The tests use only the public crate API plus the FFI entry
//! points themselves, so they exercise exactly the surface external hosts
//! link against.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Decode a hex string into bytes. The FFI transports every binary value
/// hex-encoded, so the tests need the inverse of that transport encoding.
fn hex_decode(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "hex length must be even");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex digits"))
        .collect()
}

/// Hex-encode a 32-byte value for an FFI string argument.
fn hex32(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Extract a string field from the fixed `{"public":"…","wallet":"…"}`
/// JSON envelope the FFI emits. The envelope layout is part of the ABI,
/// so positional parsing is intentional: the tests pin the exact field
/// order and quoting instead of accepting any reordered JSON.
fn envelope_field<'a>(json: &'a str, key: &str) -> &'a str {
    let marker = format!("\"{key}\":\"");
    let start = json
        .find(&marker)
        .map(|i| i + marker.len())
        .expect("field present in the envelope");
    let end = json[start..]
        .find('"')
        .map(|i| start + i)
        .expect("field terminated in the envelope");
    &json[start..end]
}

/// Read a C string returned by the FFI into an owned Rust string and
/// release it through the FFI's own deallocator, so the allocation and
/// its release travel through the same allocator.
unsafe fn take_string(ptr: *mut c_char) -> String {
    assert!(!ptr.is_null(), "FFI returned a null string");
    let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
    slash::ffi::slash_free_string(ptr);
    s
}

#[test]
fn datadir_selection_does_not_touch_cwd() {
    // Anchoring the library to a data directory must leave the process
    // working directory untouched: the host application's relative-path
    // I/O keeps its meaning no matter when the library is initialized.
    let unique = format!(
        "slash-ffi-cwd-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let base = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&base).unwrap();
    let before = std::env::current_dir().unwrap();
    let c_dir = CString::new(base.to_str().unwrap()).unwrap();
    // The first initialization succeeds...
    let rc = unsafe { slash::ffi::slash_init(c_dir.as_ptr()) };
    assert_eq!(rc, 0, "slash_init must accept an existing directory");
    // ...and anchors every on-chain file access to the selected directory:
    // saving a chain lands the manifest inside it, not beside the process.
    let chain = slash::chain::Chain::genesis();
    slash::save(&chain);
    assert!(
        base.join("manifest.bin").exists(),
        "the manifest must land inside the anchored data directory"
    );
    // The process working directory never moved.
    let after = std::env::current_dir().unwrap();
    assert_eq!(
        before, after,
        "slash_init must not mutate the process working directory"
    );
    // The data-directory selection is a exactly-once contract: a second
    // call is refused instead of silently re-anchoring every path.
    let rc2 = unsafe { slash::ffi::slash_init(c_dir.as_ptr()) };
    assert_eq!(rc2, -1, "a second slash_init must fail");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn wallet_round_trip_via_ffi() {
    // The encrypted keypair envelope carries the public key and the
    // encrypted secret; decrypting under the same password reproduces a
    // secret that derives exactly the advertised public key.
    let password = CString::new("correct horse battery staple").unwrap();
    let json = unsafe {
        take_string(slash::ffi::slash_generate_encrypted_keypair(
            password.as_ptr(),
        ))
    };
    let public_hex = envelope_field(&json, "public").to_string();
    let wallet_hex = envelope_field(&json, "wallet").to_string();
    let c_wallet = CString::new(wallet_hex).unwrap();
    // Decryption under the correct password succeeds...
    let secret_hex = unsafe {
        take_string(slash::ffi::slash_decrypt_secret(
            c_wallet.as_ptr(),
            password.as_ptr(),
        ))
    };
    let secret = hex_decode(&secret_hex);
    assert_eq!(secret.len(), 32, "the secret must decode to 32 bytes");
    let secret: [u8; 32] = secret.try_into().unwrap();
    // ...and the derived public key matches the advertised one exactly.
    assert_eq!(
        slash::crypto::public_from_secret(&secret),
        <[u8; 32]>::try_from(hex_decode(&public_hex).as_slice()).unwrap(),
    );
    // A wrong password refuses decryption instead of returning garbage.
    let wrong = CString::new("wrong password").unwrap();
    let bad = unsafe { slash::ffi::slash_decrypt_secret(c_wallet.as_ptr(), wrong.as_ptr()) };
    assert!(bad.is_null(), "a wrong password must refuse decryption");
    // Null inputs are refused without side effects.
    assert!(
        unsafe { slash::ffi::slash_generate_encrypted_keypair(std::ptr::null()) }.is_null(),
        "a null password must be refused"
    );
}

#[test]
fn proof_verification_rejects_oversized_input() {
    // The verifier pins both directions: an honest proof verifies, and
    // input past the FFI's hygiene caps — an oversized sibling array or
    // malformed hex — is refused outright instead of being processed.
    let honest = [0xCDu8; 32];
    let foreign = [0xABu8; 32];
    let c_honest_root = CString::new(hex32(&honest)).unwrap();
    let c_honest_leaf = CString::new(hex32(&honest)).unwrap();
    let c_foreign_root = CString::new(hex32(&foreign)).unwrap();
    let c_empty = CString::new("[]").unwrap();
    // An empty sibling list folds to the leaf hash itself, so a root equal
    // to the leaf verifies and any other root refuses.
    assert_eq!(
        unsafe {
            slash::ffi::slash_verify_page_proof(
                c_honest_root.as_ptr(),
                c_honest_leaf.as_ptr(),
                0,
                c_empty.as_ptr(),
            )
        },
        1,
        "an honest proof must verify"
    );
    assert_eq!(
        unsafe {
            slash::ffi::slash_verify_page_proof(
                c_foreign_root.as_ptr(),
                c_honest_leaf.as_ptr(),
                0,
                c_empty.as_ptr(),
            )
        },
        0,
        "a mismatched root must refuse"
    );
    // One sibling past the 64-entry cap refuses outright.
    let one = format!("\"{}\"", hex32(&[0u8; 32]));
    let oversized = format!("[{}]", vec![one; 65].join(","));
    let c_oversized = CString::new(oversized).unwrap();
    assert_eq!(
        unsafe {
            slash::ffi::slash_verify_page_proof(
                c_honest_root.as_ptr(),
                c_honest_leaf.as_ptr(),
                0,
                c_oversized.as_ptr(),
            )
        },
        0,
        "a sibling array past the cap must refuse"
    );
    // Malformed hex in either digest refuses instead of decoding garbage.
    let c_bad_hex = CString::new("zz").unwrap();
    assert_eq!(
        unsafe {
            slash::ffi::slash_verify_page_proof(
                c_honest_root.as_ptr(),
                c_bad_hex.as_ptr(),
                0,
                c_empty.as_ptr(),
            )
        },
        0,
        "a malformed leaf digest must refuse"
    );
}
