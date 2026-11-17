//! Shared test fixtures exposed as public library API.
//!
//! Every integration-style test crate and the benchmark suite builds the
//! library with the `test-utils` feature (through the self-referential
//! dev-dependency in `Cargo.toml`) and imports these helpers through one
//! glob. Because the items are public library API, the dead-code lint
//! never fires for the subsets a given crate does not use.
//!
//! Layout:
//! - `blocks` — the nonce search and the block builders
//! - `tx` — the transaction builders and the encrypted-secret helpers
//! - `watchdog` — the wall-clock watchdog that turns an intra-operation
//!   stall in a soak driver into an abort with the last heartbeat context
//!
//! Every helper is deterministic: fixed keys, fixed nonces, fixed
//! timestamps, so a failing test can be replayed bit for bit.

mod blocks;
mod tx;
mod watchdog;

pub use blocks::*;
pub use tx::*;
pub use watchdog::Watchdog;

pub use crate::chain;
pub use crate::crypto;
pub use crate::state;
use std::sync::atomic::Ordering;

/// Process-wide lock serializing the network-mode flag. The flag is
/// process-global, and a test that flips it to testnet must hold the
/// lock for its whole body: a parallel test resetting the flag to
/// mainnet mid-flight would otherwise hand it a mainnet identifier and
/// make the assertion order-dependent. Tests that only ever reset the
/// flag to mainnet do not need the lock — the write is idempotent — but
/// taking it is harmless and keeps the discipline uniform.
pub static TEST_MODE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Process-wide lock serializing working-directory mutation. The working
/// directory is process-global state: a test that removes its temporary
/// tree while a parallel test still reads the working directory hands the
/// other a deleted path, and `current_dir` then fails with a not-found
/// error. Holding one lock for the whole lifetime of every test-directory
/// guard keeps the enter-read-remove sequence strictly serialized, so no
/// test ever observes a directory another test removed.
static TEST_CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

