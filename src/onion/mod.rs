//! src/onion/mod.rs — 3-hop onion routing envelope construction and
//! peeling.
//!
//! Every envelope carries fixed-size padded layers so traffic analysis
//! cannot distinguish hop count or payload size from the outer envelope,
//! and each layer is bound to the relay's X25519 identity through an
//! ephemeral-static Diffie-Hellman shared secret.
//!
//! Layout:
//! - `build` — envelope construction (`create_onion`)
//! - `peel` — layer removal and inner decryption at the exit relay
//! - `tests` — construction and peeling regressions (test-only)

mod build;
mod peel;
#[cfg(test)]
mod tests;

pub use build::create_onion;
pub use peel::{decrypt_inner, peel};

use serde::{Deserialize, Serialize};

/// Fixed ciphertext size for every onion layer after encryption.
/// All layer ciphertexts are padded to this length so traffic analysis
/// cannot distinguish hop count or payload size from the outer envelope.
const LAYER_SIZE: usize = 1024;
/// Fixed ciphertext size for the inner transaction payload.
const INNER_SIZE: usize = 2048;
/// Number of relays in the mandatory path. Anything shorter than three
/// hops leaks the sender's or recipient's network position to the first
/// or last relay, so the constructor refuses shorter paths.
pub const MIN_RELAYS: usize = 3;

/// Domain tag for keys encrypting routing layers: the entry and middle
/// layers whose plaintext carries the next relay's X25519 address.
pub(crate) const LAYER_KEY_DOMAIN: u8 = 0x01;
/// Domain tag for the key encrypting the inner transaction payload at
/// the exit relay.
pub(crate) const INNER_KEY_DOMAIN: u8 = 0x02;
/// Domain tag for the key encrypting the final marker layer whose
/// plaintext signals the exit condition to the last relay.
pub(crate) const MARKER_KEY_DOMAIN: u8 = 0x03;

/// Error returned when an onion envelope cannot be constructed.
///
/// The error is a plain value type (rather than a panic) because envelope
/// construction is exposed to wallet and FFI callers, where a malformed
/// relay list is an ordinary input error and must never abort the host
/// process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnionError {
    /// Fewer relays than the mandatory three-hop path were supplied.
    TooFewRelays { provided: usize, required: usize },
    /// A relay public key is low order, so every Diffie-Hellman output
    /// against it is the all-zero string and the layer would be
    /// encrypted under a publicly computable key.
    LowOrderRelayKey { index: usize },
    /// The serialized transaction does not fit into the fixed-size inner
    /// payload even after framing, so the envelope cannot be built.
    PayloadTooLarge { size: usize, limit: usize },
}

impl std::fmt::Display for OnionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OnionError::TooFewRelays { provided, required } => write!(
                f,
                "onion routing requires at least {} relays, {} provided",
                required, provided
            ),
            OnionError::LowOrderRelayKey { index } => write!(
                f,
                "relay {} uses a low-order X25519 key and cannot secure an envelope layer",
                index
            ),
            OnionError::PayloadTooLarge { size, limit } => write!(
                f,
                "serialized transaction of {} bytes exceeds the {}-byte inner payload",
                size, limit
            ),
        }
    }
}

impl std::error::Error for OnionError {}

/// One layer of the onion envelope.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    pub ephemeral: [u8; 32],
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

/// A 3-hop onion-wrapped transaction.
/// The final_ephemeral field stores the X25519 public key needed by the
/// exit relay to decrypt the inner payload after all layers are peeled away.
/// Envelopes carry no identifier field: replay protection keys on the
/// hash of the serialized envelope bytes, which the originator cannot
/// influence without breaking authentication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnionTx {
    pub layers: Vec<Layer>,
    pub inner: Vec<u8>,
    pub inner_nonce: [u8; 12],
    /// Public ephemeral key used to encrypt the inner transaction payload.
    /// The final relay uses this together with its static secret to derive
    /// the AES-GCM key for the inner ciphertext.
    pub final_ephemeral: [u8; 32],
}

/// Result of peeling one layer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeelResult {
    pub next_relay: [u8; 32],
    pub remaining: OnionTx,
}

/// Pad a variable-length payload to a fixed size by prepending a 2-byte length
/// and appending zero bytes. The length prefix is hidden inside the AES-GCM ciphertext.
/// Returns None when the payload does not fit, so callers can surface a
/// typed input error instead of aborting the host process.
fn pad_payload(data: &[u8], size: usize) -> Option<Vec<u8>> {
    if data.len() + 2 > size {
        return None;
    }
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
    out.resize(size, 0);
    Some(out)
}

/// Strip the length prefix and padding from a decrypted payload.
/// Returns None if the length prefix is malformed.
fn unpad_payload(padded: &[u8]) -> Option<Vec<u8>> {
    if padded.len() < 2 {
        return None;
    }
    let len = u16::from_le_bytes(padded[..2].try_into().ok()?) as usize;
    if 2 + len > padded.len() {
        return None;
    }
    Some(padded[2..2 + len].to_vec())
}

/// Clear the exported byte copy of an ephemeral X25519 secret.
/// The shared layer key is the only secret derivative needed after the
/// Diffie-Hellman step; every copy of the ephemeral secret material that
/// this crate controls is overwritten so the bytes cannot linger in freed
/// stack memory. The upstream StaticSecret value itself has no guaranteed
/// zeroization contract and is confined to the smallest possible scope.
fn clear_ephemeral_secret_copy(secret: &x25519_dalek::StaticSecret) {
    let mut bytes = secret.to_bytes();
    zeroize::Zeroize::zeroize(&mut bytes);
}
