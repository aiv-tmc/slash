//! The authoritative on-chain DAO state and the proposal lifecycle:
//! submission under the open-proposal cap with the quorum denominator
//! captured at submission, first-vote-wins voting, snapshot-pinned
//! tallying, close, execution, and bounded retention of terminal records.

use super::payload::{execute_payload, payload_valid};
use super::types::{
    DaoOp, DaoPayload, MintProvenance, Proposal, ProposalStatus, DAO_MAX_PAUSE_BLOCKS,
    DAO_MAX_PROPOSALS, DAO_TERMINAL_RETENTION, DAO_VOTING_PERIOD,
};
use super::weight::{close_outcome, locked_weight, total_locked_weight};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Authoritative on-chain DAO state, persisted as part of the state header.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DaoState {
    /// Live and recently terminal proposals, keyed by id. Terminal
    /// proposals survive for a bounded retention window so their outcome
    /// stays observable and auditable on-chain, and are then removed, so
    /// the table the per-block governance pass iterates stays bounded for
    /// the node's whole life. Ids are never reused: the monotonically
    /// increasing `next_id` only ever moves forward.
    pub proposals: BTreeMap<u64, Proposal>,
    /// Next proposal id to assign.
    pub next_id: u64,
    /// Provenance of every cell a governance mint ever issued. The bonding curve
    /// funds sell payouts exclusively from payments buyers made into the
    /// treasury vault; a governance mint moved these cells out without
    /// any such payment, so they carry no buy-back rights, and the
    /// transfer engine refuses any sell of them back to the treasury
    /// through this single set.
    pub minted: MintProvenance,
}

impl DaoState {
    /// Empty DAO state: no proposals, the first id reserved, and no
    /// governance-minted provenance recorded.
    pub fn genesis() -> Self {
        Self {
            proposals: BTreeMap::new(),
            next_id: 1,
            minted: MintProvenance::default(),
        }
    }

    /// Count proposals that are currently open for votes.
    pub fn open_count(&self) -> usize {
        self.proposals
            .values()
            .filter(|p| p.status == ProposalStatus::Voting)
            .count()
    }

