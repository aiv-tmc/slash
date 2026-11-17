//! The page-proof verification entry point of the C ABI.
//!
//! The call returns a boolean, so every input is bounded before any work
//! happens: the sibling array is capped before decoding, the leaf index
//! must fit the platform's usize, and every malformed string is refused
//! with a zero. The body runs inside the same panic guard as the other
//! entry points: a panic here converts to a plain "proof invalid" verdict
//! instead of unwinding across the language boundary and aborting the
//! embedding process. Verification itself is the same fold the
//! light-client guide documents.

use super::{decode_hex_ptr, ffi_guard, MAX_PROOF_SIBLINGS};
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_ulonglong};

/// Verify a page proof against a claimed Merkle root.
/// `hex_root` is the 64-character hex encoding of the 32-byte expected root.
/// `hex_leaf_hash` is the 64-character hex encoding of the 32-byte leaf hash.
/// `siblings_json` is a JSON array of at most 64 64-character hex strings;
/// longer arrays are refused outright so a hostile host cannot push an
/// unbounded allocation into a boolean-returning call. `leaf_index` must
/// fit in `usize` on this platform; larger values are refused, not
/// truncated. Returns 1 when the proof is valid and 0 otherwise.
///
/// # Safety
/// `hex_root`, `hex_leaf_hash`, and `siblings_json` must be valid,
/// NUL-terminated C strings for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn slash_verify_page_proof(
    hex_root: *const c_char,
    hex_leaf_hash: *const c_char,
    leaf_index: c_ulonglong,
    siblings_json: *const c_char,
) -> c_int {
    ffi_guard(0, move || {
        if hex_root.is_null() || hex_leaf_hash.is_null() || siblings_json.is_null() {
            return 0;
        }
        let Ok(leaf_index) = usize::try_from(leaf_index) else {
            return 0;
        };
        let root = match decode_hex_ptr(hex_root) {
            Ok(r) => r,
            Err(_) => return 0,
        };
        let leaf_hash = match decode_hex_ptr(hex_leaf_hash) {
            Ok(l) => l,
            Err(_) => return 0,
        };
        let siblings_str = match unsafe { CStr::from_ptr(siblings_json) }.to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let siblings_vec: Vec<String> = match serde_json::from_str(siblings_str) {
            Ok(v) => v,
            Err(_) => return 0,
        };
        // Bound the work before decoding: the page tree is at most 16
        // levels deep, so anything past the cap is malformed on any
        // honest chain.
        if siblings_vec.len() > MAX_PROOF_SIBLINGS {
            return 0;
        }
        let mut siblings = Vec::with_capacity(siblings_vec.len());
        for s in siblings_vec {
            match crate::decode_key(&s) {
                Ok(h) => siblings.push(h),
                Err(_) => return 0,
            }
        }

        let proof = crate::state::PageProof {
            leaf_hash,
            siblings,
            leaf_index,
        };

        if crate::state::verify_page_proof(&root, &proof) {
            1
        } else {
            0
        }
    })
}
