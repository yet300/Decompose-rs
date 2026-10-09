//! Reentrancy-safe shared back-dispatcher handle.
//!
//! Essenty's [`BackDispatcher`] holds a mutable borrow across synchronous
//! callback dispatch, so any nested borrow from inside a callback panics
//! (`RefCell already borrowed`). [`SharedBack`] is the Decompose-owned
//! abstraction above it (no Essenty changes): an `Rc`-backed (`Clone`,
//! `!Send + !Sync`) handle whose every entry point runs inside a uniform
//! busy window. The outermost frame runs its operation then drains a FIFO
//! pending queue; nested frames only enqueue (or are rejected, see below).
//! No `RefCell` borrow is ever held across a nested entry, and raw
//! `Rc<RefCell<BackDispatcher>>` never appears in public API.
//!
//! Implemented audit contracts:
//!
//! - **R-01 — same-dispatcher reentrant invocation is unsupported.** Calling
//!   [`back`](SharedBack::back) (or any predictive entry point) on a
//!   `SharedBack` that is already dispatching returns `false` (predictive
//!   progress/cancel/invoke return `Ok(false)`) and queues nothing. In
//!   particular there is no deferred self-`Invoke`: the
//!   `callback → back → drain → callback → …` infinite synchronous drain
//!   loop cannot occur. Cross-dispatcher forwarding (parent bridge → child)
//!   stays synchronous, and ping-pong terminates because the already-busy
//!   dispatcher in any cycle refuses reentry.
//! - **R-02 — panic discards the aborted dispatch.** If a callback unwinds,
//!   the busy flag resets and the entire pending mutation queue from the
//!   aborted dispatch is discarded; nothing replays during a later unrelated
//!   call. The dispatcher retains its last completed state and the handle
//!   stays reusable. Post-panic callers must re-apply any intended mutation.
//! - **R-03 — enabled listeners receive the evented aggregate.** The
//!   aggregate listener delivers the newly computed `enabled: bool`; bridge
//!   gate logic must use that evented value and must NOT call
//!   [`can_handle`](SharedBack::can_handle) from inside the listener —
//!   during a busy window `can_handle()` intentionally reflects the cached
//!   pre-drain aggregate.
//!
//! Registration requested *during* active same-dispatcher dispatch is
//! rejected with [`DispatchInProgress`](crate::DispatchInProgress) (there is
//! no id reservation for queued registration). Everything else queues FIFO
//! when busy and applies deterministically after the outer operation
//! releases dispatcher access.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::rc::{Rc, Weak};

use essenty_back_handler::{BackDispatcher, BackError, BackEvent, BackHandle, GesturePosition};

use crate::DispatchInProgress;

/// Queued mutation applied after the outermost dispatch frame completes.
enum PendingOp {
    SetEnabled { id: u64, enabled: bool },
    SetPriority { id: u64, priority: i32 },
    Unregister { id: u64 },
    AddListener { id: u64, listener: EnabledListener },
    RemoveListener { id: u64 },
}

/// Wrapper-level enabled listener: receives the evented aggregate.
type EnabledListener = Rc<dyn Fn(bool)>;

struct Inner {
    dispatcher: RefCell<BackDispatcher>,
    /// Count of active operations on this dispatcher (entry, dispatch, or
    /// drain). Binary in practice: nested entries never increment, they
    /// take queue/reject paths instead.
    busy: Cell<usize>,
    pending: RefCell<VecDeque<PendingOp>>,
    after: RefCell<VecDeque<Box<dyn FnOnce()>>>,
    running_after: Cell<bool>,
    /// Aggregate `can_handle` as of the last completed outer operation.
    /// Served to callers that ask during a busy window.
    cached_can_handle: Cell<bool>,
    cached_handler_count: Cell<usize>,
    /// Wrapper-level enabled-listener registry. Kept outside the dispatcher
    /// so add/remove never alias dispatcher dispatch; one internal Essenty
    /// listener forwards the evented aggregate to these.
    listeners: RefCell<BTreeMap<u64, EnabledListener>>,
    next_listener_id: Cell<u64>,
}

