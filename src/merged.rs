//! Decompose-side merged lifecycle: `min(parent, manual)` as a lifecycle.
//!
//! Only `Destroyed` is honored at construction; any other input state is
//! treated as `Initialized`, so the merged registry always begins
//! `Initialized` and forward progress comes from subscription replay plus
//! live callbacks. Every delivery maps to `move_to(min(side_a, side_b))`
//! with the upstream callback table (`onCreate → Created`, `onStart →
//! Started`, `onResume → Resumed`, `onPause → Started`, `onStop → Created`,
//! `onDestroy → Destroyed`; reverse transitions are lossy by design).
//! `move_to` is idempotent with sticky destroy, relying on Essenty cascading
//! for multi-step jumps. If either input is already `Destroyed`, no
//! subscriptions are taken and the merged lifecycle is born `Destroyed`. On
//! merged `Destroyed` both observers unsubscribe (exactly-once observable
//! destroy; later parent transitions have no effect). Delivery is fully
//! synchronous and reentrant.
//!
//! Upstream reference: `MergedLifecycle` plus `MergedLifecycleTest` (40+
//! cases) in `arkivanov/Decompose@00840bc`; Essenty delivery-order parity
//! with Kotlin is not claimed.

use std::cell::Cell;
use std::rc::Rc;

use essenty_lifecycle::{LifecycleError, LifecycleRegistry, LifecycleState};

/// Upstream semantic `min` of two lifecycle states.
///
/// `Destroyed` is terminal and dominates: if either side is destroyed the
/// effective state is `Destroyed`. Among the forward states the minimum by
/// forward rank wins. This deliberately does NOT use Rust's derived `Ord`
/// on [`LifecycleState`], on which `Destroyed` is maximal and would corrupt
/// the minimum.
fn semantic_min(first: LifecycleState, second: LifecycleState) -> LifecycleState {
    if first.is_destroyed() || second.is_destroyed() {
        return LifecycleState::Destroyed;
    }
    fn rank(state: LifecycleState) -> u8 {
        match state {
            LifecycleState::Initialized => 0,
            LifecycleState::Created => 1,
            LifecycleState::Started => 2,
            LifecycleState::Resumed => 3,
            LifecycleState::Destroyed => 4,
        }
    }
    if rank(first) <= rank(second) {
        first
    } else {
        second
    }
}

/// Drives `merged` toward `target`, idempotently.
///
/// - Already-destroyed merged registries ignore every target (sticky
///   destroy).
/// - `Initialized → Destroyed` goes through `create(); destroy()` so
///   observers see the upstream-compatible `Created, Destroyed` sequence
///   (a bare Essenty `destroy()` from `Initialized` would skip `Created`).
/// - `AlreadyDestroyed` from a reentrant terminal race is swallowed as
///   idempotent success. `InvalidTransition` is unreachable by construction
///   (sides never deliver `Initialized`, so a backward move to `Initialized`
///   can never be requested once merged has advanced) and is ignored
///   defensively.
pub(crate) fn drive(merged: &LifecycleRegistry, target: LifecycleState) {
    if merged.state().is_destroyed() {
        return;
    }
    if target.is_destroyed() {
        if merged.state() == LifecycleState::Initialized
            && let Err(LifecycleError::AlreadyDestroyed) = merged.create()
        {
            return;
        }
        let _ = merged.destroy();
        return;
    }
    if merged.state() == target {
        return;
    }
    match merged.move_to(target) {
        Ok(()) | Err(LifecycleError::AlreadyDestroyed) => {}
        Err(LifecycleError::InvalidTransition { .. }) => {
            // Unreachable by construction (see above); ignore defensively.
        }
    }
}

