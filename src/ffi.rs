// The entry points keep C-idiomatic signatures: one flat parameter list per
// call, pointers for strings, and no nested structs. Grouping the wallet
// bundle into a struct would help Rust callers but force every embedding
// language to replicate the layout, so the arity lint is allowed.
#![allow(clippy::too_many_arguments)]

//! src/ffi.rs — C-API entry points for embedding Slash into external
//! applications and hardware wallets.
//!
//! Every entry point validates its inputs before any work, converts
//! failures into the entry point's contract value (null pointer or
//! negative integer), and never lets a panic cross the language
//! boundary: each body runs inside a guard that converts an unwinding
//! panic into the same failure value a bad input produces, because a
//! panic unwinding out of `extern "C"` aborts the embedding process.
//! Allocation failure still aborts the process before any unwinding can
//! run; the guard contains unwinding panics — arithmetic, serialization,
//! dependency panics — which is what a host process can survive and
//! report.
//!
//! The page-proof verifier lives in a child module kept in its own file
//! so the proof pipeline stays readable next to the state engine it
//! verifies against; it is re-exported at this level so the symbol path
//! stays `slash::ffi::slash_verify_page_proof`.

use serde_json::json;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_ulonglong};

/// The page Merkle tree is at most 16 levels deep, so a proof never needs
/// more than 64 siblings; anything larger is malformed and refused before
/// any allocation work happens.
const MAX_PROOF_SIBLINGS: usize = 64;

#[path = "ffi_proof.rs"]
mod proof_impl;

pub use proof_impl::slash_verify_page_proof;

/// Run an FFI body and convert any panic into the entry point's failure
/// value. The bodies capture raw C pointers, which are not `UnwindSafe`,
/// so the closure is wrapped in `AssertUnwindSafe`; the bodies only
/// dereference pointers after a successful null check, which keeps that
/// assertion sound.
fn ffi_guard<T>(failure: T, body: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
        Ok(v) => v,
        Err(_) => failure,
    }
}

/// Initialize the library with the data directory used for all
/// subsequent file access. This function must be called exactly once
/// before any other call: the data directory is process-global and a
/// second initialization is refused. The directory must exist; it is
/// canonicalized so a relative anchor cannot drift with a later
/// working directory change. Returns 0 on success, -1 when the
/// directory does not exist, cannot be canonicalized, or the library
/// was already initialized.
///
/// # Safety
/// `datadir` must be a valid, NUL-terminated C string for the duration
/// of the call.
#[no_mangle]
pub unsafe extern "C" fn slash_init(datadir: *const c_char) -> c_int {
    ffi_guard(-1, move || {
        if datadir.is_null() {
            return -1;
        }
        let c_str = unsafe { CStr::from_ptr(datadir) };
        let path = match c_str.to_str() {
            Ok(s) => s,
            Err(_) => return -1,
        };
        let dir = std::path::Path::new(path);
        if !dir.is_dir() {
            return -1;
        }
        let canonical = match std::fs::canonicalize(dir) {
            Ok(p) => p,
            Err(_) => return -1,
        };
        match crate::storage::set_data_dir(canonical) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    })
}