impl Inner {
    fn run_after(&self) {
        if self.running_after.replace(true) {
            return;
        }
        struct Reset<'a>(&'a Inner);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.running_after.set(false);
                if std::thread::panicking() {
                    let abandoned = std::mem::take(&mut *self.0.after.borrow_mut());
                    drop(abandoned);
                }
            }
        }
        let _reset = Reset(self);
        loop {
            let callback = self.after.borrow_mut().pop_front();
            let Some(callback) = callback else { break };
            callback();
        }
    }
    /// Applies queued ops FIFO. Each op borrows the dispatcher separately;
    /// listener callbacks fired along the way re-queue (busy is still held).
    fn drain(&self) {
        loop {
            let op = self.pending.borrow_mut().pop_front();
            let Some(op) = op else { break };
            match op {
                PendingOp::SetEnabled { id, enabled } => {
                    self.dispatcher.borrow_mut().set_enabled(id, enabled);
                }
                PendingOp::SetPriority { id, priority } => {
                    self.dispatcher.borrow_mut().set_priority(id, priority);
                }
                PendingOp::Unregister { id } => {
                    self.dispatcher.borrow_mut().unregister(id);
                }
                PendingOp::AddListener { id, listener } => {
                    self.listeners.borrow_mut().insert(id, listener);
                }
                PendingOp::RemoveListener { id } => {
                    self.listeners.borrow_mut().remove(&id);
                }
            }
        }
        self.refresh_cache();
    }

    fn refresh_cache(&self) {
        let dispatcher = self.dispatcher.borrow();
        self.cached_can_handle.set(dispatcher.can_handle());
        self.cached_handler_count.set(dispatcher.handler_count());
    }

    /// Snapshots wrapper listeners and delivers the evented aggregate.
    /// Runs inside dispatcher mutation (busy is held), so user callbacks
    /// queue rather than borrow.
    fn forward_enabled(&self, enabled: bool) {
        let listeners = self
            .listeners
            .borrow()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for listener in listeners {
            listener(enabled);
        }
    }

    fn listener_known(&self, id: u64) -> bool {
        let mut known = self.listeners.borrow().contains_key(&id);
        // FIFO scan: the last queued mention wins.
        for op in self.pending.borrow().iter() {
            match op {
                PendingOp::AddListener { id: other, .. } if *other == id => known = true,
                PendingOp::RemoveListener { id: other } if *other == id => known = false,
                _ => {}
            }
        }
        known
    }
}

/// Guard for one outermost operation frame.
///
/// On normal completion the caller drains and completes explicitly. On
/// unwind (R-02) the guard resets the busy flag and discards the pending
/// queue from the aborted dispatch so stale mutations never replay later.
/// The guard itself never panics: all unwind-path access is best-effort via
/// `try_borrow`.
struct OuterFrame<'a> {
    inner: &'a Inner,
    completed: bool,
}

impl<'a> OuterFrame<'a> {
    fn enter(inner: &'a Inner) -> Self {
        debug_assert_eq!(inner.busy.get(), 0);
        inner.busy.set(1);
        Self {
            inner,
            completed: false,
        }
    }

    fn complete(mut self) {
        self.inner.drain();
        self.inner.busy.set(0);
        self.completed = true;
        self.inner.run_after();
    }
}

impl Drop for OuterFrame<'_> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        if std::thread::panicking() {
            // R-02: aborted dispatch — reset and discard, never replay.
            if let Ok(mut pending) = self.inner.pending.try_borrow_mut() {
                pending.clear();
            }
            let abandoned = self
                .inner
                .after
                .try_borrow_mut()
                .ok()
                .map(|mut after| std::mem::take(&mut *after));
            drop(abandoned);
            self.inner.busy.set(0);
            if let Ok(dispatcher) = self.inner.dispatcher.try_borrow() {
                self.inner.cached_can_handle.set(dispatcher.can_handle());
                self.inner
                    .cached_handler_count
                    .set(dispatcher.handler_count());
            }
        } else {
            self.inner.busy.set(0);
        }
    }
}

/// Reentrancy-safe shared handle to a [`BackDispatcher`].
///
/// `Rc`-backed and [`Clone`]; `!Send + !Sync` like the rest of the core.
/// All methods take `&self`. Public users never receive
/// `Rc<RefCell<BackDispatcher>>`: every operation below funnels through the
/// busy window, so synchronous callbacks (including parent→child bridge
/// forwarding) can safely mutate through this handle.
///
/// Idle mutations apply synchronously and are immediately observable.
/// Same-dispatcher mutations from inside a callback MUST go through these
/// methods (which queue); raw nested borrowing is unsupported by API
/// construction.
#[derive(Clone)]
pub struct SharedBack {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for SharedBack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedBack")
            .field("can_handle", &self.can_handle())
            .field("handler_count", &self.handler_count())
            .finish_non_exhaustive()
    }
}

impl SharedBack {
    /// Runs internal navigation work once the dispatcher borrow is released.
    pub(crate) fn after_dispatch(&self, callback: impl FnOnce() + 'static) {
        self.inner.after.borrow_mut().push_back(Box::new(callback));
        if !self.is_dispatching() {
            self.inner.run_after();
        }
    }
    /// Creates a fresh dispatcher with no callbacks and no listeners.
    #[must_use]
    pub fn new() -> Self {
        let inner = Rc::new(Inner {
            dispatcher: RefCell::new(BackDispatcher::new()),
            busy: Cell::new(0),
            pending: RefCell::new(VecDeque::new()),
            after: RefCell::new(VecDeque::new()),
            running_after: Cell::new(false),
            cached_can_handle: Cell::new(false),
            cached_handler_count: Cell::new(0),
            listeners: RefCell::new(BTreeMap::new()),
            next_listener_id: Cell::new(0),
        });
        // Single internal Essenty listener: forwards the evented aggregate
        // to wrapper-level listeners. Weak link so the dispatcher never
        // keeps the handle alive.
        let weak: Weak<Inner> = Rc::downgrade(&inner);
        inner
            .dispatcher
            .borrow_mut()
            .add_enabled_changed_listener(move |enabled| {
                if let Some(inner) = weak.upgrade() {
                    inner.forward_enabled(enabled);
                }
            });
        Self { inner }
    }

