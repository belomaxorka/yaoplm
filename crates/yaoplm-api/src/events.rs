use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use crate::JobEvent;

type Listener = Arc<dyn Fn(&JobEvent) + Send + Sync>;

#[derive(Default)]
struct Listeners {
    next_id: u64,
    items: Vec<(u64, Listener)>,
}

/// Fans job events out to front-end subscribers.
///
/// Listeners run on the publishing thread, so they must return quickly:
/// forward the event to a channel or socket and nothing more. Clones share
/// the listeners.
#[derive(Default, Clone)]
pub struct EventBus {
    inner: Arc<Mutex<Listeners>>,
}

impl EventBus {
    pub fn subscribe(&self, listener: impl Fn(&JobEvent) + Send + Sync + 'static) -> Subscription {
        let mut listeners = lock(&self.inner);
        let id = listeners.next_id;
        listeners.next_id += 1;
        listeners.items.push((id, Arc::new(listener)));
        Subscription {
            id,
            inner: Arc::downgrade(&self.inner),
        }
    }

    pub fn publish(&self, event: &JobEvent) {
        // Call listeners outside the lock so they may (un)subscribe.
        let listeners: Vec<Listener> = lock(&self.inner)
            .items
            .iter()
            .map(|(_, listener)| Arc::clone(listener))
            .collect();
        for listener in listeners {
            listener(event);
        }
    }

    pub fn subscriber_count(&self) -> usize {
        lock(&self.inner).items.len()
    }
}

impl fmt::Debug for EventBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventBus")
            .field("subscribers", &self.subscriber_count())
            .finish()
    }
}

/// Keeps a listener registered until dropped.
#[must_use = "dropping a Subscription unsubscribes immediately"]
pub struct Subscription {
    id: u64,
    inner: Weak<Mutex<Listeners>>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            lock(&inner).items.retain(|(id, _)| *id != self.id);
        }
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription")
            .field("id", &self.id)
            .finish()
    }
}

// A panicking listener must not take the bus down for everyone else.
fn lock(mutex: &Mutex<Listeners>) -> MutexGuard<'_, Listeners> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::JobId;

    #[test]
    fn delivers_until_subscription_is_dropped() {
        let bus = EventBus::default();
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&seen);
        let sub = bus.subscribe(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        bus.publish(&JobEvent::Started { job: JobId(1) });
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        assert_eq!(bus.subscriber_count(), 1);

        drop(sub);
        bus.publish(&JobEvent::Finished { job: JobId(1) });
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        assert_eq!(bus.subscriber_count(), 0);
    }

    #[test]
    fn subscription_outliving_bus_drops_cleanly() {
        let bus = EventBus::default();
        let sub = bus.subscribe(|_| {});
        drop(bus);
        drop(sub);
    }
}
