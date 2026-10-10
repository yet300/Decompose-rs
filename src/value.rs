//! Synchronous, single-threaded observable state with shared snapshots.
//!
//! Use [`Value::get_cloned`] for owned/Copy payloads and [`Value::with`] for
//! immediate borrowed reads. Both release internal borrows before user code.
//! [`MutableValue::modify`] edits a cloned payload and publishes a new snapshot;
//! existing snapshots remain immutable. Stored observers/mappers need owned
//! captures because the source can outlive the registration call.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

mod lifecycle;
mod mapped;
pub use lifecycle::ObserveLifecycleMode;

trait Derived<T> {
    fn get(&self) -> Rc<T>;
    fn subscribe(self: Rc<Self>, callback: Box<dyn Fn(Rc<T>)>) -> Subscription;
}

enum ValueSource<T> {
    Direct(Rc<Source<T>>),
    Derived(Rc<dyn Derived<T>>),
}

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
    source: ValueSource<T>,
}

impl<T> Clone for Value<T> {
    fn clone(&self) -> Self {
        Self {
            source: match &self.source {
                ValueSource::Direct(source) => ValueSource::Direct(source.clone()),
                ValueSource::Derived(source) => ValueSource::Derived(source.clone()),
            },
        }
    }
}

impl<T> Value<T> {
    /// Returns the latest shared snapshot, without cloning its payload.
    pub fn get(&self) -> Rc<T> {
        match &self.source {
            ValueSource::Direct(source) => source.current.borrow().clone(),
            ValueSource::Derived(source) => source.get(),
        }
    }

    /// Clones the current payload (copies it when `T: Copy`).
    pub fn get_cloned(&self) -> T
    where
        T: Clone,
    {
        self.with(Clone::clone)
    }

    /// Reads one snapshot through a short-lived reference. The callback may
    /// borrow stack data or reenter the source; no `RefCell` guard is held.
    pub fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        read(&self.get())
    }
}

impl<T: 'static> Value<T> {
    /// Observes payload references with the same immediate replay as `subscribe`.
    /// The stored callback must own its captures; the payload reference is
    /// valid only for each invocation.
    pub fn subscribe_ref(&self, callback: impl Fn(&T) + 'static) -> Subscription {
        self.subscribe(move |snapshot| callback(&snapshot))
    }

    /// Observes owned payloads, cloning once per delivery (including replay).
    /// The callback is retained by the source and must own its captures.
    pub fn subscribe_cloned(&self, callback: impl Fn(T) + 'static) -> Subscription
    where
        T: Clone,
    {
        self.subscribe_ref(move |value| callback(value.clone()))
    }

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
        let source = match &self.source {
            ValueSource::Direct(source) => source,
            ValueSource::Derived(source) => return source.clone().subscribe(Box::new(callback)),
        };
        let observer = Rc::new(Observer {
            callback: Box::new(callback),
            active: Cell::new(true),
            ready: Cell::new(false),
        });
        source.observers.borrow_mut().push(observer.clone());

        let source = Rc::downgrade(source);
        let target = Rc::downgrade(&observer);
        // Construct the cancellation guard before replay, so unwinding cannot
        // leave an observer whose caller never received a cancellation token.
        let subscription = Subscription::from_cancel(move || {
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
        });

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

    /// Creates an eagerly initialized view cached by source snapshot identity.
    ///
    /// The mapper runs outside source and cache borrows. With no observers,
    /// source changes are mapped on the next read. Recursive reads of the same
    /// uncached input panic; unwinding resets this guard for subsequent reads.
    pub fn map<R: 'static>(&self, mapper: impl Fn(&T) -> R + 'static) -> Value<R> {
        mapped::create(self.clone(), mapper)
    }

    /// Observes while the lifecycle meets the selected activity threshold.
    ///
    /// An active lifecycle immediately replays the latest snapshot. Dropping
    /// or canceling the token unregisters both observers; detaching retains the
    /// binding until destruction, which must be signaled explicitly. The token
    /// keeps the value and lifecycle alive. A detached binding must receive
    /// `Destroy` to release these resources, even if external handles are dropped.
    pub fn subscribe_with_lifecycle(
        &self,
        lifecycle: &essenty_lifecycle::LifecycleRegistry,
        mode: ObserveLifecycleMode,
        observer: impl Fn(Rc<T>) + 'static,
    ) -> Subscription {
        lifecycle::subscribe(self.clone(), lifecycle, mode, observer)
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
    source: Rc<Source<T>>,
}

