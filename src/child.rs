//! Child-scope construction: the normative 9-step sequence.
//!
//! ```text
//! 1. validate supplied lifecycle state (destroyed -> Err, no effects)
//! 2. duplicate state-key check BEFORE consuming restored bytes
//! 3. consume restored child-state bytes
//! 4. create/register child state keeper (DCS1 provider)
//! 5. obtain retained child InstanceKeeper via get_or_create (record pre-existence)
//! 6. create/register back bridge
//! 7. merged lifecycle + subscriptions in dependency order
//! 8. build ContextParts
//! 9. invoke custom context factory LAST
//! ```
//!
//! The only ordering note: the manual-destroy guard is subscribed first
//! *within* step 7 (so reverse-delivery fires it last, after functional
//! cleanup), while the back-bridge gate subscription from step 6 fires after
//! it. That residual ordering is unobservable — the bridge is already
//! stopped by teardown before the guard panics — and post-panic observer
//! cleanup is explicitly unspecified.
//!
//! [`Rollback`] removes exactly the resources created by the failed attempt:
//! a pre-existing retained instance scope is never destroyed, and `Drop`
//! uses only panic-resistant best-effort paths (no `unwrap`, `try_borrow`
//! for keepers, busy-aware [`SharedBack`] methods, idempotent Essenty
//! removals).

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use essenty_back_handler::BackPhase;
use essenty_instance_keeper::InstanceKeeper;
use essenty_lifecycle::{LifecycleRegistry, LifecycleState};
use essenty_state_keeper::{StateKeeper, StateKeeperError};

use crate::context::{
    ChildKey, ContextFactory, ContextParts, MANUAL_DESTROY_PANIC_MESSAGE, SharedInstances,
    SharedKeeper,
};
use crate::error::DecomposeError;
use crate::framing;
use crate::merged::{self, MergedLifecycle};
use crate::shared_back::SharedBack;

const EMPTY_MAP: BTreeMap<String, Vec<u8>> = BTreeMap::new();

fn manual_destroy_panic() {
    panic!("{MANUAL_DESTROY_PANIC_MESSAGE}");
}

struct BackRollback {
    parent: SharedBack,
    handle_id: u64,
    child: SharedBack,
    listener_id: u64,
}

struct Rollback {
    state: Option<(SharedKeeper, String)>,
    instance: Option<(SharedInstances, String)>,
    back: Option<BackRollback>,
    merged: Option<MergedLifecycle>,
    /// Fixed-child instance-cascade hook, which lives on the parent
    /// lifecycle and is therefore parent-visible. Manual-path hooks live on
    /// the caller-owned manual registry instead: they die with that registry
    /// (manual destroy clears them) and are not rollback-tracked.
    fixed_hook: Option<(LifecycleRegistry, u64)>,
    defused: bool,
}

impl Rollback {
    fn new() -> Self {
        Self {
            state: None,
            instance: None,
            back: None,
            merged: None,
            fixed_hook: None,
            defused: false,
        }
    }
}

impl Drop for Rollback {
    fn drop(&mut self) {
        if self.defused {
            return;
        }
        // Best-effort and panic-resistant: keeper access via try_borrow
        // (skipped on conflict), SharedBack methods (busy-aware, never
        // borrowing across dispatch), idempotent Essenty removals.
        if let Some((state, key)) = self.state.take()
            && let Ok(mut keeper) = state.try_borrow_mut()
        {
            keeper.unregister(&key);
        }
        // Only entries created by this attempt are tracked here; a
        // pre-existing retained scope is never destroyed by rollback.
        if let Some((instances, key)) = self.instance.take()
            && let Ok(mut scope) = instances.try_borrow_mut()
        {
            scope.destroy(&key);
        }
        if let Some(bridge) = self.back.take() {
            bridge.parent.unregister(bridge.handle_id);
            bridge
                .child
                .remove_enabled_changed_listener(bridge.listener_id);
        }
        if let Some(merged) = self.merged.take() {
            // Releases parent/manual observers. Safe during unwind: Essenty
            // holds no registry borrow across observer delivery, and no
            // back-dispatch frame borrows a lifecycle registry.
            merged.shutdown();
        }
        if let Some((lifecycle, id)) = self.fixed_hook.take() {
            let _ = lifecycle.unsubscribe(id);
        }
    }
}

