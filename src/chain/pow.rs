//! Proof-of-work verification.
//!
//! Slash uses RandomX so that mining stays CPU-friendly and ASIC-resistant.
//! The consensus-critical property here is that the verifier reconstructs
//! the exact byte string the miner searched over, hashed through a light
//! verification context. Any drift between the two sides would make valid
//! blocks unverifiable, so the input layout is part of the protocol.
//!
//! Every input is prefixed with a versioned domain tag ([`POW_DOMAIN`]) that
//! binds the proof to the Slash proof-of-work domain. Reusing a proof
//! across domains — another chain, another algorithm, a future protocol
//! revision — requires colliding under both constructions. The miner and
//! the verifier share the single builder [`pow_input`], so both sides hash
//! byte-identical inputs by construction.

/// Domain tag prepended to every RandomX proof-of-work input. The tag
/// versions the proof space: rotating the PoW algorithm or the input domain
/// is a new tag together with a new network identifier, never a mid-network
/// switch.
pub const POW_DOMAIN: &[u8] = b"SLASH-POW/v1";

/// Assemble the canonical RandomX input for a block header field set: the
/// domain tag followed by `prev || time || height || nonce || miner` in
/// little-endian encoding. Both the mining loops and the verifier call this
/// builder, so the two sides hash identical buffers by construction.
pub fn pow_input(prev: [u8; 32], time: u64, height: u64, miner: [u8; 32], nonce: u64) -> Vec<u8> {
    let mut inp = Vec::with_capacity(POW_DOMAIN.len() + 88);
    inp.extend_from_slice(POW_DOMAIN);
    inp.extend_from_slice(&prev);
    inp.extend_from_slice(&time.to_le_bytes());
    inp.extend_from_slice(&height.to_le_bytes());
    inp.extend_from_slice(&nonce.to_le_bytes());
    inp.extend_from_slice(&miner);
    inp
}

/// Verify the proof-of-work of a header candidate and return the derived
/// slot cell. The hash input is the canonical builder output, matching the
/// mining loops bit for bit. A zero difficulty fails closed: no nonce can
/// satisfy it because the modulo never evaluates.
///
/// The returned value is the proof-of-work slot: the treasury-owned cell
/// index `value % N` that the header claims as `mined_cell`. Returning the
/// slot only on a valid proof pins the slot to the hash output, so a miner
/// cannot pick an arbitrary cell and must claim whatever the nonce draws.
pub fn verify_pow(
    prev: [u8; 32],
    time: u64,
    height: u64,
    miner: [u8; 32],
    nonce: u64,
    diff: u64,
) -> Option<u64> {
    if diff == 0 {
        return None;
    }
    // Light context: full-memory mining contexts are cached separately with
    // a TTL; verification only needs the fast variant.
    let ctx = crate::mining::get_context(&prev, false);
    let hasher = rust_randomx::Hasher::new(ctx);
    let inp = pow_input(prev, time, height, miner, nonce);
    let out = hasher.hash(&inp);
    let h = out.as_ref();
    // The first eight bytes of the hash, little-endian, are the candidate
    // value; the difficulty game is divisibility of that value.
    let value = u64::from_le_bytes(h[..8].try_into().ok()?);
    if value % diff == 0 {
        Some(value % crate::state::N)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pow_input_prefixes_the_versioned_domain_tag() {
        // The tag leads every input; the payload after the tag matches the
        // untagged field layout exactly, which is what separates the two
        // domains byte for byte.
        let prev = [7u8; 32];
        let miner = [9u8; 32];
        let inp = pow_input(prev, 1_700_000_000, 42, miner, 77);
        assert!(inp.starts_with(POW_DOMAIN));
        assert_eq!(inp.len(), POW_DOMAIN.len() + 88);
        let legacy = [
            &prev[..],
            &1_700_000_000u64.to_le_bytes()[..],
            &42u64.to_le_bytes()[..],
            &77u64.to_le_bytes()[..],
            &miner[..],
        ]
        .concat();
        assert_eq!(inp[POW_DOMAIN.len()..], legacy[..]);
    }

    #[test]
    fn pow_input_differs_across_every_field() {
        // Every header field moves the input, so no field can be swapped
        // without invalidating every existing proof.
        let base = pow_input([1u8; 32], 100, 5, [2u8; 32], 9);
        assert_ne!(base, pow_input([3u8; 32], 100, 5, [2u8; 32], 9));
        assert_ne!(base, pow_input([1u8; 32], 101, 5, [2u8; 32], 9));
        assert_ne!(base, pow_input([1u8; 32], 100, 6, [2u8; 32], 9));
        assert_ne!(base, pow_input([1u8; 32], 100, 5, [2u8; 32], 10));
        assert_ne!(base, pow_input([1u8; 32], 100, 5, [4u8; 32], 9));
    }

    #[test]
    fn miner_and_verifier_hash_identical_inputs() {
        // With difficulty one every nonce satisfies the predicate, so the
        // first nonce proves agreement: the verifier returns exactly the
        // slot a manual hash over the same builder-produced input yields.
        let prev = [5u8; 32];
        let miner = [6u8; 32];
        let slot = verify_pow(prev, 1_700_000_000, 1, miner, 0, 1).unwrap();
        let ctx = crate::mining::get_context(&prev, false);
        let out = rust_randomx::Hasher::new(ctx).hash(&pow_input(prev, 1_700_000_000, 1, miner, 0));
        let value = u64::from_le_bytes(out.as_ref()[..8].try_into().unwrap());
        assert_eq!(slot, value % crate::state::N);
    }

    #[test]
    fn zero_difficulty_never_verifies() {
        assert!(verify_pow([0u8; 32], 0, 0, [0u8; 32], 0, 0).is_none());
    }
}
