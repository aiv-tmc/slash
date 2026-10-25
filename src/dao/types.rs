//! src/dao/types.rs — the value types of the governance layer.
//!
//! `ProposalStatus` is the lifecycle; `Proposal` is the record the
//! proposal table stores; `DaoOp` is the memo-carried operation. All
//! three are plain data: every rule that moves them lives in `state.rs`,
//! and they derive serde so the proposal table survives the state header
//! on disk.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Lifecycle of one proposal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalStatus {
    /// Open for votes.
    Voting,
    /// Closed and approved; waiting out the execution delay.
    Passed,
    /// Closed and executed; terminal.
    Executed,
    /// Closed without quorum or majority; terminal.
    Rejected,
    /// Closed and approved but the payload was not executable at the
    /// scheduled height; terminal.
    Expired,
}

/// One governance proposal. Weight is pinned at the snapshot height, so
/// purchases and burns inside the voting window cannot move the outcome
/// of a live proposal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u64,
    pub payload: DaoPayload,
    /// The address that paid the submission fee; recorded so the origin
    /// of every proposal is auditable on-chain.
    pub submitter: [u8; 32],
    /// The first height at which votes are accepted.
    pub start_height: u64,
    /// The height at which voting ends; votes land in
    /// [start_height, end_height).
    pub end_height: u64,
    /// The height whose locked supply decides the vote; fixed at
    /// submission, so weight earned or burned inside the window cannot
    /// influence the outcome.
    pub snapshot_height: u64,
    /// The quorum denominator, captured at submission: the total locked
    /// weight at the snapshot height. Cached on the record so closing a
    /// proposal never rescans the ledger for a denominator that was
    /// fixed the moment the proposal opened.
    pub locked_total: u64,
    pub yes_weight: u64,
    pub no_weight: u64,
    /// Every address that has cast a vote; the first vote wins, so the
    /// set doubles as the duplicate-vote guard.
    pub voters: std::collections::BTreeSet<[u8; 32]>,
    pub status: ProposalStatus,
    /// The height at which a passed proposal becomes executable; zero
    /// while the proposal has not passed.
    pub exec_height: u64,
}

/// The executable effect of a passed proposal. Every variant is bounded
/// at submission time so a passed proposal can only request transitions
/// that can actually execute.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DaoPayload {
    /// Mint treasury cells to an address through the canonical treasury
    /// selection.
    TreasuryMint { amount: u64, to: [u8; 32] },
    /// Pay treasury-vault cells to an address.
    TreasuryPayout { amount: u64, to: [u8; 32] },
    /// Replace the dev fund signer.
    RotateDevFundSigner { public: [u8; 32] },
    /// Replace the treasury signer.
    RotateTreasurySigner { public: [u8; 32] },
    /// Set the dev-fee share in basis points.
    SetDevFee { bps: u64 },
    /// Set the coinbase dev-fee share in basis points.
    SetMineFee { bps: u64 },
    /// Freeze one or more operation classes until a fixed height without
    /// stopping the chain: blocks keep arriving — coinbase, slot, and fee
    /// claims all still execute — while the flagged classes are refused
    /// in every block. The freeze takes effect from the block after the
    /// carrying one, so every execution path reads the same pre-block
    /// state and the pause cannot activate mid-block. A nonzero flag set
    /// and a future height are required, so the payload can never request
    /// a transition that would be inert or already expired.
    EmergencyPause { flags: u8, until_height: u64 },
    /// Clear the emergency pause outright, before its scheduled expiry.
    EmergencyResume,
}

/// Provenance of cells issued by governance mints. The bonding curve
/// funds sell payouts exclusively from payments buyers made into the
/// treasury vault; a governance mint moves cells to a recipient without
/// any such payment, so those cells must never acquire buy-back rights:
/// without provenance a coalition able to pass TreasuryMint proposals
/// could mint to an accomplice and sell the cells back at the curve sell
/// price, draining the vault with no purchase ever occurring. The set
/// stores merged, non-overlapping half-open ranges keyed by start cell, so
/// membership queries scan at most one predecessor entry plus one
/// successor entry regardless of how many mints the chain has executed.
/// Provenance never expires: a transferred cell keeps it, exactly like
/// physical provenance. The set rides the governance state in the state
/// header, so it persists and replays with every snapshot and journal.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MintProvenance(pub BTreeMap<u64, u64>);

