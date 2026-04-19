//! In-process event bus for IPC subscribers.
//!
//! The App emits lifecycle [`Event`]s (pane started / pane exited /
//! ...) via [`EventBus::emit`]. IPC subscriber connections register
//! via [`EventBus::subscribe`] and stream the events out over the
//! wire.
//!
//! # Delivery semantics (best-effort)
//!
//! Each subscriber has a bounded [`sync_channel`] of
//! [`CHANNEL_CAPACITY`] events. If a subscriber is too slow to
//! drain, new events are **dropped for that subscriber only** and
//! its [`EventBus`] will synthesize an [`Event::EventsDropped`]
//! meta-event on the next successful send so the subscriber can
//! recover awareness of the gap. We never block the App event loop.
//!
//! Because drops can happen, the event stream is **not** a reliable
//! replication source for a subscriber that needs an exact state
//! mirror. It's a live-feed for reactive workflows (e.g. "a worker
//! pane exited, react").
//!
//! Disconnected subscribers (their `Receiver` was dropped) are
//! cleaned up either (a) eagerly on [`EventBus::unsubscribe`] or
//! (b) lazily on the next [`EventBus::emit`] via `try_send`
//! detecting a disconnected sender.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use super::Event;

const CHANNEL_CAPACITY: usize = 256;

/// Opaque handle identifying an individual subscription. Used to
/// explicitly unregister without waiting for the next emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubId(u64);

struct Sub {
    id: SubId,
    tx: SyncSender<Event>,
    dropped_count: u64,
}

/// Multi-producer, multi-consumer event bus. Cheap to clone — the
/// internal subscriber list is `Arc`-shared.
#[derive(Default, Clone)]
pub struct EventBus {
    subs: Arc<Mutex<Vec<Sub>>>,
    next_id: Arc<AtomicU64>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new subscriber. Returns the subscription id plus a
    /// receiver to drain events from. The caller should call
    /// [`unsubscribe`](Self::unsubscribe) when done; otherwise the
    /// bus will reclaim the slot lazily on the next emit.
    pub fn subscribe(&self) -> (SubId, Receiver<Event>) {
        let (tx, rx) = sync_channel(CHANNEL_CAPACITY);
        let id = SubId(self.next_id.fetch_add(1, Ordering::Relaxed));
        if let Ok(mut subs) = self.subs.lock() {
            subs.push(Sub {
                id,
                tx,
                dropped_count: 0,
            });
        }
        (id, rx)
    }

    /// Explicitly remove a subscription. Safe to call even if the
    /// subscriber has already been GC'd by a previous emit.
    pub fn unsubscribe(&self, id: SubId) {
        if let Ok(mut subs) = self.subs.lock() {
            subs.retain(|s| s.id != id);
        }
    }

    /// Broadcast an event to all live subscribers. Slow subscribers
    /// drop the event but stay subscribed (accumulating a count that
    /// is reported via a synthetic `EventsDropped` on the next
    /// successful send). Disconnected subscribers are removed.
    pub fn emit(&self, event: Event) {
        let mut subs = match self.subs.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        subs.retain_mut(|sub| {
            // First, flush any outstanding dropped-count notice.
            if sub.dropped_count > 0 {
                let notice = Event::EventsDropped {
                    count: sub.dropped_count,
                    ts_ms: now_ms(),
                };
                match sub.tx.try_send(notice) {
                    Ok(()) => {
                        sub.dropped_count = 0;
                    }
                    Err(TrySendError::Full(_)) => {
                        // Still too slow; defer the notice.
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        return false;
                    }
                }
            }
            match sub.tx.try_send(event.clone()) {
                Ok(()) => true,
                Err(TrySendError::Full(_)) => {
                    sub.dropped_count = sub.dropped_count.saturating_add(1);
                    true
                }
                Err(TrySendError::Disconnected(_)) => false,
            }
        });
    }

    #[cfg(test)]
    pub fn subscriber_count(&self) -> usize {
        self.subs.lock().map(|s| s.len()).unwrap_or(0)
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(id: usize) -> Event {
        Event::PaneStarted {
            id,
            name: None,
            role: None,
            ts_ms: 0,
        }
    }

    #[test]
    fn emit_reaches_single_subscriber() {
        let bus = EventBus::new();
        let (_id, rx) = bus.subscribe();
        bus.emit(started(1));
        assert_eq!(rx.try_recv().ok(), Some(started(1)));
    }

    #[test]
    fn emit_fans_out_to_multiple_subscribers() {
        let bus = EventBus::new();
        let (_a, rx1) = bus.subscribe();
        let (_b, rx2) = bus.subscribe();
        bus.emit(started(7));
        assert_eq!(rx1.try_recv().ok(), Some(started(7)));
        assert_eq!(rx2.try_recv().ok(), Some(started(7)));
    }

    #[test]
    fn unsubscribe_removes_immediately() {
        let bus = EventBus::new();
        let (id, _rx) = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);
        bus.unsubscribe(id);
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[test]
    fn unsubscribe_of_unknown_id_is_noop() {
        let bus = EventBus::new();
        bus.unsubscribe(SubId(999));
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[test]
    fn dropped_receiver_is_gc_on_next_emit() {
        let bus = EventBus::new();
        let (_a, rx1) = bus.subscribe();
        let (_b, rx2) = bus.subscribe();
        drop(rx2);
        assert_eq!(bus.subscriber_count(), 2);
        bus.emit(started(1));
        let _ = rx1.try_recv();
        assert_eq!(bus.subscriber_count(), 1);
    }

    #[test]
    fn slow_subscriber_surfaces_events_dropped_meta_event() {
        let bus = EventBus::new();
        let (_id, rx) = bus.subscribe();
        // Overflow the channel.
        for i in 0..(CHANNEL_CAPACITY + 5) {
            bus.emit(started(i));
        }
        // Drain the first window of events that fit.
        let mut payload_events = 0;
        while rx.try_recv().is_ok() {
            payload_events += 1;
        }
        assert!(payload_events <= CHANNEL_CAPACITY);
        assert!(payload_events > 0);

        // Next emit should prepend an EventsDropped meta-event with
        // the accumulated drop count, then the real event.
        bus.emit(started(9999));
        let first = rx.try_recv().expect("meta-event");
        match first {
            Event::EventsDropped { count, .. } => {
                assert!(count > 0, "expected non-zero drop count");
            }
            other => panic!("expected EventsDropped, got {other:?}"),
        }
        let second = rx.try_recv().expect("real event");
        assert_eq!(second, started(9999));
    }
}
