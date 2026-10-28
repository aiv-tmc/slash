//! The transaction list and the fee claims of a block, executed against a
//! speculative state with the treasury pairing gates applied on both
//! sides of the bonding curve through the shared pairing tables.

use super::super::chain::Chain;
use super::super::pairing::{self, Funding, PairingTables};
use super::super::subsidy::subsidy_at;
use super::super::types::{Block, BlockError};
use crate::state;

/// Emit a rejection trace line when the `SLASH_TRACE_TX` environment
/// variable is set. The switch is read once per block execution; the
/// facility costs nothing when disabled and names the exact refusal site
/// — freeze gate, user transaction engine, or treasury pairing — with
/// the fields that decided it, so a failing template or fixture can be
/// diagnosed from a single test run instead of bisecting consensus code.
fn trace_rejection(switch: bool, site: &str, detail: String) {
    if switch {
        eprintln!("[txs] refused at {site}: {detail}");
    }
}

/// Parse a governance operation out of a transaction memo. This is the
/// single parse site for the whole execution path: the emergency-freeze
/// lifeline check and the operation application below both consume its
/// result, so a memo either parses for every consumer or for none, and
/// the freeze carve-out can never disagree with the operation the block
/// actually applies.
fn parse_governance_op(memo: &Option<Vec<u8>>) -> Option<crate::dao::DaoOp> {
    memo.as_ref()
        .and_then(|bytes| bincode::deserialize::<crate::dao::DaoOp>(bytes).ok())
}

/// The emergency lifeline predicate: a transaction carrying an
/// `EmergencyResume` submission is the single class admitted while the
/// broad user-transaction freeze is live. The predicate delegates to the
/// canonical implementation beside the governance types, so the mempool
/// admission path and this execution path decide lifeleness on the same
/// bytes by construction and can never drift apart.
fn is_emergency_lifeline(memo: &Option<Vec<u8>>) -> bool {
    crate::dao::is_emergency_lifeline(memo)
}