    /// Submit a proposal, capturing the quorum denominator at the moment
    /// the proposal opens. `locked_total` is the total locked weight at
    /// the submission height, supplied by the caller; the value rides the
    /// record so closing the proposal later never rescans the ledger.
    /// Returns None when the open proposal cap is reached, the payload
    /// is structurally invalid, or the requested pause reaches past the
    /// horizon a submission may bind.
    pub fn submit(
        &mut self,
        payload: DaoPayload,
        submitter: [u8; 32],
        height: u64,
        locked_total: u64,
    ) -> Option<u64> {
        if self.open_count() >= DAO_MAX_PROPOSALS {
            return None;
        }
        if !payload_valid(&payload) {
            return None;
        }
        // The pause horizon is bounded on both sides relative to the
        // submission height: a freeze that has already expired at
        // submission is an inert proposal, and one reaching past the
        // bound could outlive any plausible quorum and deadlock the
        // frozen classes permanently, while a class that needs longer
        // protection can be re-paused by a fresh proposal. The bounds
        // live here rather than in the shape validator because only the
        // submission knows the height the horizon must be measured from.
        if let DaoPayload::EmergencyPause { until_height, .. } = &payload {
            if *until_height <= height || *until_height > height.saturating_add(DAO_MAX_PAUSE_BLOCKS)
            {
                return None;
            }
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let proposal = Proposal {
            id,
            payload,
            submitter,
            start_height: height,
            end_height: height + DAO_VOTING_PERIOD,
            snapshot_height: height,
            locked_total,
            yes_weight: 0,
            no_weight: 0,
            voters: std::collections::BTreeSet::new(),
            status: ProposalStatus::Voting,
            exec_height: 0,
        };
        self.proposals.insert(id, proposal);
        Some(id)
    }

    /// Record a vote. The voter's weight is supplied by the caller, which
    /// evaluates the locked-cell snapshot at the proposal snapshot height.
    /// Returns false when the proposal is not open, the address already
    /// voted, or the weight is zero.
    pub fn vote(
        &mut self,
        proposal_id: u64,
        voter: [u8; 32],
        weight: u64,
        approve: bool,
        height: u64,
    ) -> bool {
        if weight == 0 {
            return false;
        }
        let Some(p) = self.proposals.get_mut(&proposal_id) else {
            return false;
        };
        if p.status != ProposalStatus::Voting {
            return false;
        }
        if height < p.start_height || height >= p.end_height {
            return false;
        }
        if !p.voters.insert(voter) {
            return false;
        }
        if approve {
            p.yes_weight = p.yes_weight.saturating_add(weight);
        } else {
            p.no_weight = p.no_weight.saturating_add(weight);
        }
        true
    }

    /// Tally every proposal whose voting window has ended at `height`,
    /// against the quorum denominator captured at submission, so the tally
    /// is a pure function of the state as it stood when the proposal
    /// opened and performs no ledger scans. Passed proposals sit in the
    /// queue for the execution delay so voters can observe the outcome
    /// before any state change.
    pub fn tally_at(&mut self, height: u64) {
        for p in self.proposals.values_mut() {
            if p.status != ProposalStatus::Voting || height < p.end_height {
                continue;
            }
            let (status, exec_height) = close_outcome(p, p.locked_total);
            p.status = status;
            p.exec_height = exec_height;
        }
    }

    /// The per-block governance tick the chain driver runs: tally whatever
    /// has closed and retire terminal records past the retention window.
    pub fn close_due(&mut self, height: u64) {
        self.tally_at(height);
        self.prune_terminal(height);
    }

    /// Execute every passed proposal that reached its executable height.
    /// Execution flows through the same state transition engine as every
    /// other economic primitive; a governance mint additionally pins the
    /// funded ranges into the provenance set so the minted cells can
    /// never be sold back through the curve. Marks commit only after
    /// post-execution ownership confirms the engine moved exactly the
    /// reserved cells to the payload target, so the provenance set can
    /// never diverge from the ledger. Unexecutable payloads expire
    /// instead of blocking the queue. After the pass, terminal proposals
    /// older than the retention window are pruned.
    pub fn execute_due(&mut self, state: &mut crate::state::State, height: u64) {
        let due: Vec<u64> = self
            .proposals
            .values()
            .filter(|p| p.status == ProposalStatus::Passed && height >= p.exec_height)
            .map(|p| p.id)
            .collect();
        for id in due {
            let payload = match self.proposals.get(&id) {
                Some(p) => p.payload.clone(),
                None => continue,
            };
            let ok = match &payload {
                DaoPayload::TreasuryMint { amount, to } => {
                    // Reserve the main funding range on the pre-execution
                    // state: canonical selection is deterministic, so the
                    // reservation equals the range the mint consumes.
                    let main = state.select_single(crate::state::TREASURY, *amount);
                    let ok = execute_payload(state, &payload, height);
                    if ok {
                        // Pin the main range only when ownership confirms
                        // the mint moved exactly those cells to the target.
                        if let Some((s, e)) = main {
                            if !state.owns(crate::state::TREASURY, s, e, height)
                                && state.owns(*to, s, e, height)
                            {
                                self.minted.mark(s, e);
                            }
                        }
                        // The dev-share mint selects on the post-main
                        // state; reserve and verify it the same way.
                        let dev = amount.saturating_mul(state.dev_fee_bps) / 10_000;
                        if dev > 0 {
                            if let Some((ds, de)) = state.select_single(crate::state::TREASURY, dev)
                            {
                                if !state.owns(crate::state::TREASURY, ds, de, height)
                                    && state.owns(crate::state::DEV_FUND, ds, de, height)
                                {
                                    self.minted.mark(ds, de);
                                }
                            }
                        }
                    }
                    ok
                }
                _ => execute_payload(state, &payload, height),
            };
            let p = self.proposals.get_mut(&id).expect("id collected above");
            p.status = if ok {
                ProposalStatus::Executed
            } else {
                ProposalStatus::Expired
            };
        }
        self.prune_terminal(height);
    }

    /// Drop terminal proposals whose retention window has passed. The
    /// window keeps every finished proposal observable on-chain long
    /// enough to audit its outcome and then releases the record, so the
    /// proposal set stays bounded for the node's whole life. Pruning is a
    /// pure function of the proposals and the height, so every node drops
    /// the same ids at the same height; votes for a pruned id find no
    /// proposal and are dropped silently, like votes for an unknown id.
    fn prune_terminal(&mut self, height: u64) {
        let dead: Vec<u64> = self
            .proposals
            .values()
            .filter(|p| {
                p.status != ProposalStatus::Voting
                    && p.status != ProposalStatus::Passed
                    && height > p.end_height.saturating_add(DAO_TERMINAL_RETENTION)
            })
            .map(|p| p.id)
            .collect();
        for id in dead {
            self.proposals.remove(&id);
        }
    }

    /// Apply one governance operation carried by a transaction memo. The
    /// caller has already executed the transfer itself, so a rejected
    /// operation never invalidates the transaction: the transfer stands
    /// and the operation is silently dropped. Voting weight is evaluated
    /// at the target proposal's snapshot height. A submission evaluates
    /// the quorum denominator against the state the carrying block has
    /// built so far, which is exactly the state every other node sees at
    /// the same position in the same block.
    pub fn apply_op(
        &mut self,
        state: &crate::state::State,
        voter: [u8; 32],
        op: &DaoOp,
        height: u64,
    ) -> bool {
        match op {
            DaoOp::SubmitProposal { payload } => {
                let locked_total = total_locked_weight(state, height);
                self.submit(payload.clone(), voter, height, locked_total)
                    .is_some()
            }
            DaoOp::Vote {
                proposal_id,
                approve,
            } => {
                let snapshot = self
                    .proposals
                    .get(proposal_id)
                    .map(|p| p.snapshot_height)
                    .unwrap_or(height);
                let weight = locked_weight(state, voter, snapshot);
                self.vote(*proposal_id, voter, weight, *approve, height)
            }
        }
    }
}

#[cfg(test)]
mod submission_tests {
    use super::*;

