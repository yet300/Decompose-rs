//! Synchronous, single-threaded observable state with shared snapshots.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

struct Observer<T> {
    callback: Box<dyn Fn(Rc<T>)>,
    active: Cell<bool>,
    ready: Cell<bool>,
}

struct Source<T> {
    current: RefCell<Rc<T>>,
    observers: RefCell<Vec<Rc<Observer<T>>>>,
    emitting: Cell<bool>,
}

/// A read-only handle to observable state.
///
/// Cloning this handle shares the source without cloning `T`. Observers receive
/// `Rc` snapshots synchronously, and may subscribe, cancel, or update the source
/// from their callbacks. This handle is single-threaded (`!Send` and `!Sync`).
pub struct Value<T> {
    source: Rc<Source<T>>,
}

impl<T> Clone for Value<T> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
        }
    }
}

impl<T> Value<T> {
    /// Returns the latest shared snapshot, without cloning its payload.
    pub fn get(&self) -> Rc<T> {
        self.source.current.borrow().clone()
    }
}

impl<T: 'static> Value<T> {
    /// Registers an observer and synchronously replays the latest snapshot.
    ///
    /// Replay repeats if the callback changes the source, until it catches up.
    /// Updates made during an emission round are conflated to the latest
    /// snapshot for the next round. A subscriber added during emission may
    /// therefore receive the latest snapshot twice, but never receives an older
    /// snapshot after its replay. Separate calls register separate observers.
    ///
    /// Keep the returned token alive, or call [`Subscription::detach`] to leave
    /// the observer registered for the source's lifetime. If replay panics,
    /// registration is rolled back and the panic propagates.
    pub fn subscribe(&self, callback: impl Fn(Rc<T>) + 'static) -> Subscription {
        let observer = Rc::new(Observer {
            callback: Box::new(callback),
            active: Cell::new(true),
            ready: Cell::new(false),
        });
        self.source.observers.borrow_mut().push(observer.clone());

        let source = Rc::downgrade(&self.source);
        let target = Rc::downgrade(&observer);
        // Construct the cancellation guard before replay, so unwinding cannot
        // leave an observer whose caller never received a cancellation token.
        let subscription = Subscription {
            cancellation: RefCell::new(Some(Box::new(move || {
                let Some(observer) = target.upgrade() else {
                    return;
                };
                observer.active.set(false);
                if let Some(source) = source.upgrade() {
                    let removed = {
                        let mut observers = source.observers.borrow_mut();
                        observers
                            .iter()
                            .position(|entry| Rc::ptr_eq(entry, &observer))
                            .map(|index| observers.remove(index))
                    };
                    // Captured values may run arbitrary Drop implementations.
                    drop(removed);
                }
            }))),
        };

        loop {
            let value = self.get();
            (observer.callback)(value.clone());
            if Rc::ptr_eq(&value, &self.get()) {
                observer.ready.set(true);
                break;
            }
        }
        subscription
    }
}

/// A writable handle to synchronous observable state.
///
/// Clones share the source, and no operation requires `T: Clone`. Each update
/// immediately replaces the current snapshot. Notifications are synchronous;
/// nested updates finish the current notification round and then deliver only
/// the latest snapshot. Snapshot identity, rather than payload equality,
/// determines whether another round is needed.
pub struct MutableValue<T> {
    value: Value<T>,
}

impl<T> Clone for MutableValue<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
        }
    }
}

impl<T> MutableValue<T> {
    /// Creates a source containing the supplied payload.
    pub fn new(value: T) -> Self {
        Self::from_rc(Rc::new(value))
    }

    /// Creates a source using an existing shared snapshot.
    pub fn from_rc(value: Rc<T>) -> Self {
        Self {
            value: Value {
                source: Rc::new(Source {
                    current: RefCell::new(value),
                    observers: RefCell::new(Vec::new()),
                    emitting: Cell::new(false),
                }),
            },
        }
    }

    /// Returns a read-only handle to this source.
    pub fn value(&self) -> Value<T> {
        self.value.clone()
    }

    /// Returns the latest shared snapshot, without cloning its payload.
    pub fn get(&self) -> Rc<T> {
        self.value.get()
    }

    /// Replaces the current payload and notifies observers.
    pub fn set(&self, value: T) {
        self.set_rc(Rc::new(value));
    }