/// Generate a fresh Ed25519 keypair, encrypt the secret under the
/// caller's password, and return a heap JSON string `{"public": "...",
/// "wallet": "..."}`. `generate_keypair` returns the keypair as
/// `(secret, public)`; the advertised public key is derived from the
/// same secret seed that gets encrypted, so a successful decryption
/// always reproduces a secret that derives exactly the advertised
/// public key. The wallet value is the hex encoding of a bincode triple
/// `(salt, nonce, ciphertext)`; key derivation is Argon2id (64 MiB,
/// 3 iterations, 4 lanes) and the cipher is AES-256-GCM. The secret
/// bytes are zeroized as soon as the ciphertext exists. Returns null on
/// invalid input; the caller owns the returned string.
///
/// # Safety
/// `password` must be a valid, NUL-terminated C string for the duration
/// of the call.
#[no_mangle]
pub unsafe extern "C" fn slash_generate_encrypted_keypair(password: *const c_char) -> *mut c_char {
    ffi_guard(std::ptr::null_mut(), move || {
        if password.is_null() {
            return std::ptr::null_mut();
        }
        let pwd = match unsafe { CStr::from_ptr(password) }.to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let (mut secret, public) = crate::crypto::generate_keypair();
        let (salt, nonce, ciphertext) = crate::crypto::encrypt_wallet(&secret, pwd);
        zeroize::Zeroize::zeroize(&mut secret);
        let wallet = match bincode::serialize(&(salt, nonce, ciphertext)) {
            Ok(w) => w,
            Err(_) => return std::ptr::null_mut(),
        };
        let out = json!({ "public": hex::encode(public), "wallet": hex::encode(wallet) });
        match CString::new(out.to_string()) {
            Ok(cstr) => cstr.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

/// Decrypt a wallet blob produced by
/// `slash_generate_encrypted_keypair`. The blob is the hex encoding of
/// the bincode triple `(salt, nonce, ciphertext)`. Returns the
/// hex-encoded 32-byte secret key. The caller is responsible for freeing
/// the returned string with `slash_free_string` and for securely
/// overwriting the memory once the secret is used. Returns null on
/// invalid input or wrong password.
///
/// # Safety
/// `wallet_hex` and `password` must be valid, NUL-terminated C strings
/// for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn slash_decrypt_secret(
    wallet_hex: *const c_char,
    password: *const c_char,
) -> *mut c_char {
    ffi_guard(std::ptr::null_mut(), move || {
        if wallet_hex.is_null() || password.is_null() {
            return std::ptr::null_mut();
        }
        let wallet_str = match unsafe { CStr::from_ptr(wallet_hex) }.to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let pwd = match unsafe { CStr::from_ptr(password) }.to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let blob = match hex::decode(wallet_str) {
            Ok(b) => b,
            Err(_) => return std::ptr::null_mut(),
        };
        let (salt, nonce, ciphertext): ([u8; 16], [u8; 12], Vec<u8>) =
            match bincode::deserialize(&blob) {
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
    })
}

/// Return the cell balance of the 64-character hex-encoded address.
/// Returns 0 when the pointer is null, the text is not valid UTF-8 or
/// hex, or the address is unknown.
///
/// # Safety
/// `hex_public` must be a valid, NUL-terminated C string for the duration
/// of the call.
#[no_mangle]
pub unsafe extern "C" fn slash_get_balance(hex_public: *const c_char) -> c_ulonglong {
    ffi_guard(0, move || {
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
    })
}

/// Sign a 32-byte transaction hash with the provided Ed25519 secret key.
/// `hex_secret` is the 64-character hex encoding of the 32-byte secret
/// key. `hex_payload` is the 64-character hex encoding of the 32-byte
/// blake3 hash. The payload must already be the protocol's
/// per-transaction signature hash: this primitive signs exactly the 32
/// bytes it is given in every domain the key is ever used for, so
/// signing application data directly produces a signature valid
/// everywhere the key lives. Use `Chain::tx_signature_hash` for
/// transactions. The caller must call `slash_free_string` to release the
/// returned pointer.
///
/// # Safety
/// `hex_secret` and `hex_payload` must be valid, NUL-terminated C
/// strings for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn slash_sign_transaction(
    hex_secret: *const c_char,
    hex_payload: *const c_char,
) -> *mut c_char {
    ffi_guard(std::ptr::null_mut(), move || {
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
        // The secret is only needed for the signature itself; clear it
        // before returning so no copy survives on the stack beyond this
        // call.
        zeroize::Zeroize::zeroize(&mut secret);
        let hex_sig = hex::encode(&sig);
        match CString::new(hex_sig) {
            Ok(cstr) => cstr.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    })
}

/// Free a string previously returned by any `slash_*` function. Passing
/// a null pointer is a no-op.
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