    #[test]
    fn pause_horizon_rejects_binds_past_the_bound() {
        // A pause reaching past the submission-bound horizon is refused,
        // and the bound is measured from the submission height: the
        // same payload that is in bounds at a late height is refused at an
        // early one, and a payload exactly at the bound is accepted.
        let mut dao = DaoState::genesis();
        let far = height_at_bound(100) + 1;
        let payload = || DaoPayload::EmergencyPause {
            flags: 0b01,
            until_height: far,
        };
        assert!(dao.submit(payload(), [1u8; 32], 100, 0).is_none());
        assert!(dao
            .submit(
                DaoPayload::EmergencyPause {
                    flags: 0b01,
                    until_height: height_at_bound(100),
                },
                [1u8; 32],
                100,
                0,
            )
            .is_some());
    }

    #[test]
    fn pause_horizon_scales_with_submission_height() {
        // The horizon is relative, not absolute: submitting the same
        // absolute height at a later block is accepted once it falls
        // inside the bound of that submission.
        let mut dao = DaoState::genesis();
        let target = 100 + DAO_MAX_PAUSE_BLOCKS;
        assert!(dao
            .submit(
                DaoPayload::EmergencyPause {
                    flags: 0b01,
                    until_height: target,
                },
                [1u8; 32],
                100,
                0,
            )
            .is_some());
    }

    /// The furthest in-bounds horizon for a submission at `height`.
    fn height_at_bound(height: u64) -> u64 {
        height + DAO_MAX_PAUSE_BLOCKS
    }
}
