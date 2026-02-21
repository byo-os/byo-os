//! Tracked tokio channels with queue depth monitoring.
//!
//! Wraps `tokio::sync::mpsc::unbounded_channel` with automatic depth
//! counting via [`byo::channel::ChannelCounter`].

use std::sync::Arc;

use byo::channel::ChannelCounter;
use tokio::sync::mpsc;

/// Sending half of a tracked unbounded tokio channel.
pub struct TrackedUnboundedSender<T> {
    tx: mpsc::UnboundedSender<T>,
    counter: ChannelCounter,
}

impl<T> Clone for TrackedUnboundedSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            counter: self.counter.clone(),
        }
    }
}

impl<T> TrackedUnboundedSender<T> {
    /// Send a value, incrementing the queue counter on success.
    pub fn send(&self, val: T) -> Result<(), mpsc::error::SendError<T>> {
        self.tx.send(val)?;
        self.counter.inc();
        Ok(())
    }

    /// Access the underlying queue counter.
    pub fn counter(&self) -> &ChannelCounter {
        &self.counter
    }
}

/// Receiving half of a tracked unbounded tokio channel.
pub struct TrackedUnboundedReceiver<T> {
    rx: mpsc::UnboundedReceiver<T>,
    counter: ChannelCounter,
}

impl<T> TrackedUnboundedReceiver<T> {
    /// Async receive, decrementing the queue counter on success.
    pub async fn recv(&mut self) -> Option<T> {
        let val = self.rx.recv().await?;
        self.counter.dec();
        Some(val)
    }

    /// Non-blocking receive, decrementing the queue counter on success.
    pub fn try_recv(&mut self) -> Result<T, mpsc::error::TryRecvError> {
        let val = self.rx.try_recv()?;
        self.counter.dec();
        Ok(val)
    }

    /// Access the underlying queue counter.
    pub fn counter(&self) -> &ChannelCounter {
        &self.counter
    }
}

/// Create a tracked `tokio::sync::mpsc` unbounded channel.
pub fn tracked_unbounded_channel<T>(
    name: impl Into<Arc<str>>,
    high_mark: isize,
    low_mark: isize,
) -> (TrackedUnboundedSender<T>, TrackedUnboundedReceiver<T>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let counter = ChannelCounter::new(name, high_mark, low_mark);
    (
        TrackedUnboundedSender {
            tx,
            counter: counter.clone(),
        },
        TrackedUnboundedReceiver { rx, counter },
    )
}
