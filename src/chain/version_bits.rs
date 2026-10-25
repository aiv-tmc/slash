//! Version-bit soft-fork deployments.
//!
//! A deployment is a named bit position in the block version field with a
//! signaling window and a threshold. Signals are counted per retarget
//! window: the counter resets when a block opens a new window, and lock-in
//! requires the threshold inside one completed window, with activation
//! deferred one full interval after lock-in. This mirrors the BIP-9
//! lifecycle and keeps soft-fork politics independent of difficulty
//! retargeting even though both use the same window length.
//!
//! Every built-in start height sits exactly on a retarget boundary (a
//! multiple of the 2016-block window), so the first signaling window of
//! every deployment is complete from its first block onward: no partial
//! window ever exists whose signals the boundary reset could discard,
//! and honest signaling is never wasted between `start_height` and the
//! first boundary.

use serde::{Deserialize, Serialize};

/// Lifecycle states of a version-bit soft-fork deployment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeploymentStatus {
    /// Not yet signaling.
    Defined,
    /// Signaling is being counted inside retarget windows.
    Started,
    /// Threshold reached in one completed window; activation is scheduled.
    LockedIn,
    /// Rules are enforced from this height onward.
    Active,
    /// Timeout passed without lock-in; the deployment is dead.
    Failed,
}

/// A version-bit deployment record. Signaling is counted per retarget
/// window: the counter resets at each window boundary and lock-in requires
/// the threshold inside one completed window, with activation deferred one
/// full interval after lock-in.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Deployment {
    pub name: String,
    pub bit: u32,
    pub start_height: u64,
    pub timeout_height: u64,
    pub threshold: u32,
    pub status: DeploymentStatus,
    pub activation_height: Option<u64>,
    pub signals: u32,
}

/// The set of deployments tracked by the chain. Built-ins cover the dev
/// fund, DAO activation, transaction and block policy soft-forks, and the
/// post-quantum scheme gate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VersionBits {
    pub deployments: Vec<Deployment>,
}

impl VersionBits {
    /// Construct the built-in deployment schedule for a fresh chain.
    pub fn new() -> Self {
        let mk = |name: &str, bit: u32, start: u64, timeout: u64, threshold: u32| Deployment {
            name: name.to_string(),
            bit,
            start_height: start,
            timeout_height: timeout,
            threshold,
            status: DeploymentStatus::Defined,
            activation_height: None,
            signals: 0,
        };
        // All built-ins share the standard 75% window threshold (1512 of
        // 2016). Start heights are exact multiples of the retarget
        // interval (2016), so each deployment's signaling begins at a
        // window boundary and its first window is always complete;
        // timeouts sit far beyond any reachable lock-in moment. The
        // schedule spaces early governance (dev fund, DAO) well before
        // the policy forks so activated rules exist by the time policy
        // transactions appear.
        Self {
            deployments: vec![
                mk("min_tx_v2", 0, 100_800, 400_000, 1512),
                mk("smaller_blocks", 1, 120_960, 420_000, 1512),
                mk("scheme1_ready", 2, 141_120, 440_000, 1512),
                mk("require_v2", 3, 161_280, 460_000, 1512),
                mk("dev_fund", 4, 10_080, 210_000, 1512),
                mk("dao_v1", 5, 20_160, 220_000, 1512),
            ],
        }
    }

    /// Look up a deployment by its human-readable name.
    pub fn by_name(&self, name: &str) -> Option<&Deployment> {
        self.deployments.iter().find(|d| d.name == name)
    }

    /// Look up a deployment by its name and borrow it mutably, so callers
    /// can advance its lifecycle state or signal tally in place.
    pub fn by_name_mut(&mut self, name: &str) -> Option<&mut Deployment> {
        self.deployments.iter_mut().find(|d| d.name == name)
    }
}

impl Default for VersionBits {
    fn default() -> Self {
        Self::new()
    }
}