/// Merged lifecycle: the effective `min(parent, manual)` exposed as one
/// [`LifecycleRegistry`].
///
/// Parent registries own the observer lifetime (observers live in the parent
/// registries and capture the merged registry strongly). Dropping this
/// struct unsubscribes nothing — the transient parent→merged retention is
/// broken deterministically when merged reaches `Destroyed` (observers
/// remove themselves) or via the crate-internal shutdown.
/// Teardown hooks call shutdown and, where they own scope end, drive the
/// merged registry to `Destroyed` through the crate-internal driver.
///
/// Reentrancy: side observers never drive the merged registry directly.
/// Each delivery records the freshly computed `min` as the pending target
/// and runs a pump that steps toward the *latest* pending target one step
/// at a time. Nested deliveries (including transitions issued from inside
/// merged observers) only update the pending target; the outermost pump
/// picks them up, so a stale outer target can never oscillate against nested
/// convergence.
#[derive(Debug, Clone)]
pub struct MergedLifecycle {
    inner: Rc<Inner>,
}

#[derive(Debug)]
struct Inner {
    merged: LifecycleRegistry,
    first: LifecycleRegistry,
    second: LifecycleRegistry,
    first_id: Cell<Option<u64>>,
    second_id: Cell<Option<u64>>,
    first_state: Cell<LifecycleState>,
    second_state: Cell<LifecycleState>,
    pending: Cell<Option<LifecycleState>>,
    pumping: Cell<bool>,
}

impl Inner {
    fn pump(&self) {
        if self.pumping.replace(true) {
            return;
        }
        while let Some(target) = self.pending.take() {
            if self.merged.state().is_destroyed() {
                break;
            }
            if target.is_destroyed() {
                // Terminal: full F-08 sequence. Nested side events can only
                // reaffirm destruction; the loop drains them as no-ops.
                if self.merged.state() == LifecycleState::Initialized {
                    let _ = self.merged.create();
                }
                let _ = self.merged.destroy();
                continue;
            }
            if self.merged.state() == target {
                continue;
            }
            step_once_toward(&self.merged, target);
        }
        self.pumping.set(false);
        if self.merged.state().is_destroyed() {
            self.shutdown();
        }
    }

    /// Removes both parent observers, if still present. Idempotent: each id
    /// is taken at most once, and unknown ids unsubscribe to `false`.
    fn shutdown(&self) {
        if let Some(id) = self.first_id.take() {
            let _ = self.first.unsubscribe(id);
        }
        if let Some(id) = self.second_id.take() {
            let _ = self.second.unsubscribe(id);
        }
    }
}

/// One step of the merged registry toward a non-destroyed target.
///
/// Step-method errors are ignored and re-converged by the pump loop:
/// `AlreadyDestroyed` from a reentrant terminal race is idempotent success,
/// and an `InvalidTransition` from a nested state change simply yields to
/// the next loop iteration, which recomputes from fresh state. A backward
/// step to `Initialized` is unreachable by construction (sides never deliver
/// `Initialized` once merged has advanced) and is a defensive no-op.
fn step_once_toward(merged: &LifecycleRegistry, target: LifecycleState) {
    fn rank(state: LifecycleState) -> u8 {
        match state {
            LifecycleState::Initialized => 0,
            LifecycleState::Created => 1,
            LifecycleState::Started => 2,
            LifecycleState::Resumed => 3,
            LifecycleState::Destroyed => 4,
        }
    }
    let current = merged.state();
    if current.is_destroyed() || current == target || target == LifecycleState::Initialized {
        return;
    }
    if rank(current) < rank(target) {
        let _ = match current {
            LifecycleState::Initialized => merged.create(),
            LifecycleState::Created => merged.start(),
            LifecycleState::Started => merged.resume(),
            _ => Ok(()),
        };
    } else {
        let _ = match current {
            LifecycleState::Resumed => merged.pause(),
            LifecycleState::Started => merged.stop(),
            _ => Ok(()),
        };
    }
}

