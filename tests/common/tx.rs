//! Transaction builders: canonical wire transactions signed by the
//! consensus signature hash, plus the wire/engine output conversion.

use super::{chain, crypto, state};

/// Build and sign a wire transaction for the caller-supplied keypair.
/// `outputs` are engine-level outputs (including optional locks); the
/// signature commits to the sender, inputs, outputs, chain identifier,
/// scheme, and memo, exactly as consensus verification recomputes it.
pub fn make_tx(
    from: [u8; 32],
    sk: &[u8; 32],
    inputs: &[(u64, u64)],
    outputs: &[state::Output],
    chain_id: impl AsRef<[u8]>,
) -> chain::Tx {
    let hash = chain::Chain::tx_signature_hash(
        &from,
        inputs,
        outputs,
        chain_id.as_ref(),
        0,
        &None,
    );
    let sig = crypto::sign(sk, &hash);
    chain::Tx {
        from,
        inputs: inputs.to_vec(),
        outputs: outputs
            .iter()
            .map(|o| chain::Output {
                start: o.start,
                end: o.end,
                to: o.to,
                lock: o.lock,
            })
            .collect(),
        sig,
        scheme: 0,
        memo: None,
    }
}

/// Chain-first transaction builder used by the `chain_tests` suite.
///
/// `who` is the sender's public key and `sk` its secret; outputs are given
/// as (start, end, owner) triples and must pack the inputs exactly. Every
/// output returning cells to the sender carries `lock` as its lock height
/// when `lock` is non-zero, modelling a stake or escrow; outputs to third
/// parties (payees, the fee vault) stay unlocked.
pub fn make_tx_for(
    chain: &chain::Chain,
    who: [u8; 32],
    sk: &[u8; 32],
    inputs: &[(u64, u64)],
    outputs: &[(u64, u64, [u8; 32])],
    lock: u64,
) -> chain::Tx {
    let engine_outputs: Vec<state::Output> = outputs
        .iter()
        .map(|&(start, end, to)| state::Output {
            start,
            end,
            to,
            lock: if lock > 0 && to == who { Some(lock) } else { None },
        })
        .collect();
    make_tx(who, sk, inputs, &engine_outputs, chain.chain_id.as_bytes())
}

/// Variant of `make_tx_for` whose trailing argument is the height at which
/// the transaction is intended to be validated. The height is recorded as
/// the sender-output lock; tests that check lock expiry semantics mine the
/// chain up to the boundary before validating the returned transaction, so
/// mempool validation (which always evaluates at the next height) observes
/// exactly the intended evaluation height.
pub fn make_tx_at(
    chain: &chain::Chain,
    who: [u8; 32],
    sk: &[u8; 32],
    inputs: &[(u64, u64)],
    outputs: &[(u64, u64, [u8; 32])],
    height: u64,
) -> chain::Tx {
    make_tx_for(chain, who, sk, inputs, outputs, height)
}

/// Convert wire-level outputs into engine-level outputs. The two types
/// carry the identical four fields; the conversion is a pure field copy
/// used when a fixture drives `State::tx` directly.
pub fn state_outputs(wire: &[chain::Output]) -> Vec<state::Output> {
    wire.iter()
        .map(|o| state::Output {
            start: o.start,
            end: o.end,
            to: o.to,
            lock: o.lock,
        })
        .collect()
}
