//! src/ffi.rs — the C ABI for external wallets and integrations.
//!
//! Every entry point follows the same contract: null pointers are
//! rejected without side effects, malformed text is rejected without
//! panics, and every returned string is released with
//! `slash_free_string`.
//!
//! The page-proof verification entry point lives in the `proof_impl`
//! child module; it is re-exported here so the canonical
//! `slash::ffi::slash_verify_page_proof` path keeps serving FFI hosts.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_ulonglong};

#[path = "ffi_proof.rs"]
mod proof_impl;

pub use proof_impl::slash_verify_page_proof;

/// Select the data directory that every subsequent `slash_*` call
/// operates on. The directory is stored inside the library: every
/// on-chain file (`manifest.bin`, page files, block storage, key files)
/// is resolved against it by the storage layer, and the process working
/// directory is never touched — an embedding application's own
/// relative-path I/O keeps its original meaning no matter when the
/// library is initialized. The call must happen exactly once, before any
/// other `slash_*` function; a second call is refused. Returns 0 on
/// success and -1 when the pointer is null, the path is not valid UTF-8,
/// the directory does not exist, or a data directory is already set.
///
/// # Safety
/// `datadir` must be a valid, NUL-terminated C string for the duration of
/// the call.
#[no_mangle]
pub unsafe extern "C" fn slash_init(datadir: *const c_char) -> c_int {
    if datadir.is_null() {
        return -1;
    }
    let text = match unsafe { CStr::from_ptr(datadir) }.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let path = std::path::Path::new(text);
    if !path.is_dir() {
        return -1;
    }
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    match crate::storage::set_data_dir(canonical) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// The maximum number of Merkle siblings one page proof may carry. A page
/// holds at most PAGE_SIZE cells, so its tree depth never exceeds
/// 16 levels; the cap leaves wide margin for future formats while still
/// bounding the work one FFI call can be forced to perform.
const MAX_PROOF_SIBLINGS: usize = 64;

/// Generate a new Ed25519 keypair, encrypt the secret with the caller's
/// password, and return a JSON object holding both halves:
/// `{"public":"<hex>","wallet":"<hex>"}`. The public key is the spendable
/// address; the wallet field is the encrypted secret in the same
/// Argon2id + AES-256-GCM format `crypto::encrypt_wallet` produces
/// (16-byte salt, 12-byte nonce, ciphertext), hex-encoded for transport.
/// The caller frees the pointer with `slash_free_string` and keeps the
/// wallet field as the only copy of the secret.
///
/// # Safety
/// `password` must be a valid, NUL-terminated C string for the duration
/// of the call.
#[no_mangle]
pub unsafe extern "C" fn slash_generate_encrypted_keypair(password: *const c_char) -> *mut c_char {
    if password.is_null() {
        return std::ptr::null_mut();
    }
    let pwd = match unsafe { CStr::from_ptr(password) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let (sk, pk) = crate::crypto::generate_keypair();
    let (salt, nonce, ciphertext) = crate::crypto::encrypt_wallet(&sk, pwd);
    // The secret is only needed until the encrypted blob exists; clear
    // every copy before any serialization work can observe it.
    let mut sk_copy = sk;
    zeroize::Zeroize::zeroize(&mut sk_copy);
    let blob = bincode::serialize(&(salt, nonce, ciphertext)).unwrap_or_default();
    let json = format!(
        "{{\"public\":\"{}\",\"wallet\":\"{}\"}}",
        hex::encode(pk),
        hex::encode(blob)
    );
    match CString::new(json) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Decrypt a wallet blob produced by `slash_generate_encrypted_keypair`
/// and return the hex-encoded 32-byte Ed25519 secret. The caller owns the
/// returned memory: release it with `slash_free_string` and overwrite the
/// bytes in the caller's own buffer as soon as signing completes, because
/// a hex string is not a zeroizing container. Returns null on a null
/// pointer, malformed input, a wrong password, or tampered ciphertext.
///
/// # Safety
/// `wallet_hex` and `password` must be valid, NUL-terminated C strings
/// for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn slash_decrypt_secret(
    wallet_hex: *const c_char,
    password: *const c_char,
) -> *mut c_char {
    if wallet_hex.is_null() || password.is_null() {
        return std::ptr::null_mut();
    }
    let blob_hex = match unsafe { CStr::from_ptr(wallet_hex) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let pwd = match unsafe { CStr::from_ptr(password) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let blob = match hex::decode(blob_hex) {
        Ok(b) => b,
        Err(_) => return std::ptr::null_mut(),
    };
    let (salt, nonce, ciphertext): ([u8; 16], [u8; 12], Vec<u8>) = match bincode::deserialize(&blob)
    {
        Ok(v) => v,
        Err(_) => return std::ptr::null_mut(),
    };
    let secret = match crate::crypto::decrypt_wallet(&salt, &nonce, &ciphertext, pwd) {
        Some(s) => s,
        None => return std::ptr::null_mut(),
    };
    let hex_secret = hex::encode(secret);
    match CString::new(hex_secret) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Return the cell balance of the 64-character hex-encoded address.
/// Returns 0 when the pointer is null, the text is not valid UTF-8 or hex,
/// or the address is unknown.
///
/// # Safety
/// `hex_public` must be a valid, NUL-terminated C string for the duration
/// of the call.
#[no_mangle]
pub unsafe extern "C" fn slash_get_balance(hex_public: *const c_char) -> c_ulonglong {
    if hex_public.is_null() {
        return 0;
    }
    let c_str = unsafe { CStr::from_ptr(hex_public) };
    let hex = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let addr = match crate::decode_key(hex) {
        Ok(a) => a,
        Err(_) => return 0,
    };
    let chain = crate::cached_load();
    chain.state.balance(addr)
}

/// Sign a 32-byte transaction hash with the provided Ed25519 secret key.
/// `hex_secret` is the 64-character hex encoding of the 32-byte secret key.
/// `hex_payload` is the 64-character hex encoding of the 32-byte blake3 hash.
/// The payload must already be the protocol's per-transaction signature
/// hash: this primitive signs exactly the 32 bytes it is given in every
/// domain the key is ever used for, so signing application data directly
/// produces a signature valid everywhere the key lives. Use
/// `Chain::tx_signature_hash` for transactions.
/// The caller must call `slash_free_string` to release the returned pointer.
///
/// # Safety
/// `hex_secret` and `hex_payload` must be valid, NUL-terminated C strings
/// for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn slash_sign_transaction(
    hex_secret: *const c_char,
    hex_payload: *const c_char,
) -> *mut c_char {
    if hex_secret.is_null() || hex_payload.is_null() {
        return std::ptr::null_mut();
    }
    let secret_str = match unsafe { CStr::from_ptr(hex_secret) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let payload_str = match unsafe { CStr::from_ptr(hex_payload) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let mut secret = match crate::decode_key(secret_str) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let payload = match crate::decode_key(payload_str) {
        Ok(p) => p,
        Err(_) => {
            zeroize::Zeroize::zeroize(&mut secret);
            return std::ptr::null_mut();
        }
    };

    let sig = crate::crypto::sign(&secret, &payload);
    // The secret is only needed for the signature itself; clear it before
    // returning so no copy survives on the stack beyond this call.
    zeroize::Zeroize::zeroize(&mut secret);
    let hex_sig = hex::encode(&sig);
    match CString::new(hex_sig) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a string previously returned by any `slash_*` function.
/// Passing a null pointer is a no-op.
///
/// # Safety
/// `s` must be either NULL or a pointer previously returned by
/// `slash_generate_encrypted_keypair`, `slash_decrypt_secret`, or
/// `slash_sign_transaction` and not yet freed; after this call the
/// pointer must not be used again.
#[no_mangle]
pub unsafe extern "C" fn slash_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

/// Helper to decode a hex string passed from C into a fixed 32-byte
/// array. Shared by the proof entry point in the child module and any
/// future fixed-width hex argument.
pub(crate) fn decode_hex_ptr(ptr: *const c_char) -> anyhow::Result<[u8; 32]> {
    let c_str = unsafe { CStr::from_ptr(ptr) };
    let hex = c_str.to_str()?;
    crate::decode_key(hex)
}