    /// Returns `true` while this dispatcher is inside an operation
    /// (dispatch or drain). Primarily useful for tests and debugging.
    #[must_use]
    pub fn is_dispatching(&self) -> bool {
        self.inner.busy.get() > 0
    }

    /// Registers a callback with a priority and initial enabled flag.
    ///
    /// Higher `priority` wins; ties break toward the later registration
    /// (LIFO). The callback receives the event plus this handle for
    /// in-handler mutations.
    ///
    /// WARNING (P1-06): the `&SharedBack` peer is for in-handler operations
    /// only — DO NOT retain, clone, or store it beyond callback execution.
    /// A retained peer closes `dispatcher → callback → peer → dispatcher`
    /// and leaks the dispatcher for as long as the peer is held (proven by
    /// the bridge-lifetime test). Clone the handle you registered *with*
    /// instead if shared ownership is needed.
    ///
    /// # Errors
    ///
    /// Returns [`DispatchInProgress`](crate::DispatchInProgress) when called
    /// while this same dispatcher is dispatching. There is no queued
    /// registration: callers (notably child-scope construction from inside a
    /// back callback) must treat this as a hard failure and roll back.
    ///
    /// # Panics
    ///
    /// A panicking wrapper enabled listener during register-triggered
    /// notification (P1-07) leaves the underlying Essenty handler inserted
    /// before any public handle is returned: partially applied state.
    /// Listener panics are programmer error — use a fresh fixture, do not
    /// catch and continue on this path. (R-02 queue discard covers
    /// callback-*dispatch* panics, not listener registration races.)
    pub fn register(
        &self,
        priority: i32,
        enabled: bool,
        callback: impl FnMut(BackEvent, &SharedBack) + 'static,
    ) -> Result<BackHandle, DispatchInProgress> {
        if self.inner.busy.get() > 0 {
            return Err(DispatchInProgress);
        }
        let frame = OuterFrame::enter(&self.inner);
        // Weak peer upgraded per call: a strong peer captured in the entry
        // would close an Rc cycle (dispatcher -> entry -> peer -> dispatcher)
        // and leak every dispatcher with live callbacks. The upgrade cannot
        // fail here: the outermost frame holds its handle for the whole call.
        let weak = Rc::downgrade(&self.inner);
        let mut callback = callback;
        let handle = self
            .inner
            .dispatcher
            .borrow_mut()
            .register(priority, enabled, move |event| {
                if let Some(peer) = weak.upgrade().map(|inner| SharedBack { inner }) {
                    callback(event, &peer);
                }
            });
        frame.complete();
        Ok(handle)
    }

    /// Removes the callback registered under `id`.
    ///
    /// Applies immediately when idle; queued FIFO when this dispatcher is
    /// busy. Removing the in-flight gesture owner follows Essenty semantics
    /// (the owner receives cancellation; a later progress event may claim a
    /// fallback handler).
    pub fn unregister(&self, id: u64) {
        if self.inner.busy.get() > 0 {
            self.inner
                .pending
                .borrow_mut()
                .push_back(PendingOp::Unregister { id });
            return;
        }
        let frame = OuterFrame::enter(&self.inner);
        self.inner.dispatcher.borrow_mut().unregister(id);
        frame.complete();
    }

    /// Enables or disables a callback. Immediate when idle, queued FIFO when
    /// busy. Disabling a gesture owner affects future gestures, not the owner
    /// already claimed for the current gesture.
    pub fn set_enabled(&self, id: u64, enabled: bool) {
        if self.inner.busy.get() > 0 {
            self.inner
                .pending
                .borrow_mut()
                .push_back(PendingOp::SetEnabled { id, enabled });
            return;
        }
        let frame = OuterFrame::enter(&self.inner);
        self.inner.dispatcher.borrow_mut().set_enabled(id, enabled);
        frame.complete();
    }

    /// Changes a callback's priority. Immediate when idle, queued FIFO when
    /// busy. Affects future winner selection, not the owner already claimed
    /// for the current gesture.
    pub fn set_priority(&self, id: u64, priority: i32) {
        if self.inner.busy.get() > 0 {
            self.inner
                .pending
                .borrow_mut()
                .push_back(PendingOp::SetPriority { id, priority });
            return;
        }
        let frame = OuterFrame::enter(&self.inner);
        self.inner
            .dispatcher
            .borrow_mut()
            .set_priority(id, priority);
        frame.complete();
    }

