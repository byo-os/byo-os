//! Tracked channels with channel depth monitoring and hysteresis warnings.
//!
//! [`tracked_channel`] wraps `std::sync::mpsc` with automatic depth counting.
//! Sends increment, receives decrement. When depth crosses `high_mark`
//! upward a warning fires; when it drops below `low_mark` a recovery
//! info log fires. The hysteresis band prevents log spam.
//!
//! The counter uses `AtomicIsize` so that relaxed-ordering reordering
//! (dec observed before inc) simply produces a briefly negative value
//! instead of wrapping to `usize::MAX` and panicking.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc;

// ---------------------------------------------------------------------------
// ChannelCounter — the core depth tracker
// ---------------------------------------------------------------------------

/// Shared channel depth counter with hysteresis-based warnings.
///
/// Clone is cheap (Arc bumps). All clones share the same counter and
/// alert state.
#[derive(Clone)]
pub struct ChannelCounter {
    len: Arc<AtomicIsize>,
    state: Arc<AlertState>,
}

struct AlertState {
    name: Arc<str>,
    /// True while the queue is in the "high" state.
    high: AtomicBool,
    high_mark: isize,
    low_mark: isize,
}

impl ChannelCounter {
    /// Create a new counter.
    ///
    /// - `name`: label used in log messages (e.g. `"router"`, `"stdin"`)
    /// - `high_mark`: warn when depth rises above this value
    /// - `low_mark`: log recovery when depth drops below this value
    ///
    /// `low_mark` should be less than `high_mark` to create a hysteresis
    /// band that prevents rapid toggling.
    pub fn new(name: impl Into<Arc<str>>, high_mark: isize, low_mark: isize) -> Self {
        debug_assert!(low_mark < high_mark, "low_mark must be < high_mark");
        Self {
            len: Arc::new(AtomicIsize::new(0)),
            state: Arc::new(AlertState {
                name: name.into(),
                high: AtomicBool::new(false),
                high_mark,
                low_mark,
            }),
        }
    }

    /// Increment and warn if crossing the high-water mark.
    pub fn inc(&self) {
        let new = self.len.fetch_add(1, Ordering::Relaxed) + 1;
        if new > self.state.high_mark && !self.state.high.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                channel = &*self.state.name,
                depth = new,
                "channel depth exceeded {} — consumer may be falling behind",
                self.state.high_mark,
            );
        }
    }

    /// Decrement and log recovery if crossing below the low-water mark.
    pub fn dec(&self) {
        let new = self.len.fetch_sub(1, Ordering::Relaxed) - 1;
        if new < self.state.low_mark && self.state.high.swap(false, Ordering::Relaxed) {
            tracing::info!(
                channel = &*self.state.name,
                depth = new,
                "channel depth recovered below {}",
                self.state.low_mark,
            );
        }
    }

    /// Current depth (relaxed read — may be slightly stale or briefly negative).
    pub fn load(&self) -> isize {
        self.len.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// TrackedSender / TrackedReceiver — std::sync::mpsc wrappers
// ---------------------------------------------------------------------------

/// Sending half of a tracked channel. Increments the depth counter on
/// each successful send.
pub struct TrackedSender<T> {
    tx: mpsc::Sender<T>,
    counter: ChannelCounter,
}

impl<T> TrackedSender<T> {
    /// Send a value, incrementing the queue counter on success.
    pub fn send(&self, val: T) -> Result<(), mpsc::SendError<T>> {
        self.tx.send(val)?;
        self.counter.inc();
        Ok(())
    }

    /// Access the underlying queue counter.
    pub fn counter(&self) -> &ChannelCounter {
        &self.counter
    }
}

impl<T> Clone for TrackedSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            counter: self.counter.clone(),
        }
    }
}

/// Receiving half of a tracked channel. Decrements the depth counter on
/// each successful receive.
pub struct TrackedReceiver<T> {
    rx: mpsc::Receiver<T>,
    counter: ChannelCounter,
}

impl<T> TrackedReceiver<T> {
    /// Blocking receive, decrementing the queue counter on success.
    pub fn recv(&self) -> Result<T, mpsc::RecvError> {
        let val = self.rx.recv()?;
        self.counter.dec();
        Ok(val)
    }

    /// Non-blocking receive, decrementing the queue counter on success.
    pub fn try_recv(&self) -> Result<T, mpsc::TryRecvError> {
        let val = self.rx.try_recv()?;
        self.counter.dec();
        Ok(val)
    }

    /// Blocking receive with timeout, decrementing the queue counter on success.
    pub fn recv_timeout(&self, timeout: std::time::Duration) -> Result<T, mpsc::RecvTimeoutError> {
        let val = self.rx.recv_timeout(timeout)?;
        self.counter.dec();
        Ok(val)
    }

    /// Access the underlying queue counter.
    pub fn counter(&self) -> &ChannelCounter {
        &self.counter
    }
}

/// Create a tracked `std::sync::mpsc` channel.
///
/// Returns `(TrackedSender, TrackedReceiver)` that automatically maintain
/// a channel depth counter with hysteresis warnings.
pub fn tracked_channel<T>(
    name: impl Into<Arc<str>>,
    high_mark: isize,
    low_mark: isize,
) -> (TrackedSender<T>, TrackedReceiver<T>) {
    let (tx, rx) = mpsc::channel();
    let counter = ChannelCounter::new(name, high_mark, low_mark);
    (
        TrackedSender {
            tx,
            counter: counter.clone(),
        },
        TrackedReceiver { rx, counter },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_inc_dec() {
        let q = ChannelCounter::new("test", 100, 50);
        assert_eq!(q.load(), 0);
        q.inc();
        q.inc();
        assert_eq!(q.load(), 2);
        q.dec();
        assert_eq!(q.load(), 1);
        q.dec();
        assert_eq!(q.load(), 0);
    }

    #[test]
    fn high_mark_triggers_and_recovers() {
        let q = ChannelCounter::new("test", 3, 1);
        // Fill to 4 (above high_mark=3)
        for _ in 0..4 {
            q.inc();
        }
        assert!(q.state.high.load(Ordering::Relaxed));

        // Drain to 2 — above low_mark=1, should still be high
        q.dec();
        q.dec();
        assert_eq!(q.load(), 2);
        assert!(q.state.high.load(Ordering::Relaxed));

        // Drain to 0 — below low_mark, should recover
        q.dec();
        q.dec();
        assert_eq!(q.load(), 0);
        assert!(!q.state.high.load(Ordering::Relaxed));
    }

    #[test]
    fn clone_shares_state() {
        let q1 = ChannelCounter::new("test", 10, 5);
        let q2 = q1.clone();
        q1.inc();
        q2.inc();
        assert_eq!(q1.load(), 2);
        assert_eq!(q2.load(), 2);
    }

    #[test]
    fn tracked_channel_counts() {
        let (tx, rx) = tracked_channel::<u32>("test", 100, 50);
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        assert_eq!(tx.counter().load(), 2);

        let _ = rx.try_recv().unwrap();
        assert_eq!(rx.counter().load(), 1);

        let _ = rx.try_recv().unwrap();
        assert_eq!(rx.counter().load(), 0);
    }

    #[test]
    fn from_string_name() {
        let name = format!("write-{}", "app1");
        let q = ChannelCounter::new(name, 100, 50);
        q.inc();
        assert_eq!(q.load(), 1);
    }
}
