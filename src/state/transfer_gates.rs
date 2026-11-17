//! The shared predicate core of the transfer engine: every check that
//! does not mutate state, in one place. The committing engine
//! (`State::tx`) and the read-only admission twin
//! (`State::tx_speculative`) both enter here, so no gate can drift
//! between mempool admission and block execution. The method is defined
//! in this child module of `transfer.rs` and reaches the crate as
//! `pub(crate)`, exactly the visibility the two entry points and the
//! speculative twin already relied on. Under the `SLASH_TRACE_TX`
//! environment variable every refusal logs the gate that issued it, so
//! a failing transaction localizes its defect from a single run.

use super::*;

/// Log one gate refusal when the trace switch is set. The site name is
/// a stable identifier: tests and operators match on it, and the
/// consensus behavior never depends on whether the line prints.
fn gate_trace(site: &str, from: &[u8; 32], inputs: &[(u64, u64)]) {
    if std::env::var_os("SLASH_TRACE_TX").is_some() {
        eprintln!(
            "[txs] gate {site} refused: from {} inputs {:?}",
            hex::encode(from),
            inputs
        );
    }
}

impl State {
    /// Every predicate that does not mutate state, in one place: shape,
    /// minimums, the treasury-lock and provenance refusals, signature
    /// authorization, the mandatory fee, and the dev-fund share. The
    /// committing engine and the admission twin both enter here, so no
    /// gate can drift between mempool admission and block execution.
    pub(crate) fn transfer_gates(
        &self,
        from: &[u8; 32],
        inputs: &[(u64, u64)],
        outputs: &[Output],
        sig: &[u8],
        chain_id: &[u8],
        scheme: u8,
        memo: &Option<Vec<u8>>,
    ) -> bool {
        if inputs.is_empty() || outputs.is_empty() {
            gate_trace("non-empty io", from, inputs);
            return false;
        }
        // Inputs must be sorted and non-overlapping, so staged removal can
        // walk them in order and every cell is charged exactly once.
        let mut prev_end = 0u64;
        for &(s, e) in inputs {
            if e <= s || s < prev_end {
                gate_trace("sorted inputs", from, inputs);
                return false;
            }
            prev_end = e;
        }
        // Outputs must pack the inputs exactly in declaration order.
        let mut cursor = inputs[0].0;
        for out in outputs {
            if out.end <= out.start || out.start != cursor {
                gate_trace("exact packing", from, inputs);
                return false;
            }
            cursor = out.end;
        }
        if cursor != prev_end {
            gate_trace("packing coverage", from, inputs);
            return false;
        }
        let total_out: u64 = outputs.iter().map(|o| o.end - o.start).sum();
        if total_out < MIN {
            gate_trace("minimum output", from, inputs);
            return false;
        }
        // Implementation gate: only schemes this build implements reach
        // the signature check. The gate lives in the shared core, so
        // mempool admission and block execution refuse an unsupported
        // scheme identically, and an activated soft-fork deployment can
        // never admit a scheme the consensus code cannot verify.
        if !crate::crypto::scheme_supported(scheme) {
            gate_trace("scheme implemented", from, inputs);
            return false;
        }
        // A locked output back to the treasury is refused outright: no
        // key controls the treasury address, so a lock on a treasury-owned
        // cell could never be removed by spending it, and a live lock
        // inside a canonical funding range would wedge every mint
        // selecting that range.
        if outputs.iter().any(|o| o.to == TREASURY && o.lock.is_some()) {
            gate_trace("treasury lock", from, inputs);
            return false;
        }
        // A sell of governance-minted cells back to the treasury is
        // refused: the curve vault funds buy-backs exclusively from
        // payments buyers made, and a governance mint issued these cells
        // without any payment, so letting them pair with a payout would
        // drain the vault with no purchase ever occurring. The provenance
        // set lives in the consensus state, so mempool admission and
        // block execution refuse the identical transaction through this
        // single gate, and provenance survives transfers to any address.
        if outputs
            .iter()
            .any(|o| o.to == TREASURY && self.dao.minted.contains(o.start, o.end))
        {
            gate_trace("mint provenance", from, inputs);
            return false;
        }
        // Signature authorization by sender class. A regular sender signs
        // with its own key. The treasury and dev fund addresses are
        // protocol constants that no user key controls, so spends from
        // either must be signed by the configured signer for that
        // address, and a chain without a configured signer refuses every
        // transaction from it by construction.
        let signer: &[u8; 32] = if from == &TREASURY {
            if self.treasury_signer == [0u8; 32] {
                gate_trace("treasury signer configured", from, inputs);
                return false;
            }
            &self.treasury_signer
        } else if from == &DEV_FUND {
            if self.dev_fund_signer == [0u8; 32] {
                gate_trace("dev fund signer configured", from, inputs);
                return false;
            }
            &self.dev_fund_signer
        } else {
            from
        };
        // The digest commits to the sender, inputs, outputs, chain
        // identifier, scheme, and memo; the digest function lives on the
        // chain so mempool and block paths compute the identical hash for
        // the identical transaction. The verification dispatches on the
        // declared scheme and is fail-closed: a scheme without an
        // implementation in this build is rejected outright rather than
        // reinterpreted under Ed25519, so a transaction can never pass
        // verification by borrowing the cryptography of another scheme.
        let hash =
            crate::chain::Chain::tx_signature_hash(from, inputs, outputs, chain_id, scheme, memo);
        if !crate::crypto::verify_scheme(scheme, signer, &hash, sig) {
            gate_trace("signature", from, inputs);
            return false;
        }
        // Fee: outputs to the fee vault must total at least total_out /
        // MIN_FEE_DENOMINATOR. Treasury sends are consensus-only
        // constructions and never pay the user fee.
        let fee_out: u64 = outputs
            .iter()
            .filter(|o| o.to == FEE_VAULT)
            .map(|o| o.end - o.start)
            .sum();
        if from != &TREASURY && fee_out.saturating_mul(MIN_FEE_DENOMINATOR) < total_out {
            gate_trace("mandatory fee", from, inputs);
            return false;
        }
        // Dev-fund share while configured. Zero disables the rule
        // entirely. Dev-fund spends and treasury paths are exempt.
        if self.dev_fee_bps > 0 && from != &DEV_FUND && from != &TREASURY {
            let dev_out: u64 = outputs
                .iter()
                .filter(|o| o.to == DEV_FUND)
                .map(|o| o.end - o.start)
                .sum();
            let required = (total_out.saturating_mul(self.dev_fee_bps) / 10_000).max(1);
            if dev_out < required {
                gate_trace("dev fund share", from, inputs);
                return false;
            }
        }
        true
    }
}
