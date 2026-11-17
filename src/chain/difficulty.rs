//! Difficulty retargeting: one windowed rescale feeds both the main chain
//! and the header-batch schedule, with the four-fold clamp and the
//! minimum-difficulty floor.

use super::params::{DIFFICULTY_ADJUSTMENT_INTERVAL, TARGET_BLOCK_TIME_SECS};

impl super::chain::Chain {
    /// The retargeted difficulty for an absolute height. The retarget
    /// runs only when `height - 1` opens a new retarget interval; inside
    /// an interval the difficulty is inherited from the previous block.
    /// The rescale compares the measured window time against the target
    /// interval time and clamps the result to a four-fold change per
    /// interval, never below the minimum difficulty of one.
    ///
    /// The measured window is the interval that just ended: the
    /// DIFFICULTY_ADJUSTMENT_INTERVAL blocks before `height`. The block
    /// vector holds every height above `base_height`, so the window ends
    /// at the last retained block and begins one interval earlier; the
    /// boundary guard reflects that the window reads indices up to
    /// `idx - 2`, where `idx = height - base_height`.
    pub fn difficulty_for_height(&self, height: u64) -> u64 {
        let base = self.base_height.max(1);
        if height <= base || self.blocks.is_empty() {
            return 1;
        }
        let idx = height.saturating_sub(self.base_height) as usize;
        if (height - 1).is_multiple_of(DIFFICULTY_ADJUSTMENT_INTERVAL)
            && idx > 1
            && idx - 1 <= self.blocks.len()
        {
            // Measure over the window that ends at the block before
            // `height`: the DIFFICULTY_ADJUSTMENT_INTERVAL blocks of the
            // interval that just closed.
            let window_end = idx - 1;
            let start_idx = window_end.saturating_sub(DIFFICULTY_ADJUSTMENT_INTERVAL as usize);
            let start_time = self.blocks[start_idx].time;
            let end_time = self.blocks[window_end - 1].time;
            let last_diff = self.blocks[window_end - 1].difficulty;
            let actual = end_time.saturating_sub(start_time).max(1) as u128;
            let target = (DIFFICULTY_ADJUSTMENT_INTERVAL * TARGET_BLOCK_TIME_SECS) as u128;
            let raw = (last_diff as u128)
                .saturating_mul(target)
                .saturating_div(actual);
            // The clamps run in u128: on a pathological window the raw
            // rescale can exceed u64 range, and a truncating cast would
            // wrap the value before the clamp could act on it. Clamping
            // in the wider type, capping at u64::MAX, and casting once
            // keeps every window inside [last/4, last*4] with the
            // floor of one.
            let max_difficulty = (last_diff as u128).saturating_mul(4);
            let min_difficulty = ((last_diff as u128) / 4).max(1);
            return raw
                .clamp(min_difficulty, max_difficulty)
                .min(u64::MAX as u128) as u64;
        }
        if let Some(b) = self.blocks[..idx.min(self.blocks.len())].iter().next_back() {
            return b.difficulty;
        }
        1
    }

    /// The difficulty the next block must declare: the retargeted value
    /// for the next height.
    pub fn next_difficulty(&self) -> u64 {
        self.difficulty_for_height(self.tip_height() + 1)
    }
}
