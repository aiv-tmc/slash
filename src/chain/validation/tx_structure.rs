//! Amplification caps for transactions: range counts, touched-page fan-out,
//! and the ledger-domain check that keeps page arithmetic sound.

use crate::chain::chain::Chain;
use crate::chain::params::{MAX_PAGES_PER_TX, MAX_RANGES_PER_TX};
use crate::chain::types::Tx;
use crate::state;

impl Chain {
    /// The per-transaction amplification caps: the total number of input
    /// and output ranges and the number of distinct pages the transaction
    /// touches. Both validation paths call this before any state work, so
    /// a transaction crafted to maximize per-range page-map operations or
    /// to fan out across many pages is rejected at the structural gate
    /// regardless of its signatures or funding.
    pub(crate) fn tx_caps_valid(&self, tx: &Tx) -> bool {
        if tx.inputs.len().saturating_add(tx.outputs.len()) > MAX_RANGES_PER_TX {
            return false;
        }
        let mut pages = std::collections::BTreeSet::new();
        for &(start, end) in &tx.inputs {
            if end <= start || end > state::N {
                return false;
            }
            pages.insert(state::page_id(start));
        }
        for out in &tx.outputs {
            if out.end <= out.start || out.end > state::N {
                return false;
            }
            pages.insert(state::page_id(out.start));
        }
        pages.len() <= MAX_PAGES_PER_TX
    }
}