    /// Regular back invocation. Routes to the current winner.
    ///
    /// Returns `true` when a handler consumed the event. **R-01:** when
    /// called on the dispatcher that is already dispatching, returns `false`
    /// immediately and queues nothing (same-dispatcher reentrant invocation
    /// is unsupported — this is what rules out the infinite self-requeue
    /// drain loop).
    pub fn back(&self) -> bool {
        if self.inner.busy.get() > 0 {
            return false;
        }
        let frame = OuterFrame::enter(&self.inner);
        let handled = self.inner.dispatcher.borrow_mut().back();
        frame.complete();
        handled
    }

    /// Starts a predictive gesture, claiming the current winner.
    /// Returns `false` when busy (R-01) or when no enabled handler exists.
    pub fn predictive_start(&self) -> bool {
        if self.inner.busy.get() > 0 {
            return false;
        }
        let frame = OuterFrame::enter(&self.inner);
        let handled = self.inner.dispatcher.borrow_mut().predictive_start();
        frame.complete();
        handled
    }

    /// Starts a predictive gesture with platform position data.
    /// Returns `false` when busy (R-01) or when no enabled handler exists.
    pub fn predictive_start_with(&self, position: GesturePosition) -> bool {
        if self.inner.busy.get() > 0 {
            return false;
        }
        let frame = OuterFrame::enter(&self.inner);
        let handled = self
            .inner
            .dispatcher
            .borrow_mut()
            .predictive_start_with(position);
        frame.complete();
        handled
    }

    /// Delivers progress to the gesture owner.
    ///
    /// # Errors
    ///
    /// Returns [`BackError::NoGestureInProgress`] when no gesture is in
    /// flight. A reentrant call on the dispatching dispatcher (R-01) is a
    /// non-recursive no-op returning `Ok(false)`.
    pub fn predictive_progress(&self, progress: f32) -> Result<bool, BackError> {
        if self.inner.busy.get() > 0 {
            return Ok(false);
        }
        let frame = OuterFrame::enter(&self.inner);
        let result = self
            .inner
            .dispatcher
            .borrow_mut()
            .predictive_progress(progress);
        frame.complete();
        result
    }

    /// Delivers progress with platform position data. Same reentry contract
    /// as [`predictive_progress`](SharedBack::predictive_progress).
    ///
    /// # Errors
    ///
    /// Returns [`BackError::NoGestureInProgress`] when no gesture is in
    /// flight.
    pub fn predictive_progress_with(
        &self,
        progress: f32,
        position: GesturePosition,
    ) -> Result<bool, BackError> {
        if self.inner.busy.get() > 0 {
            return Ok(false);
        }
        let frame = OuterFrame::enter(&self.inner);
        let result = self
            .inner
            .dispatcher
            .borrow_mut()
            .predictive_progress_with(progress, position);
        frame.complete();
        result
    }

    /// Cancels the in-flight gesture, delivering `Cancelled` to its owner.
    ///
    /// # Errors
    ///
    /// Returns [`BackError::NoGestureInProgress`] when no gesture is in
    /// flight. A reentrant call on the dispatching dispatcher (R-01) is a
    /// non-recursive no-op returning `Ok(false)`.
    pub fn predictive_cancel(&self) -> Result<bool, BackError> {
        if self.inner.busy.get() > 0 {
            return Ok(false);
        }
        let frame = OuterFrame::enter(&self.inner);
        let result = self.inner.dispatcher.borrow_mut().predictive_cancel();
        frame.complete();
        result
    }

    /// Completes the in-flight gesture, delivering `Invoked` to its owner.
    ///
    /// # Errors
    ///
    /// Returns [`BackError::NoGestureInProgress`] when no gesture is in
    /// flight. A reentrant call on the dispatching dispatcher (R-01) is a
    /// non-recursive no-op returning `Ok(false)`.
    pub fn predictive_invoke(&self) -> Result<bool, BackError> {
        if self.inner.busy.get() > 0 {
            return Ok(false);
        }
        let frame = OuterFrame::enter(&self.inner);
        let result = self.inner.dispatcher.borrow_mut().predictive_invoke();
        frame.complete();
        result
    }

    /// Returns `true` when at least one enabled callback is registered.
    ///
    /// Fresh read when idle; maintained cached aggregate when busy (R-03).
    /// Bridge gate logic must NOT use this from inside an enabled listener —
    /// use the evented `bool` the listener receives instead.
    #[must_use]
    pub fn can_handle(&self) -> bool {
        if self.inner.busy.get() > 0 {
            return self.inner.cached_can_handle.get();
        }
        let dispatcher = self.inner.dispatcher.borrow();
        let current = dispatcher.can_handle();
        self.inner.cached_can_handle.set(current);
        current
    }