impl Chain {
    /// Execute the transaction list and the fee claims of a block against
    /// a speculative state, applying the treasury pairing rules on both
    /// sides of the bonding curve through the shared pairing tables. Buy
    /// side: a signed treasury mint at index i > 0 is valid only when an
    /// earlier ordinary transaction in the same block pays at least the
    /// curve buy price into the treasury vault, and each payment may fund
    /// only one mint. Sell side: an ordinary transaction that pays cells
    /// back to the treasury address is a sell, and a signed treasury
    /// payout funded from the vault must refund exactly one earlier sell
    /// at the curve sell price. Returns the error code of the first
    /// failing transaction.
    pub(super) fn execute_block_txs(
        &self,
        temp: &mut crate::state::State,
        block: &Block,
    ) -> Result<(), BlockError> {
        // Enforce the transaction-count cap before any execution work.
        if block.txs.len() > super::super::params::MAX_TX_PER_BLOCK {
            return Err(BlockError::InvalidTx);
        }
        let rules = crate::governance::active_rules(&self.version_bits.deployments, block.height);
        let allow_scheme1 = rules
            .iter()
            .any(|r| matches!(r, crate::governance::Rule::AllowScheme1));
        let dao_active = rules
            .iter()
            .any(|r| matches!(r, crate::governance::Rule::DaoV1));
        let start_index = if subsidy_at(block.height) > 0 { 1 } else { 0 };
        // Emergency pause, evaluated once against the pre-block state: the
        // freeze takes effect from the block after the one that carried it
        // and never mid-block. Only the lifeline class keeps flowing, so
        // the chain itself never stops — coinbase, slot, and fee claims
        // keep executing in every block, including frozen ones.
        let user_txs_frozen = temp.emergency_active(crate::dao::EMERGENCY_USER_TXS, block.height);
        // The rejection-trace switch is resolved once per execution.
        let trace = std::env::var_os("SLASH_TRACE_TX").is_some();
        let total_sold_at_start = temp.total_sold;
        let mut tables = PairingTables::new();
        let mut dev_fund_spends = 0u32;
        for (i, tx) in block.txs.iter().enumerate().skip(start_index) {
            // The freeze refuses every transaction except the lifeline:
            // votes and ordinary transfers stop, an EmergencyResume
            // submission keeps flowing so governance can always lift the
            // freeze it imposed.
            if user_txs_frozen && !is_emergency_lifeline(&tx.memo) {
                trace_rejection(
                    trace,
                    "emergency freeze",
                    format!(
                        "index {i} from {} memo_len {:?} lifeline {:?}",
                        hex::encode(tx.from),
                        tx.memo.as_ref().map(|m| m.len()),
                        is_emergency_lifeline(&tx.memo)
                    ),
                );
                return Err(BlockError::InvalidTx);
            }
            // Structural amplification caps, identical to the mempool
            // admission gate: the range and page caps bound the staging
            // footprint before any page materialization happens, and the
            // field caps refuse oversized signatures and memos before
            // any hashing or verification work. A miner must not bypass
            // the caps mempool admission enforces by including in a
            // block what admission would refuse, so block execution
            // applies the same shape checks to every transaction before
            // the engine or the pairing tables touch it.
            if !self.tx_structure_valid(tx) {
                trace_rejection(
                    trace,
                    "structure caps",
                    format!("index {i} from {}", hex::encode(tx.from)),
                );
                return Err(BlockError::InvalidTx);
            }
            if tx.from == state::TREASURY {
                // Emergency pause: the treasury-pairs flag freezes the
                // bonding-curve surface while ordinary user transfers
                // keep flowing. The check reads the pre-block state, so
                // the freeze takes effect from the block after the one
                // that carried it.
                if temp.emergency_active(crate::dao::EMERGENCY_TREASURY_PAIRS, block.height) {
                    return Err(BlockError::InvalidTx);
                }
                // Sunset parity: with dao_v1 active, a signed non-coinbase
                // treasury transaction is invalid regardless of pairing.
                if dao_active {
                    return Err(BlockError::InvalidTx);
                }
                // Classify the treasury sender by the ownership of its
                // inputs against the executing state: treasury-owned
                // inputs make it a curve mint, vault-owned inputs a sell
                // payout. A span owned by neither address has no funding
                // source and fails closed.
                let classification = PairingTables::classify(&*temp, tx, block.height)?;
                match classification {
                    Funding::Mint => {
                        // The minted amount is the total width of the
                        // outputs; a malformed range width fails closed.
                        let minted =
                            pairing::total_width(&tx.outputs).ok_or(BlockError::InvalidTx)?;
                        if minted > 0 {
                            let price = crate::treasury::buy_price(minted, temp.total_sold);
                            if !tables.pair_buy(block, i, price) {
                                return Err(BlockError::InvalidTx);
                            }
                            temp.total_sold = temp.total_sold.saturating_add(minted);
                        }
                        // The mint executes through the ordinary
                        // transaction pipeline: the treasury signer
                        // signature is verified and the canonical treasury
                        // range is debited.
                        if !temp.tx(
                            tx.from,
                            &tx.inputs,
                            &Self::to_state_outputs(&tx.outputs),
                            &tx.sig,
                            block.height,
                            self.chain_id.as_bytes(),
                            tx.scheme,
                            &tx.memo,
                        ) {
                            return Err(BlockError::InvalidTx);
                        }
                    }
                    Funding::Payout => {
                        // The payout amount is the total width of its
                        // outputs; a malformed range width fails closed.
                        let payout =
                            pairing::total_width(&tx.outputs).ok_or(BlockError::InvalidTx)?;
                        let sold = tables
                            .match_payout(payout, total_sold_at_start)
                            .ok_or(BlockError::InvalidTx)?;
                        // The payout moves buyer funds out of the vault, so
                        // it must carry a valid treasury signer signature
                        // like any other treasury transaction. An
                        // unconfigured all-zero signer key refuses the
                        // spend outright: the zero point is a valid
                        // small-order curve point, so signature
                        // verification alone would not be a sufficient
                        // barrier.
                        if temp.treasury_signer == [0u8; 32] {
                            return Err(BlockError::InvalidTx);
                        }
                        // Hash the payout exactly like any other treasury
                        // transaction: sender, inputs, outputs, chain
                        // identifier, scheme, and memo all commit to the
                        // signature.
                        let hash = Self::tx_signature_hash(
                            &tx.from,
                            &tx.inputs,
                            &Self::to_state_outputs(&tx.outputs),
                            self.chain_id.as_bytes(),
                            tx.scheme,
                            &tx.memo,
                        );
                        // The signature verifies against the declared
                        // scheme: the fail-closed dispatcher rejects any
                        // scheme this build cannot verify, so a payout can
                        // never borrow the cryptography of another scheme.
                        if !crate::crypto::verify_scheme(
                            tx.scheme,
                            &temp.treasury_signer,
                            &hash,
                            &tx.sig,
                        ) {
                            return Err(BlockError::InvalidTx);
                        }
                        // The matched sold count moves back out of
                        // total_sold and into the buy-back counter, so
                        // the curve price recovers after a sale.
                        temp.total_sold = temp.total_sold.saturating_sub(sold);
                        temp.total_bought_back = temp.total_bought_back.saturating_add(sold);
                        // The payout executes through the vault spend
                        // primitive, which requires every input to be
                        // vault-owned and unlocked and packs the outputs
                        // exactly over the inputs.
                        if !temp.payout(
                            &tx.inputs,
                            &Self::to_state_outputs(&tx.outputs),
                            block.height,
                        ) {
                            return Err(BlockError::InvalidTx);
                        }
                    }
                }
            } else {
                // Scheme gate: the declared scheme must be implemented in
                // this build and admitted by the deployment policy — scheme
                // 1 only while `scheme1_ready` is active — so an activated
                // soft fork can never enable a scheme the consensus code
                // cannot verify.
                if !crate::crypto::scheme_supported(tx.scheme) || (!allow_scheme1 && tx.scheme > 0)
                {
                    return Err(BlockError::InvalidTx);
                }
                if tx.from == state::DEV_FUND {
                    // Dev fund spends are signer-authorized and limited to
                    // one per block, matching the mempool policy.
                    dev_fund_spends += 1;
                    if dev_fund_spends > 1 {
                        return Err(BlockError::InvalidTx);
                    }
                }
                if !temp.tx(
                    tx.from,
                    &tx.inputs,
                    &Self::to_state_outputs(&tx.outputs),
                    &tx.sig,
                    block.height,
                    self.chain_id.as_bytes(),
                    tx.scheme,
                    &tx.memo,
                ) {
                    trace_rejection(
                        trace,
                        "user transaction engine",
                        format!(
                            "index {i} from {} inputs {:?} scheme {}",
                            hex::encode(tx.from),
                            tx.inputs,
                            tx.scheme
                        ),
                    );
                    return Err(BlockError::InvalidTx);
                }
                // Record any payment back to the treasury address as a
                // pending sell; a later payout in this block may refund
                // it.
                tables.record_sell(i, pairing::treasury_returned(tx));
            }
            // Apply a governance operation riding this transaction's memo
            // after the transfer itself has executed, so a rejected
            // operation never invalidates an otherwise valid transfer.
            if dao_active {
                if let Some(op) = parse_governance_op(&tx.memo) {
                    // The operation needs the proposal tables (a field
                    // of the state) and the state itself for treasury
                    // execution and weight lookups. The tables are
                    // moved out through a temporary genesis value so
                    // the two mutable borrows stay disjoint, and are
                    // moved back immediately after the call.
                    let mut dao = std::mem::replace(&mut temp.dao, crate::dao::DaoState::genesis());
                    dao.apply_op(temp, tx.from, &op, block.height);
                    temp.dao = dao;
                }
            }
        }
        // Execute the miner's fee claims: the sweep credits the miner and
        // debits the vault, rejecting any claim that fails ownership,
        // lock, or miner-destination checks. An empty claim set is valid.
        if !temp.claim_fees(
            block.miner,
            &Self::to_state_outputs(&block.fee_claims),
            block.height,
        ) {
            return Err(BlockError::InvalidTx);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "txs_tests.rs"]
mod tests;
