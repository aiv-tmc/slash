//! src/chain/validation.rs — the validation entry points shared by every
//! path that admits a block or a transaction: mempool admission (`validate_tx`),
//! the block pre-checks (`validate_block`), and the amplification caps
//! both paths enforce before any state work.
//!
//! The mempool path and the block path deliberately run the same
//! structural policy (`tx_structure_valid`) and the same state predicates
//! (`State::tx` in execution, `State::tx_speculative` in admission), so a
//! transaction admitted to the mempool is always executable in the next
//! block, and a transaction invalid for a block is never admitted to the
//! mempool in the first place. The admission twin stages on a
//! touched-page overlay and commits nothing, so testing a transaction
//! costs work proportional to the transaction rather than a clone of the
//! ledger.

use super::chain::Chain;
use super::params::{
    MAX_BLOCK_SIZE_BYTES, MAX_MEMO_BYTES, MAX_PAGES_PER_TX, MAX_RANGES_PER_TX, MAX_SIG_BYTES,
};
use crate::state;

/// The emergency lifeline predicate, byte-identical to the consensus-side
/// predicate in `chain/execution/txs.rs`: a transaction carrying an
/// `EmergencyResume` submission is the single class admitted while the
/// broad user-transaction freeze is live. Mempool admission and block
/// execution must apply the same exception to the same bytes, or a
/// lifeline transaction would assemble into templates it can never
/// execute.
fn is_emergency_lifeline(memo: &Option<Vec<u8>>) -> bool {
    matches!(
        memo.as_ref()
            .and_then(|bytes| bincode::deserialize::<crate::dao::DaoOp>(bytes).ok()),
        Some(crate::dao::DaoOp::SubmitProposal {
            payload: crate::dao::DaoPayload::EmergencyResume,
        })
    )
}

impl Chain {
    /// The mempool admission check for a transaction: structural policy
    /// first, then the read-only speculative execution against the tip
    /// state. Nothing is applied either way.
    pub fn validate_tx(&self, tx: &super::types::Tx) -> bool {
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

    /// The per-transaction amplification caps: the total number of input
    /// and output ranges and the number of distinct pages the transaction
    /// touches. Both validation paths call this before any state work, so
    /// a transaction crafted to maximize per-range page-map operations or
    /// to fan out across many pages is rejected at the structural gate
    /// regardless of its signatures or funding.
    pub(crate) fn tx_caps_valid(&self, tx: &super::types::Tx) -> bool {
        if tx.inputs.len().saturating_add(tx.outputs.len()) > MAX_RANGES_PER_TX {
            return false;
        }
        let mut pages = std::collections::BTreeSet::new();
        for &(start, _) in &tx.inputs {
            pages.insert(state::page_id(start));
        }
        for out in &tx.outputs {
            pages.insert(state::page_id(out.start));
        }
        pages.len() <= MAX_PAGES_PER_TX
    }

    /// The structural policy shared by mempool admission and block
    /// validation: input sanity, exact output packing, the minimum output
    /// amount, the field caps, the treasury-lock rule, the scheme gate,
    /// the mandatory fee, the treasury sunset, the emergency pause, the
    /// dev-fund share while the rule is active, and the global lock cap.
    /// Crate-visible so the test suites pin the policy directly instead
    /// of re-deriving it through speculative execution, which would fail
    /// a state rule before reaching the structural one under test.
    pub(crate) fn tx_structure_valid(&self, tx: &super::types::Tx) -> bool {
        if tx.inputs.is_empty() || tx.outputs.is_empty() {
            return false;
        }
        // Amplification caps run before every other structural check:
        // they are pure shape properties and bound the work of everything
        // below them.
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
        let total_out: u64 = tx.outputs.iter().map(|o| o.end - o.start).sum();
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
        let fee: u64 = tx
            .outputs
            .iter()
            .filter(|o| o.to == state::FEE_VAULT)
            .map(|o| o.end - o.start)
            .sum();
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
            let dev: u64 = tx
                .outputs
                .iter()
                .filter(|o| o.to == state::DEV_FUND)
                .map(|o| o.end - o.start)
                .sum();
            let required = (total_out.saturating_mul(self.state.dev_fee_bps) / 10_000).max(1);
            if dev < required {
                return false;
            }
        }
        // Global lock cap: locked outputs must keep the locked supply
        // bounded.
        let locked: u64 = tx
            .outputs
            .iter()
            .filter(|o| o.lock.is_some())
            .map(|o| o.end - o.start)
            .sum();
        if locked > state::MAX_LOCKED_CELLS {
            return false;
        }
        true
    }

    /// The block pre-checks: active-rule enforcement (version and size)
    /// plus the full speculative state transition. Used by gossip
    /// admission, the mining template loop, and the sync pipeline before a
    /// block is applied anywhere.
    pub fn validate_block(&self, block: &super::types::Block) -> bool {
        if self.check_block_meta(block).is_err() {
            return false;
        }
        // Enforce an activated RequireBlockVersion2 deployment.
        let rules = crate::governance::active_rules(&self.version_bits.deployments, block.height);
        if rules
            .iter()
            .any(|r| matches!(r, crate::governance::Rule::RequireBlockVersion2))
            && block.version < 2
        {
            return false;
        }
        // Enforce an activated MaxBlockSize deployment. The serialized
        // length is computed once and compared against the stricter of
        // the active deployment limit and the base consensus limit.
        let limit = rules
            .iter()
            .find_map(|r| match r {
                crate::governance::Rule::MaxBlockSize { max_bytes } => Some(*max_bytes),
                _ => None,
            })
            .unwrap_or(MAX_BLOCK_SIZE_BYTES)
            .min(MAX_BLOCK_SIZE_BYTES);
        if bincode::serialize(block)
            .map(|b| b.len())
            .unwrap_or(usize::MAX)
            > limit
        {
            return false;
        }
        self.execute_block_state(&self.state.clone(), block).is_ok()
    }
}