    /// Number of registered callbacks (enabled or not).
    ///
    /// Fresh read when idle; pre-dispatch count when busy (queued removals
    /// apply at drain).
    #[must_use]
    pub fn handler_count(&self) -> usize {
        if self.inner.busy.get() > 0 {
            return self.inner.cached_handler_count.get();
        }
        let dispatcher = self.inner.dispatcher.borrow();
        let current = dispatcher.handler_count();
        self.inner.cached_handler_count.set(current);
        current
    }

    /// Adds a listener invoked with the aggregate enabled state whenever it
    /// changes. The delivered `bool` is the newly computed aggregate —
    /// bridge code must use exactly this evented value (R-03).
    ///
    /// Additions requested while busy queue FIFO and take effect at drain,
    /// so a listener added from inside a callback never observes the event
    /// currently being delivered.
    ///
    /// # Panics
    ///
    /// Wrapper enabled listeners must not panic (P1-07). A panic unwinds
    /// through the notifying dispatcher operation with partially applied
    /// state; treat it as programmer error and use a fresh fixture.
    pub fn add_enabled_changed_listener(&self, listener: impl Fn(bool) + 'static) -> u64 {
        let id = self.inner.next_listener_id.get();
        self.inner.next_listener_id.set(id + 1);
        if self.inner.busy.get() > 0 {
            self.inner
                .pending
                .borrow_mut()
                .push_back(PendingOp::AddListener {
                    id,
                    listener: Rc::new(listener),
                });
            return id;
        }
        self.inner
            .listeners
            .borrow_mut()
            .insert(id, Rc::new(listener));
        id
    }

    /// Removes a listener added by
    /// [`add_enabled_changed_listener`](SharedBack::add_enabled_changed_listener).
    /// Returns `true` when the id was known. Removals requested while busy
    /// queue FIFO.
    pub fn remove_enabled_changed_listener(&self, id: u64) -> bool {
        let known = self.inner.listener_known(id);
        if self.inner.busy.get() > 0 {
            self.inner
                .pending
                .borrow_mut()
                .push_back(PendingOp::RemoveListener { id });
            return known;
        }
        self.inner.listeners.borrow_mut().remove(&id).is_some()
    }
}

impl Default for SharedBack {
    fn default() -> Self {
        Self::new()
    }
}

/// Crate-internal weak link to a [`SharedBack`].
///
/// Used by the child back bridge: the child's enabled listener captures a
/// `WeakBack` to the parent, breaking the one genuine reference cycle
/// (parent callback → child → listener → parent) by upgrade-per-call.
#[derive(Debug, Clone)]
pub(crate) struct WeakBack {
    weak: Weak<Inner>,
}

impl WeakBack {
    pub(crate) fn upgrade(&self) -> Option<SharedBack> {
        self.weak.upgrade().map(|inner| SharedBack { inner })
    }
}

impl SharedBack {
    /// Downgrades to a weak link. Upgrading after the handle is gone yields
    /// `None` (bridge gate updates become no-ops).
    pub(crate) fn downgrade(&self) -> WeakBack {
        WeakBack {
            weak: Rc::downgrade(&self.inner),
        }
    }

