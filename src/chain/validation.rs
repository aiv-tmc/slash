//! src/chain/validation.rs — the validation entry points shared by every
//! path that admits a block or a transaction: mempool admission
//! (`validate_tx`), the block pre-checks (`validate_block`), and the
//! amplification caps both paths enforce before any state work.
//!
//! The mempool path and the block path deliberately run the same
//! structural policy and the same state predicates, so a transaction
//! admitted to the mempool is always executable in the next block, and a
//! transaction invalid for a block is never admitted to the mempool in
//! the first place. The files under `validation/` split the entry points
//! by responsibility while keeping the `Chain` implementation unified.
//!
//! The admission twin stages on a touched-page overlay and commits
//! nothing, so testing a transaction costs work proportional to the
//! transaction rather than a clone of the ledger.

mod block;
mod tx_policy;
mod tx_structure;

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
