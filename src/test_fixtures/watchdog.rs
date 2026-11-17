//! Wall-clock watchdog for long-running soak drivers.
//!
//! Soak drivers sample a wall-clock budget only between operations, so
//! a driver that stalls inside a single operation — a selection scan
//! that never advances, a nonce grind that never finds an acceptable
//! slot — otherwise holds the test binary silent for hours while the
//! budget check keeps seeing a fresh clock. `Watchdog` closes that gap:
//! a background thread aborts the process with the last heartbeat
//! context when no progress is reported for longer than the budget.
//! Aborting (rather than panicking on the watchdog thread) is
//! deliberate: the stalled operation runs on the test thread, and a
//! panic anywhere else cannot interrupt it. This is a development and
//! CI aid for test code; production paths never construct a watchdog.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Shared {
    stop: AtomicBool,
    last_beat: Mutex<(Instant, String)>,
}

/// Aborts the process after `budget` of silence between heartbeats.
pub struct Watchdog {
    shared: Arc<Shared>,
    budget: Duration,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Watchdog {
    /// Start a watchdog that tolerates at most `budget` between two
    /// consecutive heartbeats. Every soak operation must report progress
    /// at least this often, so size the budget for the slowest
    /// legitimate operation, not for the whole run.
    pub fn start(budget: Duration) -> Self {
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            last_beat: Mutex::new((Instant::now(), String::from("watchdog start"))),
        });
        let worker = {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(100));
                if shared.stop.load(Ordering::SeqCst) {
                    return;
                }
                let (last, detail) = {
                    let guard = shared
                        .last_beat
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    (guard.0, guard.1.clone())
                };
                if last.elapsed() > budget {
                    eprintln!(
                        "watchdog: no soak progress for {:?} (budget {:?}); \
                         last heartbeat: {detail}",
                        last.elapsed(),
                        budget,
                    );
                    eprintln!(
                        "watchdog: rerun the failing test under a debugger \
                         (rust-gdb / lldb) and interrupt the process to \
                         capture the stalled stack"
                    );
                    use std::io::Write;
                    let _ = std::io::stderr().flush();
                    std::process::abort();
                }
            })
        };
        Watchdog {
            shared,
            budget,
            worker: Some(worker),
        }
    }

    /// Report progress and record a short description of the operation
    /// that completed (or is about to start) as the abort diagnostic.
    pub fn beat(&self, detail: impl Into<String>) {
        let mut guard = self
            .shared
            .last_beat
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = (Instant::now(), detail.into());
    }

    /// The per-operation budget this watchdog enforces.
    pub fn budget(&self) -> Duration {
        self.budget
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_and_drop_stop_the_worker_cleanly() {
        let watchdog = Watchdog::start(Duration::from_secs(3600));
        watchdog.beat("first operation");
        watchdog.beat(String::from("second operation"));
        assert_eq!(watchdog.budget(), Duration::from_secs(3600));
        drop(watchdog);
    }

    #[test]
    fn heartbeat_records_the_latest_detail() {
        let watchdog = Watchdog::start(Duration::from_secs(3600));
        watchdog.beat("earlier");
        watchdog.beat("latest");
        let (_, detail) = watchdog
            .shared
            .last_beat
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert_eq!(detail, "latest");
        drop(watchdog);
    }
}