thread_local! {
    /// Re-entrancy depth of the working-directory lock on the current
    /// thread. Some fixtures enter a test directory internally — the
    /// soak node constructor among them — and a test that already holds
    /// the lock would otherwise block on its own non-reentrant mutex.
    /// Counting nesting per thread lets nested acquisitions skip the wait
    /// while the outermost guard keeps the process-wide exclusion.
    static TEST_CWD_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    /// The single OS-level guard behind the depth counter: taken only by
    /// the acquisition that raises the depth from zero and released by
    /// the release that returns it to zero. Guards must drop in stack
    /// order — every fixture uses them as plain locals, and a guard moved
    /// into a longer-lived scope would release the process-wide lock
    /// while a nested guard still depends on it.
    static TEST_CWD_GUARD: std::cell::RefCell<Option<std::sync::MutexGuard<'static, ()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Test directory guard: creates a unique temporary directory, moves the
/// process working directory into it for the duration of one test, and
/// restores the previous working directory (and removes the temporary
/// tree) when the guard drops. All on-chain file access in the library is
/// relative, so isolating the working directory isolates the data. The
/// guard holds the process-wide working-directory lock, so tests using it
/// serialize their filesystem windows — they still interleave freely
/// outside those windows.
pub struct TestDir {
    previous: std::path::PathBuf,
    temp: std::path::PathBuf,
}

impl TestDir {
    fn new(label: &str) -> Self {
        let outermost = TEST_CWD_DEPTH.with(|depth| {
            let current = depth.get();
            depth.set(current + 1);
            current == 0
        });
        if outermost {
            let guard = TEST_CWD_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            TEST_CWD_GUARD.with(|cell| *cell.borrow_mut() = Some(guard));
        }
        let previous = std::env::current_dir().expect("current directory");
        let unique = format!(
            "slash-test-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let temp = std::env::temp_dir().join(unique); // nosemgrep: rust.lang.security.temp-dir -- test-only isolation harness; the directory name is generated unique
        std::fs::create_dir_all(&temp).expect("create test directory");
        std::env::set_current_dir(&temp).expect("enter test directory");
        TestDir { previous, temp }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        // Leave the temporary tree before removing it: a process's own
        // working directory cannot be removed on some platforms, and the
        // restore below must never hand the next holder of the lock a
        // deleted directory. Fall back to the system temp directory,
        // which always exists, then restore the previous directory best
        // effort — if it no longer exists the process stays in the temp
        // directory, which is still a live working directory.
        let _ = std::env::set_current_dir(std::env::temp_dir()); // nosemgrep: rust.lang.security.temp-dir -- test-only isolation harness; the directory name is generated unique
        let _ = std::fs::remove_dir_all(&self.temp);
        let _ = std::env::set_current_dir(&self.previous);
        let remaining = TEST_CWD_DEPTH.with(|depth| {
            let current = depth.get();
            depth.set(current.saturating_sub(1));
            current - 1
        });
        if remaining == 0 {
            TEST_CWD_GUARD.with(|cell| drop(cell.borrow_mut().take()));
        }
    }
}

/// Enter an isolated temporary working directory for the current test.
/// The returned guard restores the original directory on drop.
pub fn setup_test_dir(label: &str) -> TestDir {
    TestDir::new(label)
}

/// Reset the process-wide network flag to mainnet so signature hashing
/// uses the mainnet chain identifier.
pub fn reset_testnet() {
    crate::TESTNET_MODE.store(false, Ordering::SeqCst);
}

/// Set the process-wide network flag to testnet so signature hashing uses
/// the testnet chain identifier.
pub fn enable_testnet() {
    crate::TESTNET_MODE.store(true, Ordering::SeqCst);
}

/// Well-known treasury secret used by fixtures. Only ever used inside
/// tests; a node started with this key would mint on the mainnet.
pub const TREASURY_SK: [u8; 32] = [0x5C; 32];

/// Deterministic test keypair: the secret seed is the byte repeated 32
/// times, and the address is the real Ed25519 public key derived from it,
/// so signatures produced by the transaction builders verify through the
/// ordinary consensus path.
pub fn test_keypair(seed: u8) -> ([u8; 32], [u8; 32]) {
    let sk = [seed; 32];
    let pk = crypto::public_from_secret(&sk);
    (sk, pk)
}

/// Build a genesis chain whose treasury signer is the public half of
/// `TREASURY_SK`, so treasury mints and payouts verify without any
/// additional node configuration. The mutation goes through
/// `Arc::make_mut`: a fixture-owned genesis chain always has a unique
/// reference, so the write happens in place without copying the state.
pub fn treasury_genesis() -> chain::Chain {
    let mut chain = chain::Chain::genesis();
    std::sync::Arc::make_mut(&mut chain.state).treasury_signer =
        crypto::public_from_secret(&TREASURY_SK);
    chain
}

/// Deterministically mined chain prefixes shared across tests.
/// `mine_next_block` stamps block time with the height, so replaying the
/// same miner key and an empty transaction list from genesis reproduces
/// the identical block hashes; caching per (miner, blocks, network mode)
/// pays each RandomX context once per process instead of once per test.
/// The network mode joins the key because the signature hash domain
/// differs between mainnet and testnet.
pub fn shared_mined_chain(miner: [u8; 32], blocks: u64) -> chain::Chain {
    type Cache = std::collections::HashMap<([u8; 32], u64, bool), chain::Chain>;
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Cache>> = std::sync::OnceLock::new();
    let testnet = crate::TESTNET_MODE.load(Ordering::Relaxed);
    let mut cache = CACHE
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .unwrap();
    cache
        .entry((miner, blocks, testnet))
        .or_insert_with(|| {
            let mut chain = chain::Chain::genesis();
            for _ in 0..blocks {
                let block = mine_next_block(&chain, miner, vec![]);
                chain.apply(block).expect("shared prefix block applies");
            }
            chain
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_test_dirs_do_not_deadlock() {
        // A fixture that enters a test directory internally (every soak
        // node constructor does) must stay usable inside a test that
        // already holds one: the process-wide lock is re-entrant per
        // thread through the nesting counter, and the outermost guard
        // still serializes the window against other threads.
        let outer = setup_test_dir("nested-outer");
        let inner = setup_test_dir("nested-inner");
        drop(inner);
        drop(outer);
    }
}
