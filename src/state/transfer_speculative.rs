//! src/state/transfer_speculative.rs — the admission twin of the
//! transfer engine. Mempool admission must answer "would this
//! transaction apply?" without paying for a clone of the whole ledger:
//! the gates and the staging live in the shared core (`transfer_gates`,
//! `stage_transfer` on `State`, defined in the parent module), and this
//! twin simply never commits. The verdict is bit-identical to the
//! committing engine by construction — both paths run the same
//! predicates in the same order against the same state — and the
//! mempool/consensus parity tests pin exactly that. Staging touches only
//! the pages the transaction touches, so the cost of testing a
//! transaction tracks the transaction's footprint.

use crate::state::page::Output;
use crate::state::state::State;

impl State {
    /// The read-only admission check: identical predicates and staging
    /// to `tx`, zero mutation. Returns true exactly when `tx` would
    /// commit against the same state. Under the `SLASH_TRACE_TX`
    /// environment variable the refusal phase (gates or staging) is
    /// logged, mirroring the committing engine's trace so the two paths
    /// can be told apart during diagnosis.
    pub fn tx_speculative(
        &self,
        from: [u8; 32],
        inputs: &[(u64, u64)],
        outputs: &[Output],
        sig: &[u8],
        height: u64,
        chain_id: &[u8],
        scheme: u8,
        memo: &Option<Vec<u8>>,
    ) -> bool {
        if !self.transfer_gates(&from, inputs, outputs, sig, chain_id, scheme, memo) {
            if std::env::var_os("SLASH_TRACE_TX").is_some() {
                eprintln!(
                    "[txs] speculative refused at gates: from {} inputs {:?} scheme {}",
                    hex::encode(from),
                    inputs,
                    scheme
                );
            }
            return false;
        }
        if self.stage_transfer(from, inputs, outputs, height).is_none() {
            if std::env::var_os("SLASH_TRACE_TX").is_some() {
                eprintln!(
                    "[txs] speculative refused at staging: from {} inputs {:?} height {}",
                    hex::encode(from),
                    inputs,
                    height
                );
            }
            return false;
        }
        true
    }
}