impl MintProvenance {
    /// Record a governance-minted range. Overlapping and adjacent entries
    /// are merged into a single span, so repeated mints into neighboring
    /// ranges keep the map compact and queries stay bounded.
    pub fn mark(&mut self, start: u64, end: u64) {
        if end <= start {
            return;
        }
        let mut s = start;
        let mut e = end;
        // Absorb the entry immediately to the left when it overlaps the
        // new range; the span extends from that entry's start.
        if let Some((&ls, &le)) = self.0.range(..=s).next_back() {
            if le >= s {
                s = ls;
                e = e.max(le);
            }
        }
        // Absorb every entry whose start falls inside the merged span:
        // each overlaps it and can only extend its end.
        let absorbed: Vec<(u64, u64)> = self.0.range(s..e).map(|(&k, &v)| (k, v)).collect();
        for (k, v) in absorbed {
            e = e.max(v);
            self.0.remove(&k);
        }
        self.0.insert(s, e);
    }

    /// Whether any cell of the half-open range carries governance-mint
    /// provenance. The predecessor entry is checked for overlap with the
    /// range start; any entry starting inside the range covers the rest.
    pub fn contains(&self, start: u64, end: u64) -> bool {
        if end <= start {
            return false;
        }
        if let Some((_, le)) = self.0.range(..=start).next_back() {
            if *le > start {
                return true;
            }
        }
        self.0.range(start..end).next().is_some()
    }
}

/// Emergency pause flag: freeze treasury curve pairs (buys and sells) in
/// every block until the pause expires. Coinbase, slot mints, and fee
/// claims keep executing, so the chain stays live while the vulnerable
/// surface is closed.
pub const EMERGENCY_TREASURY_PAIRS: u8 = 0b01;
/// Emergency pause flag: freeze every non-coinbase transaction — the
/// broadest circuit breaker, used when the vulnerability lives in user
/// transaction handling itself. Governance proposals already open keep
/// their schedule; only new transactions stop moving.
pub const EMERGENCY_USER_TXS: u8 = 0b10;

/// A governance operation carried by a transaction memo while the DaoV1
/// rule is active. The operation applies only after the carrying
/// transfer itself succeeds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DaoOp {
    SubmitProposal { payload: DaoPayload },
    Vote { proposal_id: u64, approve: bool },
}

/// Locked cells earn DAO voting weight only when the lock expires at
/// least this many blocks past the proposal snapshot: weight is a
/// long-horizon commitment, not a spot purchase.
pub const DAO_LOCK_MIN: u64 = 10_000;

/// Quorum denominator in basis points of the snapshot locked supply.
pub const DAO_QUORUM_BPS: u64 = 500;

/// Approval threshold in basis points of the cast weight.
pub const DAO_THRESHOLD_BPS: u64 = 5_000;

/// Blocks from submission to close.
pub const DAO_VOTING_PERIOD: u64 = 4_032;

/// Blocks from close to executable height.
pub const DAO_EXEC_DELAY: u64 = 1_008;

/// Maximum open proposals. The cap bounds the proposal table and the
/// per-block governance pass.
pub const DAO_MAX_PROPOSALS: usize = 64;

/// Blocks a terminal proposal is kept after its voting period ends before
/// it is pruned. The window covers the scheduled execution height, so
/// every outcome stays observable on-chain; the table the per-block
/// governance pass iterates then stays bounded for the node's whole life.
pub const DAO_TERMINAL_RETENTION: u64 = DAO_VOTING_PERIOD + DAO_EXEC_DELAY;

#[cfg(test)]
mod provenance_tests {
    use super::MintProvenance;

    #[test]
    fn disjoint_marks_query_independently() {
        let mut p = MintProvenance::default();
        p.mark(100, 150);
        p.mark(300, 310);
        assert!(p.contains(100, 150));
        assert!(p.contains(300, 310));
        assert!(p.contains(120, 305));
        assert!(!p.contains(150, 300));
        assert!(!p.contains(0, 100));
        assert!(!p.contains(310, 400));
    }

    #[test]
    fn overlapping_marks_merge_into_one_span() {
        let mut p = MintProvenance::default();
        p.mark(100, 160);
        p.mark(150, 200);
        assert_eq!(p.0.len(), 1);
        assert!(p.contains(100, 200));
        assert_eq!(p.0.get(&100), Some(&200));
    }

    #[test]
    fn enclosing_and_nested_marks_collapse() {
        let mut p = MintProvenance::default();
        p.mark(120, 140);
        p.mark(100, 200);
        p.mark(110, 120);
        assert_eq!(p.0.len(), 1);
        assert_eq!(p.0.get(&100), Some(&200));
        assert!(!p.contains(200, 201));
    }

    #[test]
    fn empty_and_reversed_ranges_are_ignored() {
        let mut p = MintProvenance::default();
        p.mark(50, 50);
        p.mark(60, 40);
        assert!(p.0.is_empty());
        assert!(!p.contains(50, 60));
    }
}