    /// Replaces the current snapshot and notifies observers.
    ///
    /// Assigning the same `Rc` still starts a notification round when idle.
    /// Observer panics propagate and interrupt that round; the current snapshot
    /// stays committed and the source remains usable for subsequent updates.
    pub fn set_rc(&self, value: Rc<T>) {
        let source = &self.value.source;
        let previous = source.current.replace(value);
        if source.emitting.replace(true) {
            drop(previous);
            return;
        }

        struct EmissionGuard<'a>(&'a Cell<bool>);
        impl Drop for EmissionGuard<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _guard = EmissionGuard(&source.emitting);
        drop(previous);

        loop {
            let value = self.get();
            let observers: Vec<_> = source
                .observers
                .borrow()
                .iter()
                .filter(|observer| observer.ready.get())
                .cloned()
                .collect();
            // No source borrow is held while invoking user code.
            for observer in observers {
                if observer.active.get() {
                    (observer.callback)(value.clone());
                }
            }
            if Rc::ptr_eq(&value, &self.get()) {
                return;
            }
        }
    }
}

/// An observer registration that cancels automatically when dropped.
///
/// The token holds weak references to the source and observer, so keeping it
/// alive does not keep either alive. Cancellation is idempotent.
#[must_use = "dropping a subscription immediately cancels its observer"]
pub struct Subscription {
    cancellation: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl Subscription {
    pub(crate) fn from_cancel(cancel: impl FnOnce() + 'static) -> Self {
        Self {
            cancellation: RefCell::new(Some(Box::new(cancel))),
        }
    }
    /// Cancels this registration. Further calls have no effect.
    ///
    /// Cancellation also prevents callbacks captured for a pending emission
    /// round from running, unless the callback is already executing.
    pub fn cancel(&self) {
        let cancellation = self.cancellation.borrow_mut().take();
        if let Some(cancellation) = cancellation {
            cancellation();
        }
    }

    /// Leaves the observer registered until the source is dropped.
    ///
    /// Consumes this token without canceling. Capturing a strong source handle
    /// in a detached observer creates an ownership cycle; use a weak capture
    /// when the observer needs to refer back to its source.
    pub fn detach(self) {
        self.cancellation.borrow_mut().take();
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    #[test]
    fn handles_and_snapshots_do_not_require_clone_payloads() {
        struct Payload(i32);
        let initial = Rc::new(Payload(1));
        let source = MutableValue::from_rc(initial.clone());
        let writer = source.clone();
        let reader = source.value().clone();
        assert!(Rc::ptr_eq(&reader.get(), &initial));
        writer.set(Payload(2));
        assert_eq!(reader.get().0, 2);
        assert_eq!(initial.0, 1);
    }

    #[test]
    fn subscriptions_replay_synchronously_and_deliver_same_reference_assignments() {
        let source = MutableValue::new(0);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        let _token = source
            .value()
            .subscribe(move |value| output.borrow_mut().push(*value));
        assert_eq!(*seen.borrow(), [0]);
        source.set(1);
        source.set_rc(source.get());
        assert_eq!(*seen.borrow(), [0, 1, 1]);
    }

    #[test]
    fn cancel_and_drop_stop_notifications() {
        let source = MutableValue::new(0);
        let seen = Rc::new(Cell::new(0));
        let output = seen.clone();
        let token = source
            .value()
            .subscribe(move |_| output.set(output.get() + 1));
        token.cancel();
        token.cancel();
        source.set(1);
        assert_eq!(seen.get(), 1);
        let output = seen.clone();
        let token = source
            .value()
            .subscribe(move |_| output.set(output.get() + 1));
        drop(token);
        source.set(2);
        assert_eq!(seen.get(), 2);
    }

    #[test]
    fn detach_keeps_observer_until_source_is_dropped() {
        let source = MutableValue::new(0);
        let capture = Rc::new(Cell::new(0));
        let weak = Rc::downgrade(&capture);
        source
            .value()
            .subscribe(move |_| capture.set(capture.get() + 1))
            .detach();
        source.set(1);
        assert_eq!(weak.upgrade().unwrap().get(), 2);
        drop(source);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn subscription_does_not_keep_source_or_observers_alive() {
        let source = MutableValue::new(0);
        let capture = Rc::new(());
        let weak = Rc::downgrade(&capture);
        let token = source.value().subscribe(move |_| {
            let _ = &capture;
        });
        drop(source);
        assert!(weak.upgrade().is_none());
        token.cancel();
    }

    #[test]
    fn cancellation_skips_callbacks_already_captured_for_a_round() {
        let source = MutableValue::new(0);
        let pending = Rc::new(RefCell::new(None::<Subscription>));
        let target = pending.clone();
        let _first = source.value().subscribe(move |value| {
            if *value == 1 {
                target.borrow().as_ref().unwrap().cancel();
            }
        });
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        *pending.borrow_mut() = Some(
            source
                .value()
                .subscribe(move |value| output.borrow_mut().push(*value)),
        );
        source.set(1);
        assert_eq!(*seen.borrow(), [0]);
    }

    #[test]
    fn observer_can_cancel_itself() {
        let source = MutableValue::new(0);
        let token = Rc::new(RefCell::new(None::<Subscription>));
        let cancellation = token.clone();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        *token.borrow_mut() = Some(source.value().subscribe(move |value| {
            output.borrow_mut().push(*value);
            if *value == 1 {
                cancellation.borrow().as_ref().unwrap().cancel();
            }
        }));
        source.set(1);
        source.set(2);
        assert_eq!(*seen.borrow(), [0, 1]);
    }

    #[test]
    fn reentrant_writes_conflate_after_finishing_current_round() {
        let source = MutableValue::new(0);
        let writer = source.clone();
        let first_seen = Rc::new(RefCell::new(Vec::new()));
        let output = first_seen.clone();
        let _first = source.value().subscribe(move |value| {
            output.borrow_mut().push(*value);
            if *value == 1 {
                writer.set(2);
                writer.set(3);
            }
        });
        let second_seen = Rc::new(RefCell::new(Vec::new()));
        let output = second_seen.clone();
        let reader = source.value();
        let _second = source.value().subscribe(move |value| {
            output.borrow_mut().push((*value, *reader.get()));
        });
        source.set(1);
        assert_eq!(*first_seen.borrow(), [0, 1, 3]);
        assert_eq!(*second_seen.borrow(), [(0, 0), (1, 3), (3, 3)]);
    }

    #[test]
    fn replay_converges_when_observer_updates_source() {
        let source = MutableValue::new(0);
        let writer = source.clone();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        let _token = source.value().subscribe(move |value| {
            output.borrow_mut().push(*value);
            if *value == 0 {
                writer.set(1);
                writer.set(2);
            }
        });
        assert_eq!(*seen.borrow(), [0, 2]);
        source.set(3);
        assert_eq!(*seen.borrow(), [0, 2, 3]);
    }

    #[test]
    fn subscriber_added_during_emission_never_receives_stale_snapshot() {
        let source = MutableValue::new(0);
        let writer = source.clone();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        let late_token = Rc::new(RefCell::new(None));
        let token_slot = late_token.clone();
        let _first = source.value().subscribe(move |value| {
            if *value == 1 {
                writer.set(2);
                let output = output.clone();
                *token_slot.borrow_mut() = Some(
                    writer
                        .value()
                        .subscribe(move |value| output.borrow_mut().push(*value)),
                );
            }
        });
        source.set(1);
        assert_eq!(*seen.borrow(), [2, 2]);
    }

    #[test]
    fn panic_during_emission_restores_source_for_later_updates() {
        let source = MutableValue::new(0);
        let _panic = source.value().subscribe(|value| {
            if *value == 1 {
                panic!("observer panic");
            }
        });
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        let _other = source
            .value()
            .subscribe(move |value| output.borrow_mut().push(*value));
        assert!(catch_unwind(AssertUnwindSafe(|| source.set(1))).is_err());
        assert_eq!(*source.get(), 1);
        source.set(2);
        assert_eq!(*seen.borrow(), [0, 2]);
    }

    #[test]
    fn panic_during_replay_removes_observer_and_releases_its_captures() {
        let source = MutableValue::new(0);
        let calls = Rc::new(Cell::new(0));
        let count = calls.clone();
        let capture = Rc::new(());
        let weak = Rc::downgrade(&capture);
        assert!(
            catch_unwind(AssertUnwindSafe(|| source.value().subscribe(move |_| {
                let _ = &capture;
                count.set(count.get() + 1);
                panic!("replay panic");
            })))
            .is_err()
        );
        assert!(weak.upgrade().is_none());
        source.set(1);
        assert_eq!(calls.get(), 1);
    }
}