impl<T> Clone for MutableValue<T> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
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
            source: Rc::new(Source {
                current: RefCell::new(value),
                observers: RefCell::new(Vec::new()),
                emitting: Cell::new(false),
            }),
        }
    }

    /// Returns a read-only handle to this source.
    pub fn value(&self) -> Value<T> {
        Value {
            source: ValueSource::Direct(self.source.clone()),
        }
    }

    /// Returns the latest shared snapshot, without cloning its payload.
    pub fn get(&self) -> Rc<T> {
        self.source.current.borrow().clone()
    }

    /// Clones the current payload, without requiring an explicit `Rc` dereference.
    pub fn get_cloned(&self) -> T
    where
        T: Clone,
    {
        self.with(Clone::clone)
    }

    /// Reads an immutable snapshot without holding an internal borrow while
    /// invoking user code. The callback may borrow local data.
    pub fn with<R>(&self, read: impl FnOnce(&T) -> R) -> R {
        read(&self.get())
    }

    /// Edits a cloned payload, publishing a new immutable snapshot on success.
    /// Retries if the callback or `Clone` reentrantly changes the source. The
    /// callback may borrow stack data and must tolerate repeated invocation.
    /// A callback panic leaves its uncommitted edit unpublished.
    pub fn modify(&self, mut edit: impl FnMut(&mut T))
    where
        T: Clone,
    {
        self.update_pair(|previous| {
            let mut next = (*previous).clone();
            edit(&mut next);
            Rc::new(next)
        });
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
        let source = &self.source;
        let previous = source.current.replace(value);
        self.emit(previous);
    }

    /// Exchanges snapshots by identity, returning the predecessor on success
    /// or the current snapshot on failure. Synchronous and single-threaded;
    /// notifications may reenter after the exchange has committed.
    pub fn compare_exchange(&self, expected: &Rc<T>, new: Rc<T>) -> Result<Rc<T>, Rc<T>> {
        let previous = self.exchange(expected, new)?;
        self.emit(previous.clone());
        Ok(previous)
    }

    fn exchange(&self, expected: &Rc<T>, new: Rc<T>) -> Result<Rc<T>, Rc<T>> {
        let previous = {
            let mut current = self.source.current.borrow_mut();
            if !Rc::ptr_eq(&current, expected) {
                return Err(current.clone());
            }
            std::mem::replace(&mut *current, new)
        };
        Ok(previous)
    }

    /// Transforms shared snapshots, retrying reentrant writes. The borrowed
    /// callback may run more than once; payloads need neither Clone nor Eq.
    pub fn update_rc(&self, transformer: impl FnMut(Rc<T>) -> Rc<T>) {
        self.update_pair(transformer);
    }

    /// Returns the exact committed snapshot, even if notification replaces it.
    /// The borrowed transformer may be retried after reentrant writes.
    pub fn update_rc_and_get(&self, transformer: impl FnMut(Rc<T>) -> Rc<T>) -> Rc<T> {
        self.update_pair(transformer).1
    }

    /// Returns the predecessor of the successful commit. The borrowed
    /// transformer retries if it reentrantly changes the current identity.
    pub fn get_and_update_rc(&self, transformer: impl FnMut(Rc<T>) -> Rc<T>) -> Rc<T> {
        self.update_pair(transformer).0
    }

    fn update_pair(&self, mut transformer: impl FnMut(Rc<T>) -> Rc<T>) -> (Rc<T>, Rc<T>) {
        loop {
            let previous = self.get();
            let next = transformer(previous.clone());
            match self.exchange(&previous, next.clone()) {
                Ok(_) => {
                    self.emit(previous.clone());
                    return (previous, next);
                }
                Err(_) => continue,
            }
        }
    }

    fn emit(&self, previous: Rc<T>) {
        let source = &self.source;
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
/// Direct and mapped value registrations hold weak references to their source;
/// keeping their token alive does not keep the source or observer alive.
/// Lifecycle registrations retain their value and lifecycle until cancellation
/// or destruction. Cancellation is idempotent for all registrations.
#[must_use = "dropping a subscription immediately cancels its observer"]
pub struct Subscription {
    cancellation: RefCell<Option<Box<dyn FnOnce()>>>,
    detachment: RefCell<Option<Box<dyn FnOnce()>>>,
}

impl Subscription {
    /// Creates a subscription with an idempotent, one-shot cancellation callback.
    pub fn from_cancel(cancel: impl FnOnce() + 'static) -> Self {
        Self {
            cancellation: RefCell::new(Some(Box::new(cancel))),
            detachment: RefCell::new(None),
        }
    }

    fn with_detach(cancel: impl FnOnce() + 'static, detach: impl FnOnce() + 'static) -> Self {
        Self {
            cancellation: RefCell::new(Some(Box::new(cancel))),
            detachment: RefCell::new(Some(Box::new(detach))),
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
        let detachment = self.detachment.borrow_mut().take();
        drop(detachment);
    }

    /// Leaves a direct or mapped observer registered until the source is dropped.
    ///
    /// Consumes this token without canceling. Capturing a strong source handle
    /// in a detached observer creates an ownership cycle; use a weak capture
    /// when the observer needs to refer back to its source.
    /// A lifecycle registration instead retains its binding until an explicit
    /// lifecycle destruction; dropping external handles does not release it.
    /// This is leak-by-design when destruction never arrives or captures form
    /// a strong ownership cycle. Prefer retaining the RAII token.
    pub fn detach(self) {
        let cancellation = self.cancellation.borrow_mut().take();
        let detachment = self.detachment.borrow_mut().take();
        if let Some(detachment) = detachment {
            detachment();
        }
        drop(cancellation);
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