    /// Strong reference count of the shared inner state (test support for
    /// the bridge-lifetime test: proves the parent link is weak).
    #[cfg(test)]
    pub(crate) fn strong_count(&self) -> usize {
        Rc::strong_count(&self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell as StdRefCell;
    use std::rc::Rc as StdRc;

    fn enabled_event_log() -> (StdRc<StdRefCell<Vec<bool>>>, impl Fn(bool) + 'static) {
        let log = StdRc::new(StdRefCell::new(Vec::new()));
        let logged = StdRc::clone(&log);
        (log, move |enabled: bool| logged.borrow_mut().push(enabled))
    }

    #[test]
    fn basic_back_routing_and_unregister() {
        let back = SharedBack::new();
        assert!(!back.can_handle());
        assert!(!back.back());

        let hits = StdRc::new(StdRefCell::new(0u32));
        let probe = StdRc::clone(&hits);
        let handle = back
            .register(0, true, move |_, _| *probe.borrow_mut() += 1)
            .unwrap();
        assert!(back.can_handle());
        assert_eq!(back.handler_count(), 1);
        assert!(back.back());
        assert_eq!(*hits.borrow(), 1);
        back.unregister(handle.id());
        assert_eq!(back.handler_count(), 0);
        assert!(!back.can_handle());
        assert!(!back.back());
        assert_eq!(*hits.borrow(), 1);
    }

    #[test]
    fn priority_orders_and_lifo_breaks_ties() {
        let back = SharedBack::new();
        let order = StdRc::new(StdRefCell::new(Vec::new()));
        let mut ids = Vec::new();
        for (priority, tag) in [(0, "a"), (10, "b"), (10, "c")] {
            let order = StdRc::clone(&order);
            ids.push(
                back.register(priority, true, move |_, _| order.borrow_mut().push(tag))
                    .unwrap(),
            );
        }
        // Highest priority wins; on ties the later registration wins (LIFO).
        assert!(back.back());
        assert_eq!(*order.borrow(), vec!["c"]);
        // Live priority mutation affects subsequent presses.
        back.set_priority(ids[0].id(), 20);
        assert!(back.back());
        assert_eq!(*order.borrow(), vec!["c", "a"]);
    }

    #[test]
    fn disabled_callbacks_do_not_handle() {
        let back = SharedBack::new();
        let hits = StdRc::new(StdRefCell::new(0u32));
        let probe = StdRc::clone(&hits);
        let handle = back
            .register(0, false, move |_, _| *probe.borrow_mut() += 1)
            .unwrap();
        assert!(!back.can_handle());
        assert!(!back.back());
        back.set_enabled(handle.id(), true);
        assert!(back.can_handle());
        assert!(back.back());
        assert_eq!(*hits.borrow(), 1);
    }

    #[test]
    fn enabled_listener_receives_evented_aggregate() {
        let back = SharedBack::new();
        let (log, listener) = enabled_event_log();
        back.add_enabled_changed_listener(listener);
        let h1 = back.register(0, true, |_, _| {}).unwrap();
        assert_eq!(*log.borrow(), vec![true]);
        let _h2 = back.register(0, false, |_, _| {}).unwrap();
        // No aggregate change: no new event.
        assert_eq!(*log.borrow(), vec![true]);
        back.set_enabled(h1.id(), false);
        assert_eq!(*log.borrow(), vec![true, false]);
        back.unregister(h1.id());
        // Still false overall: no new event.
        assert_eq!(*log.borrow(), vec![true, false]);
    }

    #[test]
    fn self_disable_during_back_queues_and_applies_after_drain() {
        use std::cell::Cell as StdCell;
        let back = SharedBack::new();
        let (log, listener) = enabled_event_log();
        back.add_enabled_changed_listener(listener);
        let id_cell = StdRc::new(StdCell::new(u64::MAX));
        let id_probe = StdRc::clone(&id_cell);
        let handle = back
            .register(0, true, move |_, peer| {
                // Queued: takes effect after this dispatch completes.
                peer.set_enabled(id_probe.get(), false);
                // Busy-window read still reflects the pre-drain aggregate.
                assert!(peer.can_handle());
            })
            .unwrap();
        id_cell.set(handle.id());
        assert!(back.back());
        // Post-drain the disable applied exactly once.
        assert!(!back.can_handle());
        assert_eq!(*log.borrow(), vec![true, false]);
        assert!(!back.back());
    }

    #[test]
    fn self_unregister_during_back_is_safe() {
        use std::cell::Cell as StdCell;
        let back = SharedBack::new();
        let count = StdRc::new(StdRefCell::new(0u32));
        let probe = StdRc::clone(&count);
        let id_cell = StdRc::new(StdCell::new(u64::MAX));
        let id_probe = StdRc::clone(&id_cell);
        let handle = back
            .register(0, true, move |_, peer| {
                *probe.borrow_mut() += 1;
                peer.unregister(id_probe.get());
            })
            .unwrap();
        id_cell.set(handle.id());
        assert!(back.back());
        assert_eq!(*count.borrow(), 1);
        assert_eq!(back.handler_count(), 0);
        assert!(!back.back());
    }

    #[test]
    fn priority_change_during_dispatch_applies_after_drain() {
        use std::cell::Cell as StdCell;
        let back = SharedBack::new();
        let order = StdRc::new(StdRefCell::new(Vec::new()));
        let id_cell = StdRc::new(StdCell::new(u64::MAX));
        let id_probe = StdRc::clone(&id_cell);
        let order_low = StdRc::clone(&order);
        let low_handle = back
            .register(0, true, move |_, _| order_low.borrow_mut().push("low"))
            .unwrap();
        id_cell.set(low_handle.id());
        // Winner boosts the loser mid-dispatch; the queued change does not
        // affect the current dispatch but is live afterwards.
        let order_high = StdRc::clone(&order);
        back.register(50, true, move |_, peer| {
            order_high.borrow_mut().push("high");
            peer.set_priority(id_probe.get(), 100);
        })
        .unwrap();
        assert!(back.back());
        assert_eq!(*order.borrow(), vec!["high"]);
        // After drain the boost is live: "low" (priority 100) wins.
        assert!(back.back());
        assert_eq!(*order.borrow(), vec!["high", "low"]);
    }

    #[test]
    fn register_during_dispatch_is_rejected() {
        let back = SharedBack::new();
        let peer_check = StdRc::new(StdRefCell::new(String::new()));
        let probe = StdRc::clone(&peer_check);
        back.register(0, true, move |_, peer| {
            let err = peer.register(0, true, |_, _| {}).unwrap_err();
            probe.borrow_mut().push_str(&err.to_string());
            assert!(peer.is_dispatching());
        })
        .unwrap();
        assert!(back.back());
        assert!(!peer_check.borrow().is_empty());
        assert_eq!(back.handler_count(), 1);
    }

    #[test]
    fn r01_same_dispatcher_reentrant_back_returns_false_without_requeue() {
        let back = SharedBack::new();
        let calls = StdRc::new(StdRefCell::new(0u32));
        let reentry = StdRc::new(StdRefCell::new(None::<bool>));
        let probe = StdRc::clone(&calls);
        let reentry_probe = StdRc::clone(&reentry);
        back.register(0, true, move |_, peer| {
            *probe.borrow_mut() += 1;
            // Unsupported: must return false and queue nothing.
            *reentry_probe.borrow_mut() = Some(peer.back());
        })
        .unwrap();
        assert!(back.back());
        assert_eq!(*calls.borrow(), 1, "no self-requeue may multiply the event");
        assert_eq!(*reentry.borrow(), Some(false));
        // Dispatcher is fully reusable afterwards.
        assert!(back.back());
        assert_eq!(*calls.borrow(), 2);
    }

    #[test]
    fn r01_reentrant_predictive_calls_are_non_recursive_noops() {
        let back = SharedBack::new();
        let seen = StdRc::new(StdRefCell::new(Vec::new()));
        let probe = StdRc::clone(&seen);
        back.register(0, true, move |event, peer| {
            // Only the outer start reaches the callback; every reentrant
            // predictive call is a no-op returning the false-equivalent.
            probe.borrow_mut().push((
                peer.predictive_start(),
                peer.predictive_progress(0.5).unwrap(),
                peer.predictive_cancel().unwrap(),
                peer.predictive_invoke().unwrap(),
            ));
            let _ = event;
        })
        .unwrap();
        assert!(back.predictive_start());
        assert_eq!(seen.borrow().len(), 1);
        assert_eq!(*seen.borrow(), vec![(false, false, false, false)]);
        // Gesture state is coherent afterwards: cancel the in-flight gesture.
        assert!(back.predictive_cancel().unwrap());
    }

    #[test]
    fn r02_panic_discards_queued_mutations_without_stale_replay() {
        use std::cell::Cell as StdCell;
        let back = SharedBack::new();
        let id_cell = StdRc::new(StdCell::new(u64::MAX));
        let id_probe = StdRc::clone(&id_cell);
        let handle = back
            .register(0, true, move |_, peer| {
                // Queue a mutation, then unwind.
                peer.set_enabled(id_probe.get(), false);
                panic!("boom");
            })
            .unwrap();
        id_cell.set(handle.id());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            back.back();
        }));
        assert!(result.is_err());
        // The aborted dispatch's queued disable was discarded: the handler
        // is still enabled and the dispatcher is reusable.
        assert!(!back.is_dispatching());
        assert!(back.can_handle());
        let hits = StdRc::new(StdRefCell::new(0u32));
        let probe = StdRc::clone(&hits);
        // Replace the panicking callback and prove normal operation resumes
        // with no stale invoke replayed.
        back.unregister(handle.id());
        back.register(0, true, move |_, _| *probe.borrow_mut() += 1)
            .unwrap();
        assert!(back.back());
        assert_eq!(*hits.borrow(), 1);
    }

    #[test]
    fn r03_busy_window_can_handle_is_cached_predrain() {
        use std::cell::Cell as StdCell;
        let back = SharedBack::new();
        let (log, listener) = enabled_event_log();
        back.add_enabled_changed_listener(listener);
        let id_cell = StdRc::new(StdCell::new(u64::MAX));
        let id_probe = StdRc::clone(&id_cell);
        let handle = back
            .register(0, true, move |_, peer| {
                peer.set_enabled(id_probe.get(), false);
                // Queued disable not yet applied: cached pre-drain aggregate.
                // A listener MUST NOT use this value to compute gates; it must
                // use the evented bool it receives.
                assert!(peer.can_handle());
            })
            .unwrap();
        id_cell.set(handle.id());
        assert!(back.back());
        // One aggregate change delivered exactly once, with the new value.
        assert_eq!(*log.borrow(), vec![true, false]);
        assert!(!back.can_handle());
    }

    #[test]
    fn listener_add_remove_during_dispatch_queue_fifo() {
        use std::cell::Cell as StdCell;
        let back = SharedBack::new();
        let (log, listener) = enabled_event_log();
        let late_id = StdRc::new(StdRefCell::new(None::<u64>));
        let late_probe = StdRc::clone(&late_id);
        let id_cell = StdRc::new(StdCell::new(u64::MAX));
        let id_probe = StdRc::clone(&id_cell);
        let first = back
            .register(0, true, move |_, peer| {
                // Queued add: takes effect at drain, misses the current event.
                let id = peer.add_enabled_changed_listener(|_| {});
                *late_probe.borrow_mut() = Some(id);
                peer.set_enabled(id_probe.get(), false);
            })
            .unwrap();
        id_cell.set(first.id());
        back.add_enabled_changed_listener(listener);
        assert!(back.back());
        // The register-time false->true transition predates the listener, so
        // the first observed event is the drain-applied disable. The late
        // (queued) listener likewise only observes post-drain changes.
        assert_eq!(*log.borrow(), vec![false]);
        assert!(late_id.borrow().is_some());
        // Queued removal applies at drain as well.
        let extra = back.add_enabled_changed_listener(|_| {});
        let self_id = StdRc::new(StdCell::new(u64::MAX));
        let self_probe = StdRc::clone(&self_id);
        let second = back
            .register(0, true, move |_, peer| {
                peer.remove_enabled_changed_listener(extra);
                peer.set_enabled(self_probe.get(), false);
            })
            .unwrap();
        self_id.set(second.id());
        assert!(back.back());
    }

    #[test]
    fn predictive_forwarding_flows_and_gesture_ownership() {
        use essenty_back_handler::BackPhase;
        let back = SharedBack::new();
        let phases = StdRc::new(StdRefCell::new(Vec::new()));
        let probe = StdRc::clone(&phases);
        back.register(0, true, move |event, _| {
            probe.borrow_mut().push(event.phase)
        })
        .unwrap();
        assert!(back.predictive_start());
        assert!(back.predictive_progress(0.25).unwrap());
        assert!(back.predictive_cancel().unwrap());
        assert_eq!(
            *phases.borrow(),
            vec![
                BackPhase::Started,
                BackPhase::Progressed,
                BackPhase::Cancelled
            ]
        );
        // No gesture in flight: predictive steps report it coherently.
        assert_eq!(
            back.predictive_progress(0.5).unwrap_err(),
            BackError::NoGestureInProgress
        );
        assert_eq!(
            back.predictive_cancel().unwrap_err(),
            BackError::NoGestureInProgress
        );
        assert_eq!(
            back.predictive_invoke().unwrap_err(),
            BackError::NoGestureInProgress
        );
        // Start-then-invoke completes the gesture.
        assert!(back.predictive_start());
        assert!(back.predictive_invoke().unwrap());
        assert_eq!(
            *phases.borrow(),
            vec![
                BackPhase::Started,
                BackPhase::Progressed,
                BackPhase::Cancelled,
                BackPhase::Started,
                BackPhase::Invoked
            ]
        );
    }

    #[test]
    fn mid_gesture_priority_change_does_not_steal_ownership() {
        let back = SharedBack::new();
        let order = StdRc::new(StdRefCell::new(Vec::new()));
        let low = StdRc::clone(&order);
        back.register(0, true, move |event, _| {
            low.borrow_mut().push(("low", event.phase))
        })
        .unwrap();
        let high = StdRc::clone(&order);
        let high_handle = back
            .register(100, false, move |event, _| {
                high.borrow_mut().push(("high", event.phase))
            })
            .unwrap();
        assert!(back.predictive_start());
        // Enable the higher-priority handler mid-gesture: ownership stays
        // with the claimed owner (Essenty behavior, delegated untouched).
        back.set_enabled(high_handle.id(), true);
        assert!(back.predictive_progress(0.5).unwrap());
        assert!(back.predictive_cancel().unwrap());
        assert!(order.borrow().iter().all(|(tag, _)| *tag == "low"));
    }

    #[test]
    fn cross_dispatcher_forwarding_stays_synchronous_and_terminates() {
        use std::cell::Cell as StdCell;
        let parent = SharedBack::new();
        let child = SharedBack::new();
        let child_calls = StdRc::new(StdCell::new(0u32));
        let probe = StdRc::clone(&child_calls);
        child
            .register(0, true, move |_, _| probe.set(probe.get() + 1))
            .unwrap();
        let child_peer = child.clone();
        // Parent bridge-style callback: synchronous cross-dispatcher forward.
        parent
            .register(0, true, move |_, _| {
                child_peer.back();
            })
            .unwrap();
        assert!(parent.back());
        assert_eq!(child_calls.get(), 1);
        // Ping-pong attempt terminates: child callback reenters the busy
        // parent, which refuses (R-01) instead of recursing.
        let parent2 = SharedBack::new();
        let child2 = SharedBack::new();
        let p2 = parent2.clone();
        let c2 = child2.clone();
        parent2
            .register(0, true, move |_, _| {
                c2.back();
            })
            .unwrap();
        child2
            .register(0, true, move |_, _| {
                p2.back();
            })
            .unwrap();
        assert!(parent2.back());
    }
}