/// Builds one child scope, then constructs `C` through `factory`.
///
/// `manual` is `None` for fixed children (shared parent lifecycle) and
/// `Some` for manually-controlled children (merged lifecycle). See the
/// module docs for the normative sequence and rollback contract.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_child<C>(
    parent_lifecycle: &LifecycleRegistry,
    parent_state: &SharedKeeper,
    parent_instances: &SharedInstances,
    parent_back: &SharedBack,
    key: ChildKey,
    manual: Option<&LifecycleRegistry>,
    priority: i32,
    factory: ContextFactory<C>,
) -> Result<C, DecomposeError> {
    let key_str = key.as_str().to_owned();

    // Step 1: validate supplied lifecycle state before any side effect.
    if let Some(manual) = manual
        && manual.state().is_destroyed()
    {
        return Err(DecomposeError::ChildLifecycleDestroyed);
    }

    let mut rollback = Rollback::new();

    // Step 2: duplicate state-key check BEFORE consuming restored bytes
    // (failure atomicity: DuplicateKey consumes nothing and creates nothing).
    if parent_state.borrow().is_registered(key.as_str()) {
        return Err(DecomposeError::DuplicateKey { key: key_str });
    }

    // Step 3: consume restored child-state bytes once.
    let restored = parent_state.borrow_mut().consume_bytes(key.as_str());

    // Step 4: decode DCS1 and create/register the child state keeper. The
    // registered provider captures a STRONG Rc to the child keeper
    // (mandatory): the registration itself keeps the child saveable with
    // zero external handles alive. The graph parent -> provider -> child has
    // no back edge, so no cycle.
    let seeded: BTreeMap<String, Vec<u8>> = match restored {
        None => BTreeMap::new(),
        Some(bytes) => framing::decode(&bytes).map_err(|reason| {
            DecomposeError::StateKeeper(StateKeeperError::Decode {
                key: key_str.clone(),
                reason: reason.to_string(),
            })
        })?,
    };
    let child_state: SharedKeeper = Rc::new(RefCell::new(StateKeeper::with_restored(seeded)));
    {
        let owned = Rc::clone(&child_state);
        parent_state
            .borrow_mut()
            .register(key.as_str(), move || {
                // Save is infallible for byte providers; a nested codec
                // failure or borrow conflict yields an empty frame rather
                // than aborting the parent save.
                if let Ok(keeper) = owned.try_borrow()
                    && let Ok(saved) = keeper.save()
                {
                    return framing::encode(&saved);
                }
                framing::encode(&EMPTY_MAP)
            })
            .map_err(DecomposeError::StateKeeper)?;
    }
    rollback.state = Some((Rc::clone(parent_state), key_str.clone()));

    // Step 5: obtain the retained child instance scope via get_or_create
    // (never put: recreation preserves Rc::ptr_eq identity). Pre-existence
    // is recorded so rollback never destroys a retained scope it did not
    // create. A destroyed parent scope surfaces Destroyed here (F-09).
    let pre_existed = parent_instances.borrow().contains(key.as_str());
    let child_instances: SharedInstances = parent_instances
        .borrow_mut()
        .get_or_create(key.as_str(), || RefCell::new(InstanceKeeper::new()))
        .map_err(DecomposeError::InstanceKeeper)?;
    if !pre_existed {
        rollback.instance = Some((Rc::clone(parent_instances), key_str.clone()));
    }

    // Step 6: create/register the back bridge. One parent callback captures
    // a STRONG child SharedBack; the child's enabled listener captures a
    // WEAK parent link (the one genuine cycle, broken by upgrade-per-call).
    // Registration into a busy same-dispatcher rejects with
    // DispatchInProgress -> BackDispatchInProgress with full rollback.
    let child_back = SharedBack::new();
    let gate = Rc::new(Cell::new(true));
    let child_enabled = Rc::new(Cell::new(child_back.can_handle()));
    let initial_enabled = match manual {
        None => child_enabled.get(),
        Some(manual) => manual.state().is_started() && child_enabled.get(),
    };
    let child_peer = child_back.clone();
    let parent_handle = parent_back
        .register(priority, initial_enabled, move |event, _| {
            // Synchronous cross-dispatcher forward to the child's current
            // owner. Claimed-owner stability, mid-gesture priority immunity,
            // and removal-cancels-owner are inner-Essenty behaviors the
            // wrapper delegates untouched. A child with no gesture in flight
            // reports NoGestureInProgress on progress/cancel/invoke; the
            // parent already selected the child as a unit, so the outcome is
            // coherently ignored (never propagated, never queued).
            match event.phase {
                BackPhase::Started => {
                    if let Some(position) = event.position {
                        let _ = child_peer.predictive_start_with(position);
                    } else {
                        let _ = child_peer.predictive_start();
                    }
                }
                BackPhase::Progressed => {
                    let progress = event.progress.unwrap_or(0.0);
                    if let Some(position) = event.position {
                        let _ = child_peer.predictive_progress_with(progress, position);
                    } else {
                        let _ = child_peer.predictive_progress(progress);
                    }
                }
                BackPhase::Cancelled => {
                    let _ = child_peer.predictive_cancel();
                }
                BackPhase::Invoked => {
                    let _ = child_peer.back();
                }
            }
        })
        .map_err(DecomposeError::from)?;
    let handle_id = parent_handle.id();
    // R-03 normative bridge logic: parent_bridge_enabled =
    // lifecycle_gate && NEW evented child aggregate. Never
    // child.can_handle() here: during a busy window it intentionally
    // reflects the cached pre-drain aggregate.
    let listener_id = {
        let weak = parent_back.downgrade();
        let gate = Rc::clone(&gate);
        let child_enabled = Rc::clone(&child_enabled);
        child_back.add_enabled_changed_listener(move |new_child_enabled| {
            child_enabled.set(new_child_enabled);
            if let Some(parent) = weak.upgrade() {
                parent.set_enabled(handle_id, gate.get() && new_child_enabled);
            }
        })
    };
    rollback.back = Some(BackRollback {
        parent: parent_back.clone(),
        handle_id,
        child: child_back.clone(),
        listener_id,
    });
    // Start-bridge gate: fixed children are always open; manual children
    // follow the manual lifecycle's Started state (upstream gate). The gate
    // subscription reads the stored evented aggregate, never can_handle().
    if let Some(manual) = manual {
        gate.set(manual.state().is_started());
        let weak = parent_back.downgrade();
        let gate = Rc::clone(&gate);
        let child_enabled = Rc::clone(&child_enabled);
        let _gate_sub = manual
            .subscribe(move |state| {
                let open = state.is_started();
                gate.set(open);
                if let Some(parent) = weak.upgrade() {
                    parent.set_enabled(handle_id, open && child_enabled.get());
                }
            })
            .detach();
    }

    // Step 7: merged lifecycle + subscriptions in dependency order.
    let child_lifecycle: LifecycleRegistry;
    if let Some(manual) = manual {
        // Destroy-contract guard FIRST so reverse-delivery fires it LAST,
        // after functional cleanup has run. Never fires at subscription time
        // here: already-destroyed input was rejected in step 1.
        let _guard = manual.do_on_destroy(manual_destroy_panic).detach();
        let merged = MergedLifecycle::new(parent_lifecycle, manual);
        let parent_observer_id = merged.observer_ids().0;
        child_lifecycle = merged.lifecycle();
        // Child-scope end marks the nested keeper destroyed immediately
        // (bookkeeping; retained-value Drop follows last-Rc release).
        {
            let owned = Rc::clone(&child_instances);
            let _cascade = child_lifecycle
                .do_on_destroy(move || {
                    if let Ok(mut scope) = owned.try_borrow_mut() {
                        scope.destroy_all();
                    }
                })
                .detach();
        }
        // Teardown hook LAST so it fires FIRST: unregister state key,
        // destroy instance entry, end child scope, stop the back bridge,
        // release the parent merged observer, then drive merged destroyed.
        // Captures never include the manual registry itself (which owns this
        // hook): no reference cycle. All steps idempotent.
        {
            let state = Rc::clone(parent_state);
            let instances = Rc::clone(parent_instances);
            let scope = Rc::clone(&child_instances);
            let bridge = parent_back.clone();
            let peer = child_back.clone();
            let parents = parent_lifecycle.clone();
            let merged_registry = child_lifecycle.clone();
            let key = key_str.clone();
            let _teardown = manual
                .do_on_destroy(move || {
                    if let Ok(mut keeper) = state.try_borrow_mut() {
                        keeper.unregister(&key);
                    }
                    if let Ok(mut retained) = instances.try_borrow_mut() {
                        retained.destroy(&key);
                    }
                    if let Ok(mut scope) = scope.try_borrow_mut() {
                        scope.destroy_all();
                    }
                    bridge.unregister(handle_id);
                    peer.remove_enabled_changed_listener(listener_id);
                    if let Some(id) = parent_observer_id {
                        let _ = parents.unsubscribe(id);
                    }
                    merged::drive(&merged_registry, LifecycleState::Destroyed);
                })
                .detach();
        }
        rollback.merged = Some(merged);
    } else {
        child_lifecycle = parent_lifecycle.clone();
        // Fixed children persist to parent-scope end. Rust-ownership
        // adaptation (documented): a destroy hook on the shared lifecycle
        // marks the nested keeper destroyed deterministically when the
        // parent scope ends. Upstream achieves this through GC-owned
        // provider destruction; the hook is the explicit equivalent.
        let owned = Rc::clone(&child_instances);
        let hook_id = parent_lifecycle
            .do_on_destroy(move || {
                if let Ok(mut scope) = owned.try_borrow_mut() {
                    scope.destroy_all();
                }
            })
            .detach();
        rollback.fixed_hook = Some((parent_lifecycle.clone(), hook_id));
    }

    // Step 8: build ContextParts.
    let parts = ContextParts::new(
        child_lifecycle,
        Rc::clone(&child_state),
        Rc::clone(&child_instances),
        child_back,
    );

    // Step 9: invoke the custom context factory LAST. A panic unwinds
    // through Rollback (removing only attempt-created resources; a
    // pre-existing retained scope keeps Rc::ptr_eq identity) and propagates.
    let context = factory(parts);
    rollback.defused = true;
    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ComponentContext, DefaultComponentContext};
    use crate::testing::TestContext;
    use essenty_back_handler::{BackEvent, BackPhase};
    use essenty_instance_keeper::InstanceKeeperError;

    fn manual() -> LifecycleRegistry {
        LifecycleRegistry::new()
    }

    fn new_phase_log() -> Rc<RefCell<Vec<BackPhase>>> {
        Rc::new(RefCell::new(Vec::new()))
    }

    fn record(log: Rc<RefCell<Vec<BackPhase>>>) -> impl FnMut(BackEvent, &SharedBack) {
        move |event: BackEvent, _: &SharedBack| log.borrow_mut().push(event.phase)
    }

    // -- Fixed children ----------------------------------------------------

    #[test]
    fn fixed_child_shares_parent_lifecycle() {
        let parent = TestContext::initialized();
        let child = parent.child_context("c").unwrap();
        assert_eq!(child.lifecycle().state(), LifecycleState::Initialized);
        parent.lifecycle().create().unwrap();
        parent.lifecycle().start().unwrap();
        assert_eq!(child.lifecycle().state(), LifecycleState::Started);
        parent.lifecycle().destroy().unwrap();
        assert_eq!(child.lifecycle().state(), LifecycleState::Destroyed);
    }

    #[test]
    fn fixed_child_registers_all_three_namespaces() {
        let parent = TestContext::new();
        let _child = parent.child_context("c").unwrap();
        assert!(parent.state_keeper().borrow().is_registered("c"));
        assert!(parent.instance_keeper().borrow().contains("c"));
        assert_eq!(parent.back_handler().handler_count(), 1);
        // Fixed children add exactly one parent-lifecycle observer: the
        // instance-cascade hook (documented Rust-ownership adaptation).
        assert_eq!(parent.lifecycle().subscriber_count(), 1);
    }

    #[test]
    fn duplicate_key_reports_key_and_consumes_nothing() {
        let parent = TestContext::new();
        let _child = parent.child_context("dup").unwrap();
        let err = parent.child_context("dup").unwrap_err();
        assert_eq!(
            err,
            DecomposeError::DuplicateKey {
                key: "dup".to_owned()
            }
        );
        assert!(err.to_string().contains("dup"));
    }

    #[test]
    fn duplicate_rejection_leaves_restored_bytes_consumable() {
        let parent = TestContext::new();
        parent
            .state_keeper()
            .borrow_mut()
            .register("k", || b"live".to_vec())
            .unwrap();
        let saved = BTreeMap::from([("k".to_owned(), b"stale".to_vec())]);
        let parent = parent.recreate(false, saved);
        // Provider registered (e.g. by an earlier sibling scope in the same
        // recreation generation): duplicate check fires BEFORE consume.
        parent
            .state_keeper()
            .borrow_mut()
            .register("k", || b"live".to_vec())
            .unwrap();
        let err = parent.child_context("k").unwrap_err();
        assert_eq!(
            err,
            DecomposeError::DuplicateKey {
                key: "k".to_owned()
            }
        );
        assert!(parent.state_keeper().borrow().has_consumable("k"));
        assert!(!parent.instance_keeper().borrow().contains("k"));
        assert_eq!(parent.back_handler().handler_count(), 0);
    }

    // -- Manual children ---------------------------------------------------

    #[test]
    fn manual_child_follows_min_of_parent_and_manual() {
        let parent = TestContext::new();
        let manual = manual();
        let child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        assert_eq!(child.lifecycle().state(), LifecycleState::Initialized);
        manual.create().unwrap();
        assert_eq!(child.lifecycle().state(), LifecycleState::Created);
        manual.start().unwrap();
        assert_eq!(child.lifecycle().state(), LifecycleState::Started);
        manual.resume().unwrap();
        assert_eq!(child.lifecycle().state(), LifecycleState::Resumed);
    }

    #[test]
    fn manual_child_limited_by_parent_side() {
        let parent = TestContext::initialized();
        let manual = LifecycleRegistry::new();
        manual.create().unwrap();
        manual.start().unwrap();
        manual.resume().unwrap();
        let child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        // Replay: parent Initialized pins the child despite manual Resumed.
        assert_eq!(child.lifecycle().state(), LifecycleState::Initialized);
        parent.lifecycle().create().unwrap();
        parent.lifecycle().start().unwrap();
        parent.lifecycle().resume().unwrap();
        assert_eq!(child.lifecycle().state(), LifecycleState::Resumed);
    }

    #[test]
    fn manual_destroy_panics_with_contract_and_stays_destroyed() {
        let parent = TestContext::new();
        let manual = manual();
        let _child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            manual.destroy().unwrap();
        }));
        let err = result.unwrap_err();
        let message = err
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| err.downcast_ref::<&str>().copied());
        assert_eq!(message, Some(MANUAL_DESTROY_PANIC_MESSAGE));
        assert!(manual.state().is_destroyed());
        // Functional cleanup ran before the panic: provider released.
        assert!(!parent.state_keeper().borrow().is_registered("m"));
        // Second destroy is silent (no observers left to panic).
        assert!(manual.destroy().is_err());
    }

    #[test]
    fn creation_time_destroyed_input_is_err_with_zero_side_effects() {
        let parent = TestContext::new();
        let frame = crate::framing::encode(&BTreeMap::from([("v".to_owned(), vec![1])]));
        let parent = parent.recreate(false, BTreeMap::from([("m".to_owned(), frame)]));
        let manual = manual();
        manual.destroy().unwrap();
        let err = parent
            .child_context_with_lifecycle("m", &manual)
            .unwrap_err();
        assert_eq!(err, DecomposeError::ChildLifecycleDestroyed);
        assert_eq!(parent.state_keeper().borrow().provider_count(), 0);
        assert!(parent.state_keeper().borrow().has_consumable("m"));
        assert!(!parent.instance_keeper().borrow().contains("m"));
        assert_eq!(parent.back_handler().handler_count(), 0);
        assert_eq!(parent.lifecycle().subscriber_count(), 0);
    }

    // -- State namespace ---------------------------------------------------

    #[test]
    fn state_round_trip_through_parent_save() {
        let parent = TestContext::new();
        let child = parent.child_context("c").unwrap();
        child
            .state_keeper()
            .borrow_mut()
            .register("inner", || b"value-bytes".to_vec())
            .unwrap();
        let saved = parent.save_bytes();
        assert!(saved.contains_key("c"));

        let parent2 = parent.recreate(false, saved);
        assert!(parent2.state_keeper().borrow().has_consumable("c"));
        let child2 = parent2.child_context("c").unwrap();
        let restored = child2.state_keeper().borrow_mut().consume_bytes("inner");
        assert_eq!(restored, Some(b"value-bytes".to_vec()));
        // Single-shot consume: second read is empty.
        assert_eq!(
            child2.state_keeper().borrow_mut().consume_bytes("inner"),
            None
        );
    }

    #[test]
    fn unregister_on_manual_destroy_exactly_once_then_never_saved() {
        let parent = TestContext::new();
        let manual = manual();
        let _child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        assert!(parent.state_keeper().borrow().is_registered("m"));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            manual.destroy().unwrap();
        }));
        assert!(!parent.state_keeper().borrow().is_registered("m"));
        // Destroyed-lifecycle child is never saved afterwards.
        let saved = parent.save_bytes();
        assert!(!saved.contains_key("m"));
    }

    #[test]
    fn nested_grandchildren_chain_through_frames() {
        let parent = TestContext::new();
        let child = parent.child_context("c").unwrap();
        let grandchild = child.child_context("g").unwrap();
        grandchild
            .state_keeper()
            .borrow_mut()
            .register("v", || vec![7, 7, 7])
            .unwrap();
        let saved = parent.save_bytes();
        let child_frame = crate::framing::decode(&saved["c"]).unwrap();
        assert!(child_frame.contains_key("g"));
        let grand_frame = crate::framing::decode(&child_frame["g"]).unwrap();
        assert_eq!(grand_frame["v"], vec![7, 7, 7]);
    }

    #[test]
    fn siblings_reuse_internal_keys_disjointly() {
        let parent = TestContext::new();
        let a = parent.child_context("a").unwrap();
        let b = parent.child_context("b").unwrap();
        a.state_keeper()
            .borrow_mut()
            .register("inner", || b"A".to_vec())
            .unwrap();
        b.state_keeper()
            .borrow_mut()
            .register("inner", || b"B".to_vec())
            .unwrap();
        let saved = parent.save_bytes();
        let fa = crate::framing::decode(&saved["a"]).unwrap();
        let fb = crate::framing::decode(&saved["b"]).unwrap();
        assert_eq!(fa["inner"], b"A");
        assert_eq!(fb["inner"], b"B");
    }

    #[test]
    fn malformed_restored_bytes_map_to_decode_without_registration() {
        let parent = TestContext::new();
        let parent = parent.recreate(false, BTreeMap::from([("k".to_owned(), b"junk".to_vec())]));
        let err = parent.child_context("k").unwrap_err();
        assert!(matches!(
            err,
            DecomposeError::StateKeeper(essenty_state_keeper::StateKeeperError::Decode { .. })
        ));
        assert!(err.to_string().contains('k'));
        assert!(!parent.state_keeper().borrow().is_registered("k"));
    }

    // -- Instance namespace ------------------------------------------------

    #[test]
    fn instance_identity_across_config_change_fresh_on_death() {
        let parent = TestContext::new();
        let child = parent.child_context("c").unwrap();
        let scope_before = child.instance_keeper();
        child
            .instance_keeper()
            .borrow_mut()
            .put("v", 42u32)
            .unwrap();
        let saved = parent.save_bytes();

        // Config change retains the scope AND the value identity.
        let retained = parent.recreate(true, saved.clone());
        let child_cfg = retained.child_context("c").unwrap();
        assert!(Rc::ptr_eq(&child_cfg.instance_keeper(), &scope_before));
        let value: Option<Rc<u32>> = child_cfg.instance_keeper().borrow().get("v").unwrap();
        assert_eq!(*value.unwrap(), 42);

        // Process death replaces the scope.
        let reborn = parent.recreate(false, saved);
        let child_death = reborn.child_context("c").unwrap();
        assert!(!Rc::ptr_eq(&child_death.instance_keeper(), &scope_before));
        assert!(child_death.instance_keeper().borrow().is_empty());
    }

    #[test]
    fn parent_destroy_marks_nested_scopes_destroyed() {
        // Fixed child: parent-scope end cascades deterministically.
        let parent = TestContext::new();
        let child = parent.child_context("c").unwrap();
        parent.lifecycle().destroy().unwrap();
        assert!(child.instance_keeper().borrow().is_destroyed());

        // Manual child: parent destroy drives merged destroyed, and the
        // merged-destroy cascade ends the nested scope.
        let parent = TestContext::new();
        let manual = manual();
        let child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        assert_eq!(child.lifecycle().state(), LifecycleState::Initialized);
        parent.lifecycle().destroy().unwrap();
        assert_eq!(child.lifecycle().state(), LifecycleState::Destroyed);
        assert!(child.instance_keeper().borrow().is_destroyed());
    }

    #[test]
    fn manual_destroy_removes_instance_entry_once() {
        let parent = TestContext::new();
        let manual = manual();
        let _child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        assert!(parent.instance_keeper().borrow().contains("m"));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            manual.destroy().unwrap();
        }));
        assert!(!parent.instance_keeper().borrow().contains("m"));
    }

    #[test]
    fn destroyed_keeper_scope_rejects_with_instance_destroyed() {
        // F-09: a destroyed parent *keeper* is distinct from a destroyed
        // parent *lifecycle*. Keeper end surfaces Destroyed (with rollback of
        // the state registration that preceded it).
        let parent = TestContext::new();
        parent.instance_keeper().borrow_mut().destroy_all();
        let err = parent.child_context("k").unwrap_err();
        assert_eq!(
            err,
            DecomposeError::InstanceKeeper(InstanceKeeperError::Destroyed)
        );
        assert!(!parent.state_keeper().borrow().is_registered("k"));
    }

    #[test]
    fn destroyed_parent_lifecycle_allows_born_destroyed_child() {
        // F-09, other half: parent lifecycle destroyed but keeper alive —
        // creation is allowed; fixed and merged scopes are born destroyed.
        let parent = TestContext::new();
        parent.lifecycle().destroy().unwrap();
        let fixed = parent.child_context("f").unwrap();
        assert_eq!(fixed.lifecycle().state(), LifecycleState::Destroyed);
        assert!(parent.state_keeper().borrow().is_registered("f"));
        assert!(parent.save_bytes().contains_key("f"));

        let manual = LifecycleRegistry::new();
        manual.create().unwrap();
        let merged = parent.child_context_with_lifecycle("m", &manual).unwrap();
        assert_eq!(merged.lifecycle().state(), LifecycleState::Destroyed);
        // Born-destroyed merged takes zero subscriptions on either side.
        assert_eq!(parent.lifecycle().subscriber_count(), 0);
    }

    #[test]
    fn type_mismatch_leaves_value_and_rolls_back_state() {
        let parent = TestContext::new();
        parent
            .instance_keeper()
            .borrow_mut()
            .put("k", "foreign".to_owned())
            .unwrap();
        let err = parent.child_context("k").unwrap_err();
        assert_eq!(
            err,
            DecomposeError::InstanceKeeper(InstanceKeeperError::TypeMismatch("k".to_owned()))
        );
        // Retained value intact; state registration rolled back.
        let kept: Option<Rc<String>> = parent.instance_keeper().borrow().get("k").unwrap();
        assert_eq!(kept.unwrap().as_str(), "foreign");
        assert!(!parent.state_keeper().borrow().is_registered("k"));
        assert_eq!(parent.back_handler().handler_count(), 0);
    }

    // -- Back hierarchy ----------------------------------------------------

    #[test]
    fn back_gate_matrix_lifecycle_gate_and_child_aggregate() {
        let parent = TestContext::new();
        let manual = manual();
        let child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        let log = new_phase_log();
        let handle = child
            .back_handler()
            .register(0, false, record(Rc::clone(&log)))
            .unwrap();

        // Gate closed, child disabled -> false.
        assert!(!parent.back_handler().back());
        // Gate closed, child enabled -> still false.
        child.back_handler().set_enabled(handle.id(), true);
        assert!(!parent.back_handler().back());
        assert!(log.borrow().is_empty());

        // Open the gate (manual reaches Started through Resumed).
        manual.create().unwrap();
        assert!(!parent.back_handler().back()); // Created + enabled -> false
        manual.start().unwrap();
        manual.resume().unwrap();
        assert!(parent.back_handler().back());
        assert_eq!(*log.borrow(), vec![BackPhase::Invoked]);

        // Closing the gate mid-life disables routing without unregistering.
        manual.pause().unwrap();
        manual.stop().unwrap();
        assert!(!parent.back_handler().back());
        manual.start().unwrap();
        assert!(parent.back_handler().back());
        assert_eq!(log.borrow().len(), 2);
    }

    #[test]
    fn manual_destroy_stops_bridge() {
        let parent = TestContext::new();
        let manual = manual();
        manual.create().unwrap();
        manual.start().unwrap();
        let child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        child.back_handler().register(0, true, |_, _| {}).unwrap();
        assert_eq!(parent.back_handler().handler_count(), 1);
        assert!(parent.back_handler().back());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            manual.destroy().unwrap();
        }));
        assert_eq!(parent.back_handler().handler_count(), 0);
        assert!(!parent.back_handler().back());
    }

    #[test]
    fn back_priority_explicit_first_then_lifo_ties() {
        use crate::context::DEFAULT_BACK_PRIORITY;
        assert_eq!(DEFAULT_BACK_PRIORITY, 0);
        let parent = TestContext::new();
        let order = Rc::new(RefCell::new(Vec::new()));
        let low = Rc::clone(&order);
        let _a = parent.child_context_with_priority("a", 5).unwrap();
        // Winner selection is global across sibling bridges: register the
        // actual callbacks on the child dispatchers.
        let parent2 = TestContext::new();
        let c1 = parent2.child_context("c1").unwrap();
        let c2 = parent2.child_context("c2").unwrap();
        c1.back_handler()
            .register(0, true, move |_, _| low.borrow_mut().push("c1"))
            .unwrap();
        let high = Rc::clone(&order);
        c2.back_handler()
            .register(0, true, move |_, _| high.borrow_mut().push("c2"))
            .unwrap();
        // Same priority -> LIFO: the later bridge wins.
        assert!(parent2.back_handler().back());
        assert_eq!(*order.borrow(), vec!["c2"]);

        // Explicit priority beats LIFO.
        let parent3 = TestContext::new();
        let p1 = parent3.child_context("p1").unwrap();
        let p2 = parent3.child_context_with_priority("p2", 10).unwrap();
        let order = Rc::new(RefCell::new(Vec::new()));
        let o1 = Rc::clone(&order);
        p1.back_handler()
            .register(0, true, move |_, _| o1.borrow_mut().push("p1"))
            .unwrap();
        let o2 = Rc::clone(&order);
        p2.back_handler()
            .register(0, true, move |_, _| o2.borrow_mut().push("p2"))
            .unwrap();
        assert!(parent3.back_handler().back());
        assert_eq!(*order.borrow(), vec!["p2"]);
        let _ = _a;
    }

    // -- Predictive forwarding ---------------------------------------------

    #[test]
    fn predictive_forwards_start_progress_cancel_invoke_exactly_once() {
        let parent = TestContext::new();
        let manual = manual();
        manual.create().unwrap();
        manual.start().unwrap();
        manual.resume().unwrap();
        let child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        let log = new_phase_log();
        child
            .back_handler()
            .register(0, true, record(Rc::clone(&log)))
            .unwrap();

        assert!(parent.back_handler().predictive_start());
        assert_eq!(*log.borrow(), vec![BackPhase::Started]);
        assert!(parent.back_handler().predictive_progress(0.4).unwrap());
        assert_eq!(
            *log.borrow(),
            vec![BackPhase::Started, BackPhase::Progressed]
        );
        assert!(parent.back_handler().predictive_cancel().unwrap());
        assert_eq!(
            *log.borrow(),
            vec![
                BackPhase::Started,
                BackPhase::Progressed,
                BackPhase::Cancelled
            ]
        );

        assert!(parent.back_handler().predictive_start());
        assert!(parent.back_handler().predictive_invoke().unwrap());
        assert_eq!(
            *log.borrow(),
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
    fn predictive_without_gesture_is_coherent_and_removal_safe() {
        let parent = TestContext::new();
        let child = parent.child_context("c").unwrap();
        let log = new_phase_log();
        let handle = child
            .back_handler()
            .register(0, true, record(Rc::clone(&log)))
            .unwrap();
        // No gesture claimed through the bridge: coherent errors, no panic.
        assert_eq!(
            parent.back_handler().predictive_progress(0.1).unwrap_err(),
            essenty_back_handler::BackError::NoGestureInProgress
        );
        assert_eq!(
            parent.back_handler().predictive_cancel().unwrap_err(),
            essenty_back_handler::BackError::NoGestureInProgress
        );
        assert_eq!(
            parent.back_handler().predictive_invoke().unwrap_err(),
            essenty_back_handler::BackError::NoGestureInProgress
        );
        // Removal-cancels-owner follows Essenty semantics without panics:
        // the owner receives cancellation, and the parent-level progress
        // still reports delivery to the bridge (which found no child owner).
        assert!(parent.back_handler().predictive_start());
        child.back_handler().unregister(handle.id());
        assert!(parent.back_handler().predictive_progress(0.5).unwrap());
        assert_eq!(
            *log.borrow(),
            vec![BackPhase::Started, BackPhase::Cancelled]
        );
    }

    #[test]
    fn mid_gesture_priority_change_does_not_steal_bridge_ownership() {
        let parent = TestContext::new();
        let child = parent.child_context("c").unwrap();
        let log = Rc::new(RefCell::new(Vec::new()));
        let low = Rc::clone(&log);
        child
            .back_handler()
            .register(0, true, move |event, _| {
                low.borrow_mut().push(("low", event.phase))
            })
            .unwrap();
        let high = Rc::clone(&log);
        let high_handle = child
            .back_handler()
            .register(10, false, move |event, _| {
                high.borrow_mut().push(("high", event.phase))
            })
            .unwrap();
        assert!(parent.back_handler().predictive_start());
        // Enabling the higher-priority handler mid-gesture does not steal
        // the claimed owner.
        child.back_handler().set_enabled(high_handle.id(), true);
        assert!(parent.back_handler().predictive_progress(0.5).unwrap());
        assert!(parent.back_handler().predictive_cancel().unwrap());
        assert!(log.borrow().iter().all(|(tag, _)| *tag == "low"));
    }

    // -- R-03 at the bridge --------------------------------------------------

    #[test]
    fn r03_bridge_uses_evented_bool_for_gate() {
        let parent = TestContext::new();
        let manual = manual();
        manual.create().unwrap();
        manual.start().unwrap();
        manual.resume().unwrap();
        let child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        let log = new_phase_log();
        let handle = child
            .back_handler()
            .register(0, false, record(Rc::clone(&log)))
            .unwrap();
        assert!(!parent.back_handler().back());
        // Child aggregate false -> true via the evented listener: bridge opens.
        child.back_handler().set_enabled(handle.id(), true);
        assert!(parent.back_handler().back());
        assert_eq!(*log.borrow(), vec![BackPhase::Invoked]);
        // ...and back to false: bridge closes.
        child.back_handler().set_enabled(handle.id(), false);
        assert!(!parent.back_handler().back());
    }

    #[test]
    fn r03_can_handle_in_listener_pattern_is_stale_negative() {
        let parent = TestContext::new();
        let manual = manual();
        manual.create().unwrap();
        manual.start().unwrap();
        manual.resume().unwrap();
        let child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        let child_back = child.back_handler();

        // Records (evented aggregate, can_handle() read inside the listener).
        let probe = Rc::new(RefCell::new(Vec::new()));
        let probe_peer = Rc::clone(&probe);
        let back_peer = child_back.clone();
        child_back.add_enabled_changed_listener(move |evented| {
            probe_peer
                .borrow_mut()
                .push((evented, back_peer.can_handle()));
        });

        // Two enabled callbacks; the winner disables both mid-dispatch.
        let first = child_back.register(0, true, |_, _| {}).unwrap();
        let second_id = Rc::new(Cell::new(u64::MAX));
        let second_probe = Rc::clone(&second_id);
        let first_id = first.id();
        let peer = child_back.clone();
        let second = child_back
            .register(0, true, move |_, _| {
                peer.set_enabled(first_id, false);
                peer.set_enabled(second_probe.get(), false);
            })
            .unwrap();
        second_id.set(second.id());

        // LIFO: the second (later) callback wins and disables everything.
        assert!(parent.back_handler().back());
        // Two aggregate transitions, both observed with evented values. The
        // first tuple's stale read is registration-time: `register` delivers
        // its notification while the busy window is held, so the cache still
        // shows the pre-registration aggregate. The second tuple is the
        // drain-path staleness this test targets: evented false while
        // can_handle() still reads the cached pre-drain true — the old
        // `gate && child.can_handle()` pattern would compute true here.
        assert_eq!(*probe.borrow(), vec![(true, false), (false, true)]);
        // The real bridge used the evented value: routing is closed.
        assert!(!parent.back_handler().back());
    }

    // -- Observer-driven creation (F-12) -------------------------------------

    #[test]
    fn lifecycle_observer_driven_creation_succeeds() {
        let parent = TestContext::initialized();
        let created = Rc::new(RefCell::new(None));
        let probe = Rc::clone(&created);
        let parent_peer = parent.clone();
        let _sub = parent
            .lifecycle()
            .subscribe(move |state| {
                if state == LifecycleState::Created {
                    *probe.borrow_mut() = Some(parent_peer.child_context("lazy").unwrap());
                }
            })
            .detach();
        parent.lifecycle().create().unwrap();
        let child = created.borrow_mut().take().unwrap();
        // Usable without borrow panics: shares the now-Created lifecycle and
        // restores/saves through the parent.
        assert_eq!(child.lifecycle().state(), LifecycleState::Created);
        child
            .state_keeper()
            .borrow_mut()
            .register("v", || b"x".to_vec())
            .unwrap();
        assert!(parent.save_bytes().contains_key("lazy"));
    }

    #[test]
    fn back_callback_driven_creation_rejected_with_full_rollback() {
        let parent = TestContext::new();
        let outcome = Rc::new(RefCell::new(None));
        let probe = Rc::clone(&outcome);
        let parent_peer = parent.clone();
        parent
            .back_handler()
            .register(0, true, move |_, _| {
                *probe.borrow_mut() = Some(parent_peer.child_context("x").unwrap_err());
            })
            .unwrap();
        assert!(parent.back_handler().back());
        assert_eq!(
            *outcome.borrow(),
            Some(DecomposeError::BackDispatchInProgress)
        );
        // No partial child scope survived the rejection.
        assert!(!parent.state_keeper().borrow().is_registered("x"));
        assert!(!parent.instance_keeper().borrow().contains("x"));
        assert_eq!(parent.back_handler().handler_count(), 1);
        assert_eq!(parent.lifecycle().subscriber_count(), 0);
    }

    // -- Survival, ownership, rollback ---------------------------------------

    #[test]
    fn scope_survives_dropped_views_and_parent_still_saves() {
        let parent = TestContext::new();
        let child = parent.child_context("c").unwrap();
        let child_lifecycle = child.lifecycle();
        child
            .state_keeper()
            .borrow_mut()
            .register("v", || b"kept".to_vec())
            .unwrap();
        drop(child);
        // Lifetime is parent-held, not view-held: provider, bridge, and
        // instance retention are all live with zero external child handles.
        assert!(parent.state_keeper().borrow().is_registered("c"));
        assert_eq!(parent.back_handler().handler_count(), 1);
        assert!(parent.instance_keeper().borrow().contains("c"));
        let saved = parent.save_bytes();
        let frame = crate::framing::decode(&saved["c"]).unwrap();
        assert_eq!(frame["v"], b"kept");
        // ...and round-trips through restore.
        let parent2 = parent.recreate(false, saved);
        let child2 = parent2.child_context("c").unwrap();
        assert_eq!(
            child2.state_keeper().borrow_mut().consume_bytes("v"),
            Some(b"kept".to_vec())
        );
        let _ = child_lifecycle;
    }

    #[test]
    fn merged_subscription_survives_dropped_views() {
        let parent = TestContext::initialized();
        let manual = manual();
        let child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        let child_lifecycle = child.lifecycle();
        // Exactly one parent-side merged observer while attached.
        assert_eq!(parent.lifecycle().subscriber_count(), 1);
        drop(child);
        assert_eq!(parent.lifecycle().subscriber_count(), 1);
        // Advancing the parent still advances the merged child.
        parent.lifecycle().create().unwrap();
        parent.lifecycle().start().unwrap();
        parent.lifecycle().resume().unwrap();
        manual.create().unwrap();
        manual.start().unwrap();
        manual.resume().unwrap();
        assert_eq!(child_lifecycle.state(), LifecycleState::Resumed);
    }

    struct PanicContext {
        delegate: TestContext,
    }

    impl ComponentContext for PanicContext {
        fn lifecycle(&self) -> LifecycleRegistry {
            self.delegate.lifecycle()
        }
        fn state_keeper(&self) -> SharedKeeper {
            self.delegate.state_keeper()
        }
        fn instance_keeper(&self) -> SharedInstances {
            self.delegate.instance_keeper()
        }
        fn back_handler(&self) -> SharedBack {
            self.delegate.back_handler()
        }
        fn context_factory(&self) -> ContextFactory<Self> {
            Rc::new(|_| panic!("factory boom"))
        }
    }

    #[test]
    fn factory_panic_rolls_back_attempt_only() {
        let parent = PanicContext {
            delegate: TestContext::new(),
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            parent.child_context("k").unwrap();
        }));
        assert!(result.is_err());
        // Only attempt-created resources were removed.
        assert!(!parent.state_keeper().borrow().is_registered("k"));
        assert!(!parent.instance_keeper().borrow().contains("k"));
        assert_eq!(parent.back_handler().handler_count(), 0);
        assert_eq!(parent.lifecycle().subscriber_count(), 0);
    }

    #[test]
    fn rollback_never_destroys_preexisting_retained_scope() {
        let parent = PanicContext {
            delegate: TestContext::new(),
        };
        // Simulate a retained scope from an unrestored recreation generation.
        let retained: SharedInstances = parent
            .instance_keeper()
            .borrow_mut()
            .get_or_create("k", || RefCell::new(InstanceKeeper::new()))
            .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            parent.child_context("k").unwrap();
        }));
        assert!(result.is_err());
        assert!(parent.instance_keeper().borrow().contains("k"));
        let current: SharedInstances = parent
            .instance_keeper()
            .borrow_mut()
            .get_or_create("k", || RefCell::new(InstanceKeeper::new()))
            .unwrap();
        assert!(Rc::ptr_eq(&current, &retained));
        assert!(!parent.state_keeper().borrow().is_registered("k"));
        assert_eq!(parent.back_handler().handler_count(), 0);
    }

    #[test]
    fn full_teardown_restores_baseline_counts() {
        let parent = TestContext::new();
        let manual = manual();
        let child = parent.child_context_with_lifecycle("m", &manual).unwrap();
        child.back_handler().register(0, true, |_, _| {}).unwrap();
        let (subs, handlers, providers) = (
            parent.lifecycle().subscriber_count(),
            parent.back_handler().handler_count(),
            parent.state_keeper().borrow().provider_count(),
        );
        assert_eq!((subs, handlers, providers), (1, 1, 1));
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            manual.destroy().unwrap();
        }));
        assert_eq!(parent.lifecycle().subscriber_count(), 0);
        assert_eq!(parent.back_handler().handler_count(), 0);
        assert_eq!(parent.state_keeper().borrow().provider_count(), 0);
        assert!(!parent.instance_keeper().borrow().contains("m"));
        assert_eq!(child.lifecycle().state(), LifecycleState::Destroyed);
    }

    #[test]
    fn bridge_lifetime_strong_child_weak_parent_releases() {
        let parent = TestContext::new();
        let child = parent.child_context("c").unwrap();
        let child_back = child.back_handler();
        let parent_back = parent.back_handler();
        // Bridge holds the child strongly; the child's listener holds the
        // parent weakly. Registration adapter peers are weak (upgraded per
        // call), so they contribute no strong counts: 3 child refs (view +
        // local + bridge child link), 2 parent refs (view + local).
        assert_eq!(child_back.strong_count(), 3);
        assert_eq!(parent_back.strong_count(), 2);
        drop(child);
        drop(parent);
        assert_eq!(child_back.strong_count(), 2);
        assert_eq!(parent_back.strong_count(), 1);
        // Releasing the last parent handle drops the dispatcher, which
        // releases the bridge callback and its strong child link.
        drop(parent_back);
        assert_eq!(child_back.strong_count(), 1);
        // The weak parent link now dangles safely: aggregate changes are
        // no-ops, never panics.
        let handle = child_back.register(0, true, |_, _| {}).unwrap();
        child_back.set_enabled(handle.id(), false);
    }

    #[test]
    fn default_context_child_constructors_smoke() {
        // The public DefaultComponentContext path exercises the same engine.
        let root = DefaultComponentContext::new({
            let lifecycle = LifecycleRegistry::new();
            lifecycle.create().unwrap();
            lifecycle
        });
        let child = root.child_context("c").unwrap();
        assert_eq!(child.state(), LifecycleState::Created);
        let manual = manual();
        let modal = root.child_context_with_lifecycle("modal", &manual).unwrap();
        assert_eq!(modal.state(), LifecycleState::Initialized);
        manual.create().unwrap();
        assert_eq!(modal.state(), LifecycleState::Created);
    }
}
