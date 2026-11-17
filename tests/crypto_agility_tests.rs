//! Consensus surface for cryptographic algorithm agility:
//! the proof-of-work input carries a versioned domain tag assembled by a
//! single shared builder, transaction signatures verify through a
//! fail-closed scheme dispatcher on both validation paths, and the
//! transaction signature hash commits to the declared scheme byte so a
//! signature minted for one scheme can never replay under another.

use slash::chain::{pow_input, verify_pow, Chain, POW_DOMAIN};
use slash::crypto::{scheme_supported, sign_scheme, verify_scheme, SCHEME_ED25519};
use slash::state::Output;

#[test]
fn pow_input_is_domain_tagged() {
    // Every proof-of-work input leads with the versioned domain tag; the
    // payload after the tag is the consensus field layout. Mutating any
    // single header field moves the input, so no field can be swapped
    // without invalidating every existing proof.
    let prev = [7u8; 32];
    let miner = [9u8; 32];
    let inp = pow_input(prev, 1_700_000_000, 42, miner, 77);
    assert!(inp.starts_with(POW_DOMAIN));
    assert!(!POW_DOMAIN.is_empty());
    let base = pow_input([1u8; 32], 100, 5, [2u8; 32], 9);
    assert_ne!(base, pow_input([3u8; 32], 100, 5, [2u8; 32], 9));
    assert_ne!(base, pow_input([1u8; 32], 101, 5, [2u8; 32], 9));
    assert_ne!(base, pow_input([1u8; 32], 100, 6, [2u8; 32], 9));
    assert_ne!(base, pow_input([1u8; 32], 100, 5, [2u8; 32], 10));
    assert_ne!(base, pow_input([1u8; 32], 100, 5, [4u8; 32], 9));
}

#[test]
fn pow_verifier_accepts_difficulty_one_and_rejects_zero() {
    // Difficulty one is satisfied by the first nonce; a zero difficulty
    // fails closed on every input.
    let slot = verify_pow([5u8; 32], 1_700_000_000, 1, [6u8; 32], 0, 1);
    assert!(slot.is_some());
    assert!(verify_pow([5u8; 32], 1_700_000_000, 1, [6u8; 32], 0, 0).is_none());
}

#[test]
fn scheme_dispatch_is_fail_closed() {
    // A perfectly valid Ed25519 signature must not verify under any
    // other scheme id, and no unimplemented scheme may sign. This is the
    // property both validation paths rely on: policy can admit a scheme
    // only when an implementation exists in the build.
    let (sk, pk) = slash::crypto::generate_keypair();
    let msg = b"agility probe";
    let sig = sign_scheme(SCHEME_ED25519, &sk, msg).expect("ed25519 is implemented");
    assert!(verify_scheme(SCHEME_ED25519, &pk, msg, &sig));
    for scheme in 1..=255u8 {
        assert!(
            !verify_scheme(scheme, &pk, msg, &sig),
            "scheme {scheme} must stay unimplemented"
        );
        assert!(!scheme_supported(scheme));
        assert!(sign_scheme(scheme, &sk, msg).is_none());
    }
}

#[test]
fn sighash_binds_the_declared_scheme() {
    // Two transactions identical in every field except the scheme byte
    // commit to different signature hashes, so a signature minted for one
    // scheme can never be replayed under another scheme's policy.
    let from = [2u8; 32];
    let inputs = vec![(0u64, 100u64)];
    let outputs = vec![Output {
        start: 0,
        end: 100,
        to: [1u8; 32],
        lock: None,
    }];
    let chain_id = b"/slash/0.5.0";
    let memo = &None;
    let scheme_zero = Chain::tx_signature_hash(&from, &inputs, &outputs, chain_id, 0, memo);
    let scheme_one = Chain::tx_signature_hash(&from, &inputs, &outputs, chain_id, 1, memo);
    assert_ne!(scheme_zero, scheme_one);
}
