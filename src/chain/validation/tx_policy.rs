//! Transaction admission policy: the public mempool entry point and the
//! structural transaction gate shared by mempool and block validation.

use super::is_emergency_lifeline;
use crate::chain::chain::Chain;
use crate::chain::params::{MAX_MEMO_BYTES, MAX_SIG_BYTES};
use crate::chain::types::Tx;
use crate::state;

impl Chain {
    /// The mempool admission check for a transaction: structural policy
    /// first, then the read-only speculative execution against the tip
    /// state. Nothing is applied either way.
    pub fn validate_tx(&self, tx: &Tx) -> bool {
        if !self.tx_structure_valid(tx) {
            return false;
        }
        // The admission twin runs every ownership, lock, fee, signature,
        // and pairing predicate the engine runs — the predicates and the
        // staging are one shared core, so mempool admission and consensus
        // cannot disagree about a transaction's validity — while staging
        // on a touched-page overlay and committing nothing.
        let height = self.tip_height() + 1;
        self.state.tx_speculative(
            tx.from,
            &tx.inputs,
            &Self::to_state_outputs(&tx.outputs),
            &tx.sig,
            height,
            self.chain_id.as_bytes(),
            tx.scheme,
            &tx.memo,
        )
    }

    /// The structural policy shared by mempool admission and block
    /// validation: input sanity, exact output packing, the minimum output
    /// amount, the field caps, the treasury-lock rule, the scheme gate,
    /// the mandatory fee, the treasury sunset, the emergency pause, the
    /// dev-fund share while the rule is active, and the lock-width bound.
    /// Crate-visible so the test suites pin the policy directly instead
    /// of re-deriving it through speculative execution, which would fail
    /// a state rule before reaching the structural one under test.
    pub(crate) fn tx_structure_valid(&self, tx: &Tx) -> bool {
        if tx.inputs.is_empty() || tx.outputs.is_empty() {
            return false;
        }
        // Amplification caps run before every other structural check:
        // they are pure shape properties and bound the work of everything
        // below them. They also reject ranges outside the ledger supply
        // before any page id is computed.
        if !self.tx_caps_valid(tx) {
            return false;
        }
        // Field caps refuse oversized variable-length fields before any
        // hashing or verification work. The enclosing block size limit
        // already bounds the wire format, but a near-megabyte signature
        // inside an otherwise valid block would still cost every node a
        // hash and a signature-parse attempt, and an oversized memo would
        // cost a bincode decode attempt on every validation path. The
        // signature cap matches the only deployed schemes (64-byte
        // Ed25519); the memo cap is generous margin over the largest
        // legal bincode-encoded DaoOp.
        if tx.sig.len() > MAX_SIG_BYTES {
            return false;
        }
        if tx.memo.as_ref().is_some_and(|m| m.len() > MAX_MEMO_BYTES) {
            return false;
        }
        // Inputs must be sorted and non-overlapping.
        let mut prev_end = 0u64;
        for &(s, e) in &tx.inputs {
            if e <= s || s < prev_end {
                return false;
            }
            prev_end = e;
        }
        // Outputs must pack the inputs exactly in declaration order.
        let mut cursor = tx.inputs[0].0;
        for out in &tx.outputs {
            if out.end <= out.start || out.start != cursor {
                return false;
            }
            cursor = out.end;
        }
        if cursor != tx.inputs.last().map(|(_, e)| *e).unwrap_or(0) {
            return false;
        }
        let mut total_out = 0u64;
        for output in &tx.outputs {
            let width = match output.end.checked_sub(output.start) {
                Some(width) => width,
                None => return false,
            };
            total_out = match total_out.checked_add(width) {
                Some(total) => total,
                None => return false,
            };
        }
        if total_out < state::MIN {
            return false;
        }
        // A locked output back to the treasury is refused in both
        // validation paths. No key controls the treasury address, so a
        // lock there could never be spent out, and a live lock inside a
        // canonical funding range would wedge every mint that selects
        // that range. The state engine enforces the same refusal on the
        // block path, keeping mempool and consensus in parity.
        if tx
            .outputs
            .iter()
            .any(|o| o.to == state::TREASURY && o.lock.is_some())
        {
            return false;
        }
        let height = self.tip_height() + 1;
        let rules = crate::governance::active_rules(&self.version_bits.deployments, height);
        // Scheme gate: a scheme is admitted only when this build
        // implements it (the fail-closed dispatcher) and the deployment
        // policy allows it — scheme 1 solely while `scheme1_ready` is
        // active — so an activated soft fork can never admit a scheme
        // the consensus code cannot verify.
        if !crate::crypto::scheme_supported(tx.scheme) {
            return false;
        }
        if rules
            .iter()
            .any(|r| matches!(r, crate::governance::Rule::AllowScheme1))
        {
            if tx.scheme > 1 {
                return false;
            }
        } else if tx.scheme != 0 {
            return false;
        }
        // Mandatory fee: every user transaction pays at least
        // total_out / MIN_FEE_DENOMINATOR into the fee vault. Treasury
        // transactions are consensus-only constructions and are exempt.
        let mut fee = 0u64;
        for output in tx.outputs.iter().filter(|o| o.to == state::FEE_VAULT) {
            let width = match output.end.checked_sub(output.start) {
                Some(width) => width,
                None => return false,
            };
            fee = match fee.checked_add(width) {
                Some(total) => total,
                None => return false,
            };
        }
        if tx.from != state::TREASURY && fee.saturating_mul(state::MIN_FEE_DENOMINATOR) < total_out
        {
            return false;
        }
        // Treasury sends are consensus-only constructions. Once governance
        // is live, the treasury sunset strips the legacy signer of its
        // authority: every signed treasury transaction is refused here,
        // in both the coinbase eras and the zero-subsidy era. The single
        // exception is the unsigned coinbase itself, which exists only
        // while the subsidy is positive: it is the scheduled mint, not a
        // signer-authorized spend, so it stays admissible exactly as long
        // as the block format reserves position zero for it. From era 9
        // on, every treasury transaction — signed or not — is refused,
        // mirroring the execution path's index rule.
        let dao_active = rules
            .iter()
            .any(|r| matches!(r, crate::governance::Rule::DaoV1));
        if dao_active
            && tx.from == state::TREASURY
            && (!tx.sig.is_empty() || crate::chain::subsidy_at(height) == 0)
        {
            return false;
        }
        // Emergency pause, mempool side: while the broad user-transaction
        // flag is live, nothing non-coinbase is admitted except the
        // lifeline — an EmergencyResume submission — which must keep
        // flowing so governance can always lift the freeze it imposed;
        // while the treasury-pairs flag is live, no signed treasury
        // transaction — the whole curve surface — is admitted. Both
        // checks mirror the execution path exactly, so a paused class
        // can never reach a miner's template from the mempool either.
        if self
            .state
            .emergency_active(crate::dao::EMERGENCY_USER_TXS, height)
            && !is_emergency_lifeline(&tx.memo)
        {
            return false;
        }
        if tx.from == state::TREASURY
            && !tx.sig.is_empty()
            && self
                .state
                .emergency_active(crate::dao::EMERGENCY_TREASURY_PAIRS, height)
        {
            return false;
        }
        // Dev-fund share while the rule is active, exempting treasury,
        // vault-pairing, and dev-fund-spend paths.
        let dev_active = rules
            .iter()
            .any(|r| matches!(r, crate::governance::Rule::DevFund));
        if dev_active && tx.from != state::DEV_FUND && tx.from != state::TREASURY {
            let mut dev = 0u64;
            for output in tx.outputs.iter().filter(|o| o.to == state::DEV_FUND) {
                let width = match output.end.checked_sub(output.start) {
                    Some(width) => width,
                    None => return false,
                };
                dev = match dev.checked_add(width) {
                    Some(total) => total,
                    None => return false,
                };
            }
            let required = (total_out.saturating_mul(self.state.dev_fee_bps) / 10_000).max(1);
            if dev < required {
                return false;
            }
        }
        // The structural gate bounds the lock width a single transaction
        // can create; the live locked-supply cap is enforced by the shared
        // staging core before any lock is committed.
        let mut locked = 0u64;
        for output in tx.outputs.iter().filter(|o| o.lock.is_some()) {
            let width = match output.end.checked_sub(output.start) {
                Some(width) => width,
                None => return false,
            };
            locked = match locked.checked_add(width) {
                Some(total) => total,
                None => return false,
            };
        }
        if locked > state::MAX_LOCKED_CELLS {
            return false;
        }
        true
    }
}