impl MergedLifecycle {
    /// Merges `first` and `second` (parent and manual lifecycles).
    ///
    /// Non-destroyed inputs — whatever their current state — contribute an
    /// initial `Initialized`; replay plus live deliveries advance the merged
    /// registry from there. A destroyed input produces an immediately
    /// `Destroyed` merged registry with zero subscriptions taken.
    #[must_use]
    pub fn new(first: &LifecycleRegistry, second: &LifecycleRegistry) -> Self {
        let inner = Rc::new(Inner {
            merged: LifecycleRegistry::new(),
            first: first.clone(),
            second: second.clone(),
            first_id: Cell::new(None),
            second_id: Cell::new(None),
            first_state: Cell::new(LifecycleState::Initialized),
            second_state: Cell::new(LifecycleState::Initialized),
            pending: Cell::new(None),
            pumping: Cell::new(false),
        });
        let scope = Self { inner };
        if first.state().is_destroyed() || second.state().is_destroyed() {
            drive(&scope.inner.merged, LifecycleState::Destroyed);
            return scope;
        }
        // Subscription callbacks may run synchronously during `subscribe`
        // (replay), i.e. before the returned guard's id is stored below, so
        // observers must not depend on the ids being present yet. Recording
        // the side state and pumping is always safe; id-dependent shutdown
        // only matters on the destroy path, which replay never delivers.
        let observe = |source: &LifecycleRegistry, is_first: bool| {
            let inner = Rc::clone(&scope.inner);
            source.subscribe(move |state| {
                if is_first {
                    inner.first_state.set(state);
                } else {
                    inner.second_state.set(state);
                }
                inner.pending.set(Some(semantic_min(
                    inner.first_state.get(),
                    inner.second_state.get(),
                )));
                inner.pump();
            })
        };
        let first_sub = observe(first, true);
        let second_sub = observe(second, false);
        // A destroy delivered during replay cannot happen (replay never
        // emits Destroyed), so storing ids after subscribing is safe.
        // Detach: parent registries own observer lifetime from here on.
        scope.inner.first_id.set(Some(first_sub.detach()));
        scope.inner.second_id.set(Some(second_sub.detach()));
        scope
    }

    /// The merged registry handle. Clones share state and observer list.
    #[must_use]
    pub fn lifecycle(&self) -> LifecycleRegistry {
        self.inner.merged.clone()
    }

    /// Current merged state.
    #[must_use]
    pub fn state(&self) -> LifecycleState {
        self.inner.merged.state()
    }

    /// Currently registered parent-observer ids, if any. Copied out without
    /// taking: [`shutdown`](MergedLifecycle::shutdown) stays authoritative
    /// for release. `None` on a side means no subscription exists there
    /// (born-destroyed merged, or already shut down).
    pub(crate) fn observer_ids(&self) -> (Option<u64>, Option<u64>) {
        (self.inner.first_id.get(), self.inner.second_id.get())
    }

