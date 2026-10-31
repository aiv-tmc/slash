//! The consensus mint primitives.
//!
//! Every path that introduces treasury- or vault-owned cells into
//! circulation lives here: the test and simulation backfill `mine`, the
//! proof-of-work slot mint, the signed treasury mint, and the vault
//! payout. All four reassign existing state ranges (they never create
//! value), keep the balance cache in lockstep with the page map, and
//! release locks only together with a successful reassignment: a lock
//! leaves both the expiry map and the creation map at once, so the maps
//! never describe the same cell differently. Every release withdraws
//! the entry's booking from the live-lock index, keeping the index an
//! exact mirror of the lock map. Locked outputs back to the treasury
//! are refused inside the shared transfer engine, so no primitive can
//! route a lock onto the treasury address.
//!
//! Every balance-cache debit is checked against the cache before the
//! page map is touched: on a consistent state the covering entry always
//! holds the debited amount (every credit lands through the same commit
//! path), but a corrupted or hand-built state must refuse the primitive
//! cleanly instead of panicking in debug or wrapping to a bogus
//! near-maximum balance in release.

use super::page::Output;
use super::params::{N, TREASURY, TREASURY_VAULT};
use super::state::State;

impl State {
    /// Seed one treasury-owned cell to a new owner. Refuses cells that
    /// are already user-owned. An expired lock releases through this
    /// path: a different claimant takes the cell and the lock drops
    /// together, while a claimant that already owns the cell drops only
    /// the lock. This is the test and simulation backfill primitive;
    /// consensus minting goes through mint_slot.
    pub fn mine(&mut self, cell: u64, owner: [u8; 32], height: u64) {
        let Some((id, off, o)) = self.get(cell) else {
            return;
        };
        if o != TREASURY {
            if o == owner {
                // The claimant already owns the cell: an expired lock may
                // still release, nothing else moves.
                if let Some(&until) = self.locks.get(&cell) {
                    if until <= height {
                        self.locks.remove(&cell);
                        self.lock_created.remove(&cell);
                        self.lock_index.unbook(until);
                    }
                }
                return;
            }
            // A different claimant takes the cell only through an expired
            // lock: the reassignment and the lock drop happen together,
            // and both balance entries move in lockstep.
            let Some(&until) = self.locks.get(&cell) else {
                return;
            };
            if until > height {
                return;
            }
            // The cache must cover the debit before the page map is
            // touched: on a consistent state the current owner was
            // credited at least this cell, but a desynchronized cache
            // refuses the reassignment instead of wrapping.
            let owner_bal = self.balance_cache.get(&o).copied().unwrap_or(0);
            if owner_bal < 1 {
                return;
            }
            let page = self.get_page_mut(id);
            if !page.remove_range(off, off + 1) {
                return;
            }
            page.insert_output(off, off + 1, owner);
            self.evict_if_pristine(id);
            self.locks.remove(&cell);
            self.lock_created.remove(&cell);
            self.lock_index.unbook(until);
            *self.balance_cache.entry(o).or_insert(0) = owner_bal - 1;
            *self.balance_cache.entry(owner).or_insert(0) += 1;
            return;
        }
        if let Some(&until) = self.locks.get(&cell) {
            if until > height {
                return;
            }
        }
        // The treasury cache entry must cover the debit before the page
        // map is touched: an absent entry stands for the full unissued
        // supply, and a present entry must hold at least one cell while
        // the page map shows this cell still treasury-owned.
        let treasury_bal = self.balance_cache.get(&TREASURY).copied().unwrap_or(N);
        if treasury_bal < 1 {
            return;
        }
        let page = self.get_page_mut(id);
        if !page.remove_range(off, off + 1) {
            return;
        }
        page.insert_output(off, off + 1, owner);
        self.evict_if_pristine(id);
        // The treasury branch reached here only past the live-lock guard
        // above, so an entry still present is an expired registration;
        // the index booking leaves with the entry.
        if let Some(until) = self.locks.remove(&cell) {
            self.lock_index.unbook(until);
        }
        self.lock_created.remove(&cell);
        *self.balance_cache.entry(owner).or_insert(0) += 1;
        *self.balance_cache.entry(TREASURY).or_insert(N) = treasury_bal - 1;
    }

    /// The proof-of-work slot mint: reassign one treasury-owned cell to
    /// the miner. A user cell is never confiscated. A lock that is still
    /// active at the executing height refuses the mint, so a slot can
    /// never confiscate a locked cell or destroy a live lock; the
    /// boolean verdict invalidates the whole header.
    pub fn mint_slot(&mut self, cell: u64, miner: [u8; 32], height: u64) -> bool {
        let Some((id, off, o)) = self.get(cell) else {
            return false;
        };
        if o != TREASURY {
            return false;
        }
        if self.locks.get(&cell).is_some_and(|&until| until > height) {
            return false;
        }
        // The treasury cache entry must cover the debit before the page
        // map is touched, exactly as in the mine backfill above.
        let treasury_bal = self.balance_cache.get(&TREASURY).copied().unwrap_or(N);
        if treasury_bal < 1 {
            return false;
        }
        let page = self.get_page_mut(id);
        if !page.remove_range(off, off + 1) {
            return false;
        }
        page.insert_output(off, off + 1, miner);
        self.evict_if_pristine(id);
        // Past the live-lock guard an entry still present is expired;
        // the index booking leaves with it.
        if let Some(until) = self.locks.remove(&cell) {
            self.lock_index.unbook(until);
        }
        self.lock_created.remove(&cell);
        *self.balance_cache.entry(miner).or_insert(0) += 1;
        *self.balance_cache.entry(TREASURY).or_insert(N) = treasury_bal - 1;
        true
    }

    /// Signed mint: move an arbitrary treasury-owned span to new outputs.
    /// Used by the coinbase path and by the treasury pairing path. The
    /// transfer engine stages every mutation and commits only on full
    /// success, so a failed mint leaves the state untouched; it also
    /// refuses locked outputs to the treasury, so a mint can never route
    /// a lock onto the treasury address.
    pub fn mint(&mut self, inputs: &[(u64, u64)], outputs: &[Output], height: u64) -> bool {
        self.apply_transfer(TREASURY, inputs, outputs, height)
    }

    /// Vault payout: move vault-owned cells to new outputs. Every input
    /// must be owned by TREASURY_VAULT. The transfer engine stages every
    /// mutation and commits only on full success, so a failed payout
    /// leaves the state untouched; it also refuses locked outputs to the
    /// treasury under the same rule as the mint path.
    pub fn payout(&mut self, inputs: &[(u64, u64)], outputs: &[Output], height: u64) -> bool {
        self.apply_transfer(TREASURY_VAULT, inputs, outputs, height)
    }
}