    /// Removes both parent observers, if still present. Idempotent.
    pub(crate) fn shutdown(&self) {
        self.inner.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resumed() -> LifecycleRegistry {
        let lifecycle = LifecycleRegistry::new();
        lifecycle.create().unwrap();
        lifecycle.start().unwrap();
        lifecycle.resume().unwrap();
        lifecycle
    }

    fn log(lifecycle: &LifecycleRegistry) -> Rc<std::cell::RefCell<Vec<LifecycleState>>> {
        let events = Rc::new(std::cell::RefCell::new(Vec::new()));
        let probe = Rc::clone(&events);
        let _log_sub = lifecycle
            .subscribe(move |state| probe.borrow_mut().push(state))
            .detach();
        events
    }

    #[test]
    fn non_destroyed_inputs_begin_initialized() {
        let fresh_parent = LifecycleRegistry::new();
        let fresh_manual = LifecycleRegistry::new();
        let merged = MergedLifecycle::new(&fresh_parent, &fresh_manual);
        assert_eq!(merged.state(), LifecycleState::Initialized);
        // Whatever the inputs' current states, construction treats them as
        // Initialized until replay/live deliveries run: replay of two
        // Resumed inputs advances the merged registry all the way.
        let replayed = MergedLifecycle::new(&resumed(), &resumed());
        assert_eq!(replayed.state(), LifecycleState::Resumed);
    }

    #[test]
    fn destroyed_input_born_destroyed_with_zero_subscriptions() {
        let parent = LifecycleRegistry::new();
        let manual = LifecycleRegistry::new();
        manual.destroy().unwrap();
        let before_parent = parent.subscriber_count();
        let before_manual = manual.subscriber_count();
        let merged = MergedLifecycle::new(&parent, &manual);
        assert_eq!(merged.state(), LifecycleState::Destroyed);
        assert_eq!(parent.subscriber_count(), before_parent);
        assert_eq!(manual.subscriber_count(), before_manual);

        let parent2 = LifecycleRegistry::new();
        parent2.destroy().unwrap();
        let manual2 = LifecycleRegistry::new();
        let merged2 = MergedLifecycle::new(&parent2, &manual2);
        assert_eq!(merged2.state(), LifecycleState::Destroyed);
        assert_eq!(manual2.subscriber_count(), 0);
    }

    #[test]
    fn forward_table_both_sides_required() {
        let parent = LifecycleRegistry::new();
        let manual = LifecycleRegistry::new();
        let merged = MergedLifecycle::new(&parent, &manual);
        let events = log(&merged.lifecycle());

        // One side alone never advances.
        parent.create().unwrap();
        parent.start().unwrap();
        parent.resume().unwrap();
        assert_eq!(merged.state(), LifecycleState::Initialized);
        assert!(events.borrow().is_empty());

        // CREATED x CREATED -> ON_CREATE.
        manual.create().unwrap();
        assert_eq!(merged.state(), LifecycleState::Created);
        assert_eq!(*events.borrow(), vec![LifecycleState::Created]);

        // STARTED x STARTED -> ON_CREATE, ON_START.
        manual.start().unwrap();
        assert_eq!(merged.state(), LifecycleState::Started);
        assert_eq!(
            *events.borrow(),
            vec![LifecycleState::Created, LifecycleState::Started]
        );

        // RESUMED x RESUMED -> + ON_RESUME.
        manual.resume().unwrap();
        assert_eq!(merged.state(), LifecycleState::Resumed);
        assert_eq!(
            *events.borrow(),
            vec![
                LifecycleState::Created,
                LifecycleState::Started,
                LifecycleState::Resumed
            ]
        );
    }

    #[test]
    fn either_side_limits_progress() {
        let parent = resumed();
        let manual = LifecycleRegistry::new();
        manual.create().unwrap();
        let merged = MergedLifecycle::new(&parent, &manual);
        // Replay: parent Resumed, manual Created -> min is Created.
        assert_eq!(merged.state(), LifecycleState::Created);
    }

    #[test]
    fn reverse_transitions_follow_min() {
        let parent = resumed();
        let manual = resumed();
        let merged = MergedLifecycle::new(&parent, &manual);
        assert_eq!(merged.state(), LifecycleState::Resumed);
        let events = log(&merged.lifecycle());
        // Subscription replay caught the logger up; observe only new moves.
        events.borrow_mut().clear();

        // Single pause -> ON_PAUSE (merged Started).
        parent.pause().unwrap();
        assert_eq!(merged.state(), LifecycleState::Started);
        assert_eq!(*events.borrow(), vec![LifecycleState::Started]);

        // Single stop -> ON_PAUSE, ON_STOP (merged Created).
        parent.stop().unwrap();
        assert_eq!(merged.state(), LifecycleState::Created);
        assert_eq!(
            *events.borrow(),
            vec![LifecycleState::Started, LifecycleState::Created]
        );
    }

    #[test]
    fn any_destroy_drives_pause_stop_destroy_and_second_is_silent() {
        let parent = resumed();
        let manual = resumed();
        let merged = MergedLifecycle::new(&parent, &manual);
        let events = log(&merged.lifecycle());
        // Subscription replay caught the logger up; observe only new moves.
        events.borrow_mut().clear();

        manual.destroy().unwrap();
        assert_eq!(merged.state(), LifecycleState::Destroyed);
        assert_eq!(
            *events.borrow(),
            vec![
                LifecycleState::Started,
                LifecycleState::Created,
                LifecycleState::Destroyed
            ]
        );
        // Second destroy (other side) is silent.
        let len = events.borrow().len();
        parent.destroy().unwrap();
        assert_eq!(events.borrow().len(), len);
    }

    #[test]
    fn initialized_to_destroyed_observes_created_then_destroyed() {
        let parent = LifecycleRegistry::new();
        let manual = LifecycleRegistry::new();
        let merged = MergedLifecycle::new(&parent, &manual);
        let events = log(&merged.lifecycle());
        parent.destroy().unwrap();
        assert_eq!(
            *events.borrow(),
            vec![LifecycleState::Created, LifecycleState::Destroyed]
        );
    }

    #[test]
    fn destroy_unsubscribes_all_and_post_destroy_moves_have_no_effect() {
        let parent = LifecycleRegistry::new();
        let manual = LifecycleRegistry::new();
        parent.create().unwrap();
        parent.start().unwrap();
        manual.create().unwrap();
        manual.start().unwrap();
        let merged = MergedLifecycle::new(&parent, &manual);
        assert_eq!(parent.subscriber_count(), 1);
        assert_eq!(manual.subscriber_count(), 1);

        manual.destroy().unwrap();
        assert_eq!(merged.state(), LifecycleState::Destroyed);
        // Both parent observers removed themselves on the destroy path.
        assert_eq!(parent.subscriber_count(), 0);
        assert_eq!(manual.subscriber_count(), 0);

        // Later parent transitions have no effect (and cannot fail the
        // destroyed merged registry observably).
        parent.resume().unwrap();
        parent.pause().unwrap();
        assert_eq!(merged.state(), LifecycleState::Destroyed);
    }

    #[test]
    fn post_destroy_driver_calls_are_silent_noops() {
        let merged_registry = LifecycleRegistry::new();
        merged_registry.create().unwrap();
        let events = log(&merged_registry);
        merged_registry.destroy().unwrap();
        let len = events.borrow().len();
        drive(&merged_registry, LifecycleState::Destroyed);
        drive(&merged_registry, LifecycleState::Resumed);
        assert_eq!(events.borrow().len(), len);
        assert_eq!(merged_registry.state(), LifecycleState::Destroyed);
    }

    #[test]
    fn reentrant_transitions_from_inside_observers_converge() {
        let parent = LifecycleRegistry::new();
        let manual = LifecycleRegistry::new();
        let merged = MergedLifecycle::new(&parent, &manual);
        let child = merged.lifecycle();
        // From inside a merged Created observer, advance the manual side;
        // nested transitions must converge without borrow panics.
        let manual_peer = manual.clone();
        let _reentrant_sub = child
            .subscribe(move |state| {
                if state == LifecycleState::Created {
                    manual_peer.start().unwrap();
                }
            })
            .detach();
        parent.create().unwrap();
        parent.start().unwrap();
        // Merged goes Created -> (nested) Started and converges.
        manual.create().unwrap();
        assert_eq!(merged.state(), LifecycleState::Started);
    }

    #[test]
    fn shutdown_is_idempotent_and_releases_parents() {
        let parent = LifecycleRegistry::new();
        let manual = LifecycleRegistry::new();
        let merged = MergedLifecycle::new(&parent, &manual);
        assert_eq!(parent.subscriber_count(), 1);
        merged.shutdown();
        merged.shutdown();
        assert_eq!(parent.subscriber_count(), 0);
        assert_eq!(manual.subscriber_count(), 0);
    }
}
