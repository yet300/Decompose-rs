use super::navigation::{Event, NavigationResult};
type ChildFactory<Ctx, C, T> = Rc<dyn Fn(&C, Ctx) -> T>;
type SavedChildren = Vec<(String, BTreeMap<String, Vec<u8>>)>;
type SnapshotChildren = Vec<(String, crate::StateSnapshot)>;
use super::{Child, NavigationError};
use crate::settings::{DiagnosticOperation, report_navigation_error};
struct QueuedEvent<S> {
    event: Event<S>,
    operation: DiagnosticOperation,
    diagnose: bool,
}
fn release_events<S>(events: VecDeque<QueuedEvent<S>>) {
    let mut cleanup = crate::cleanup::Cleanup::new();
    for queued in events {
        let Event {
            transformer,
            completion,
        } = queued.event;
        cleanup.run(|| completion(Err(NavigationError::ChildUnavailable)));
        cleanup.run(|| drop(transformer));
        cleanup.run(|| drop(completion));
    }
    cleanup.finish();
}

pub(crate) struct SavedState<S> {
    pub(crate) configurations: S,
    pub(crate) children: SavedChildren,
    pub(crate) next_key: u64,
}
struct MemoryNavigation<S> {
    kind: &'static str,
    configurations: S,
    children: SnapshotChildren,
    next_key: u64,
}
pub(crate) trait Model<C, T>: 'static {
    type State: Clone + 'static;
    type Snapshot: 'static;
    fn persistent(&self) -> bool;
    fn memory_kind(&self) -> &'static str {
        "generic"
    }
    fn configurations(&self, state: &Self::State) -> Vec<Rc<C>>;
    fn on_state_changed(&self, _state: &Self::State, _old: Option<&Self::State>) {}
    // Opt-in models refresh future factory input without recreating live instances.
    fn canonicalize_reused_configurations(&self) -> bool {
        false
    }
    fn normalize(&self, state: Self::State) -> Self::State {
        state
    }
    fn validate(&self, state: &Self::State) -> Result<(), NavigationError>;
    fn validate_transition(
        &self,
        state: &Self::State,
        _old: Option<&Self::State>,
    ) -> Result<(), NavigationError> {
        self.validate(state)
    }
    fn unique_configurations(&self) -> bool {
        false
    }
    fn configuration_fingerprint(&self, _configuration: &C) -> Option<u64> {
        None
    }
    fn should_restore(&self, _state: &Self::State) -> bool {
        true
    }
    fn statuses(&self, state: &Self::State) -> Vec<ChildStatus>;
    fn snapshot(&self, children: Vec<ChildItem<C, T>>, state: &Self::State) -> Self::Snapshot;
    fn back_enabled(&self, state: &Self::State) -> bool;
    fn back_transform(&self, state: Self::State) -> Self::State;
    fn on_back_error(&self, _error: NavigationError) {}
    fn decode(&self, bytes: &[u8]) -> Result<Option<SavedState<Self::State>>, NavigationError>;
    fn encode_with_children(
        &self,
        state: &Self::State,
        next_key: u64,
        children: impl FnOnce() -> Result<SavedChildren, NavigationError>,
    ) -> Result<Option<Vec<u8>>, NavigationError> {
        self.encode(state, &children()?, next_key)
    }
    fn encode(
        &self,
        state: &Self::State,
        children: &SavedChildren,
        next_key: u64,
    ) -> Result<Option<Vec<u8>>, NavigationError>;
}
use super::children::{ChildItem, ChildStatus};
use crate::retention::RetainedScope;
use crate::value::{MutableValue, Subscription, Value};
use crate::{
    ComponentContext, ContextFactory, ContextParts, DecomposeError, InstanceKeeperError,
    LifecycleRegistry, LifecycleState, MergedLifecycle, SharedBack, SharedInstances, SharedKeeper,
    StateKeeper,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    rc::{Rc, Weak},
};

#[derive(Default)]
struct RetainedNavigation {
    scopes: RefCell<BTreeMap<String, Rc<RetainedScope>>>,
}
impl RetainedNavigation {
    fn remove(&self, key: &str) {
        let removed = self.scopes.borrow_mut().remove(key);
        drop(removed);
    }
    fn retain(&self, mut keep: impl FnMut(&str) -> bool) {
        let removed = {
            let mut scopes = self.scopes.borrow_mut();
            let keys: Vec<_> = scopes.keys().filter(|key| !keep(key)).cloned().collect();
            keys.into_iter()
                .filter_map(|key| scopes.remove(&key))
                .collect::<Vec<_>>()
        };
        // InstanceKeeper destruction may invoke user callbacks; release the map first.
        let mut cleanup = crate::cleanup::Cleanup::new();
        for scope in removed {
            cleanup.run(|| drop(scope));
        }
        cleanup.finish();
    }
}

impl Drop for RetainedNavigation {
    fn drop(&mut self) {
        release_scopes(std::mem::take(self.scopes.get_mut()));
    }
}
fn release_scopes(scopes: BTreeMap<String, Rc<RetainedScope>>) {
    let mut cleanup = crate::cleanup::Cleanup::new();
    for (_, scope) in scopes {
        cleanup.run(|| drop(scope));
    }
    cleanup.finish();
}

struct ScopeParts {
    gate: Rc<Cell<bool>>,
    local: LifecycleRegistry,
    merged: MergedLifecycle,
    state: SharedKeeper,
    instances: SharedInstances,
    back: SharedBack,
    parent_back: SharedBack,
    handle: u64,
    listener: u64,
    lifecycle_listener: u64,
}
impl ScopeParts {
    fn end(&self, end_retention: bool) {
        self.gate.set(false);
        let mut cleanup = crate::cleanup::Cleanup::new();
        cleanup.run(|| {
            self.parent_back.unregister(self.handle);
        });
        cleanup.run(|| {
            self.back.remove_enabled_changed_listener(self.listener);
        });
        cleanup.run(|| {
            let _ = self.merged.lifecycle().unsubscribe(self.lifecycle_listener);
        });
        // Essenty commits a lifecycle step before observer delivery. If pause
        // or stop delivery panics, resume terminal destruction from that step
        // so cleanup cannot leave a partly ended scope behind.
        while !self.local.state().is_destroyed() {
            cleanup.run(|| {
                let _ = self.local.destroy();
            });
        }
        while !self.merged.state().is_destroyed() {
            cleanup
                .run(|| crate::merged::drive(&self.merged.lifecycle(), LifecycleState::Destroyed));
        }
        cleanup.run(|| self.merged.shutdown());
        if end_retention {
            cleanup.run(|| crate::InstanceStore::destroy_shared(&self.instances));
        }
        cleanup.finish();
    }
    fn activate(&self, status: ChildStatus) {
        let active = matches!(status, ChildStatus::Started | ChildStatus::Resumed);
        self.gate.set(active);
        let effective = matches!(
            self.merged.state(),
            LifecycleState::Started | LifecycleState::Resumed
        );
        self.parent_back
            .set_enabled(self.handle, active && effective && self.back.can_handle());
        let target = match status {
            ChildStatus::Created => LifecycleState::Created,
            ChildStatus::Started => LifecycleState::Started,
            ChildStatus::Resumed => LifecycleState::Resumed,
            ChildStatus::Destroyed => LifecycleState::Destroyed,
        };
        let _ = self.local.move_to(target);
    }
}
impl Drop for ScopeParts {
    fn drop(&mut self) {
        self.end(false);
    }
}
struct Scope<C, T> {
    child: Option<Child<C, T>>,
    parts: ScopeParts,
}

impl<C, T> Scope<C, T> {
    fn child(&self) -> &Child<C, T> {
        self.child.as_ref().expect("live child scope")
    }
}
impl<C, T> Drop for Scope<C, T> {
    fn drop(&mut self) {
        let mut cleanup = crate::cleanup::Cleanup::new();
        cleanup.run(|| self.parts.end(false));
        let child = self.child.take();
        cleanup.run(|| drop(child));
        cleanup.finish();
    }
}

struct Managed<C, T> {
    configuration: Rc<C>,
    key: String,
    scope: Option<Rc<Scope<C, T>>>,
    saved: crate::StateSnapshot,
}
impl<C, T> Clone for Managed<C, T> {
    fn clone(&self) -> Self {
        Self {
            configuration: self.configuration.clone(),
            key: self.key.clone(),
            scope: self.scope.clone(),
            saved: self.saved.clone(),
        }
    }
}
impl<C, T> Managed<C, T> {
    fn child(&self) -> ChildItem<C, T> {
        self.scope.as_ref().map_or_else(
            || ChildItem::Destroyed {
                configuration: self.configuration.clone(),
                key: self.key.clone(),
            },
            |s| ChildItem::Created(s.child().clone()),
        )
    }
    fn reusable(&self) -> bool {
        self.scope.as_ref().is_none_or(|s| {
            !s.parts.local.state().is_destroyed() && !s.parts.instances.borrow().is_destroyed()
        })
    }
    fn save(&self) -> Result<BTreeMap<String, Vec<u8>>, NavigationError> {
        match &self.scope {
            Some(scope) => scope
                .parts
                .state
                .try_borrow()
                .map_err(|_| {
                    NavigationError::state_message("child state keeper is borrowed".into())
                })?
                .save()
                .map_err(NavigationError::state_source),
            None => Ok(self.saved.persistent().clone()),
        }
    }
    fn save_snapshot(&self) -> Result<crate::StateSnapshot, NavigationError> {
        match &self.scope {
            Some(scope) => scope
                .parts
                .state
                .try_borrow()
                .map_err(|_| {
                    NavigationError::state_message("child state keeper is borrowed".into())
                })?
                .save_snapshot()
                .map_err(NavigationError::state_source),
            None => Ok(self.saved.clone()),
        }
    }
}

struct Prepared<C, T> {
    scopes: Vec<(Rc<Scope<C, T>>, bool)>,
    retained: Rc<RetainedNavigation>,
    committed: bool,
}
impl<C, T> Drop for Prepared<C, T> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut cleanup = crate::cleanup::Cleanup::new();
        for (scope, fresh_retention) in self.scopes.iter().rev() {
            cleanup.run(|| scope.parts.end(*fresh_retention));
            if *fresh_retention {
                cleanup.run(|| self.retained.remove(&scope.child().key));
            }
        }
        cleanup.finish();
    }
}

struct Construction {
    state: SharedKeeper,
    instances: SharedInstances,
    key: String,
    restored: Option<Vec<u8>>,
    restored_memory: Option<crate::state::MemoryValue>,
    new_retention: bool,
    retained: Option<Rc<RetainedNavigation>>,
    previous_scopes: BTreeMap<String, Rc<RetainedScope>>,
    cleanup: Option<Box<dyn FnOnce()>>,
    registered: bool,
    committed: bool,
}
impl Drop for Construction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut rollback = crate::cleanup::Cleanup::new();
        rollback.run(|| {
            if let Some(cleanup) = self.cleanup.take() {
                cleanup();
            }
        });
        rollback.run(|| {
            if let Some(value) = self.restored_memory.take() {
                self.state
                    .borrow_mut()
                    .return_memory(self.key.clone(), value);
            }
        });
        rollback.run(|| {
            if self.registered {
                let removed = self.state.borrow_mut().take_registration(&self.key);
                drop(removed);
            }
        });
        rollback.run(|| {
            if let Some(retained) = &self.retained {
                let replaced = retained
                    .scopes
                    .replace(std::mem::take(&mut self.previous_scopes));
                release_scopes(replaced);
            }
        });
        rollback.run(|| {
            if self.new_retention {
                crate::InstanceStore::release_key_shared(&self.instances, &self.key);
            }
        });
        rollback.run(|| {
            if let Some(bytes) = self.restored.take() {
                self.state
                    .borrow_mut()
                    .return_bytes(self.key.clone(), bytes);
            }
        });
        rollback.finish();
    }
}

struct Engine<Ctx, C, T, M: Model<C, T>> {
    parent: LifecycleRegistry,
    config: Option<crate::DecomposeConfig>,
    parent_state: Weak<RefCell<StateKeeper>>,
    parent_instances: Weak<RefCell<crate::instances::InstanceScope>>,
    back: SharedBack,
    retained: Rc<RetainedNavigation>,
    context_factory: ContextFactory<Ctx>,
    child_factory: ChildFactory<Ctx, C, T>,
    key: String,
    model: M,
    memory_navigation: bool,
    next_key: Cell<u64>,
    items: RefCell<Vec<Managed<C, T>>>,
    navigation_state: RefCell<Rc<M::State>>,
    state: RefCell<Option<MutableValue<M::Snapshot>>>,
    queue: RefCell<VecDeque<QueuedEvent<M::State>>>,
    processing: Cell<bool>,
    initializing: Cell<bool>,
    destroyed: Cell<bool>,
    deferred: Cell<bool>,
    source_subscription: RefCell<Option<Subscription>>,
    destroy_subscription: Cell<Option<u64>>,
    pop_handle: Cell<Option<u64>>,
}

impl<Ctx: ComponentContext + 'static, C: Eq + 'static, T: 'static, M: Model<C, T>>
    Engine<Ctx, C, T, M>
{
    // Only clone the shared handle while borrowed. User State::clone and Drop
    // may navigate, save state or end the parent, so both run outside this cell.
    fn navigation_snapshot(&self) -> Rc<M::State> {
        self.navigation_state.borrow().clone()
    }
    fn clone_navigation_state(&self) -> M::State {
        self.navigation_snapshot().as_ref().clone()
    }
    fn ensure_alive(&self) -> Result<(), NavigationError> {
        if self.destroyed.get() || self.parent.state().is_destroyed() {
            Err(NavigationError::Destroyed)
        } else {
            Ok(())
        }
    }
    fn ensure_child_available(&self, key: &str) -> Result<(), NavigationError> {
        self.ensure_alive()?;
        let scope = self.items.try_borrow().ok().and_then(|items| {
            items
                .iter()
                .find(|item| item.key == key)
                .and_then(|item| item.scope.clone())
        });
        if scope.is_some_and(|scope| {
            !scope.parts.local.state().is_destroyed()
                && !scope.parts.merged.state().is_destroyed()
                && scope
                    .parts
                    .instances
                    .try_borrow()
                    .is_ok_and(|instances| !instances.is_destroyed())
        }) {
            Ok(())
        } else {
            Err(NavigationError::ChildUnavailable)
        }
    }
    fn create(
        &self,
        config: Rc<C>,
        key: String,
        saved: crate::StateSnapshot,
    ) -> Result<Rc<Scope<C, T>>, NavigationError> {
        let retained = self
            .retained
            .scopes
            .borrow()
            .get(&key)
            .filter(|scope| !scope.instances.borrow().is_destroyed())
            .cloned();
        let owner = retained.unwrap_or_else(|| Rc::new(RetainedScope::new()));
        let local = LifecycleRegistry::new();
        let merged = MergedLifecycle::new(&self.parent, &local);
        let state = Rc::new(RefCell::new(StateKeeper::from_snapshot(saved)));
        let back = SharedBack::new();
        let child_back = back.clone();
        let handle = self
            .back
            .register(1, false, move |event, _| {
                use crate::BackPhase;
                match event.phase {
                    BackPhase::Started => {
                        if let Some(pos) = event.position {
                            child_back.predictive_start_with(pos);
                        } else {
                            child_back.predictive_start();
                        }
                    }
                    BackPhase::Progressed => {
                        if let Some(pos) = event.position {
                            let _ = child_back
                                .predictive_progress_with(event.progress.unwrap_or(0.0), pos);
                        } else {
                            let _ = child_back.predictive_progress(event.progress.unwrap_or(0.0));
                        }
                    }
                    BackPhase::Cancelled => {
                        let _ = child_back.predictive_cancel();
                    }
                    BackPhase::Invoked => {
                        child_back.back();
                    }
                }
            })
            .map_err(DecomposeError::from)?;
        let id = handle.id();
        let parent = self.back.downgrade();
        let gate = Rc::new(Cell::new(false));
        let effective = Rc::new(Cell::new(false));
        let listener_gate = gate.clone();
        let listener_effective = effective.clone();
        let listener = back.add_enabled_changed_listener(move |enabled| {
            if let Some(parent) = parent.upgrade() {
                parent.set_enabled(
                    id,
                    listener_gate.get() && listener_effective.get() && enabled,
                );
            }
        });
        let lifecycle_gate = gate.clone();
        let lifecycle_back = back.downgrade();
        let lifecycle_parent = self.back.downgrade();
        let lifecycle_listener = merged
            .lifecycle()
            .subscribe(move |state| {
                let active = matches!(state, LifecycleState::Started | LifecycleState::Resumed);
                effective.set(active);
                if let (Some(back), Some(parent)) =
                    (lifecycle_back.upgrade(), lifecycle_parent.upgrade())
                {
                    parent.set_enabled(id, lifecycle_gate.get() && active && back.can_handle());
                }
            })
            .detach();
        let parts = ScopeParts {
            gate,
            local,
            merged,
            state,
            instances: owner.instances.clone(),
            back,
            parent_back: self.back.clone(),
            handle: id,
            listener,
            lifecycle_listener,
        };
        let context = (self.context_factory)(ContextParts::new(
            parts.merged.lifecycle(),
            parts.state.clone(),
            parts.instances.clone(),
            parts.back.clone(),
        ));
        if self.destroyed.get() || self.parent.state().is_destroyed() {
            return Err(NavigationError::Destroyed);
        }
        let instance = Rc::new((self.child_factory)(&config, context));
        if self.destroyed.get() || self.parent.state().is_destroyed() {
            return Err(NavigationError::Destroyed);
        }
        let scope = Rc::new(Scope {
            child: Some(Child {
                configuration: config,
                instance,
                key: key.clone(),
            }),
            parts,
        });
        let replaced = self.retained.scopes.borrow_mut().insert(key, owner);
        drop(replaced);
        Ok(scope)
    }
    fn switch(
        &self,
        state: M::State,
        restored: Option<SnapshotChildren>,
    ) -> Result<(), NavigationError> {
        let state = self.model.normalize(state);
        let previous = self.navigation_snapshot();
        self.model.validate_transition(
            &state,
            (!self.initializing.get()).then_some(previous.as_ref()),
        )?;
        self.ensure_alive()?;
        // Policy callbacks precede preparation, so a policy panic owns no fresh scopes.
        let statuses = self.model.statuses(&state);
        self.ensure_alive()?;
        let configs = self.model.configurations(&state);
        let old_state = previous.as_ref().clone();
        // Validation must not retain the displaced state through publication.
        drop(previous);
        let old = self.items.borrow().clone();
        let mut matched = vec![false; old.len()];
        let mut candidates = std::collections::HashMap::<u64, Vec<usize>>::new();
        for (position, item) in old.iter().enumerate() {
            if let Some(fingerprint) = self.model.configuration_fingerprint(&item.configuration) {
                candidates.entry(fingerprint).or_default().push(position);
            }
        }
        let mut next = Vec::with_capacity(configs.len());
        let mut counter = self.next_key.get();
        for (index, config) in configs.iter().enumerate() {
            let is_match =
                |i: usize| !matched[i] && old[i].reusable() && old[i].configuration == *config;
            // Same-position equality avoids scanning already-matched prefixes.
            let existing_index =
                if self.model.unique_configurations() && index < old.len() && is_match(index) {
                    Some(index)
                } else if let Some(fingerprint) = self.model.configuration_fingerprint(config) {
                    candidates
                        .get(&fingerprint)
                        .and_then(|positions| positions.iter().copied().find(|&i| is_match(i)))
                } else {
                    (0..old.len()).find(|&i| is_match(i))
                };
            let existing = existing_index.map(|i| (i, &old[i]));
            let mut item = if let Some((i, item)) = existing {
                matched[i] = true;
                let mut item = item.clone();
                if self.model.canonicalize_reused_configurations() {
                    item.configuration = config.clone();
                }
                item
            } else {
                let (key, saved) = if let Some(restored) = &restored {
                    restored[index].clone()
                } else {
                    let key = format!("child/{counter}");
                    counter = counter.checked_add(1).ok_or_else(|| {
                        NavigationError::state_message("child key counter exhausted".into())
                    })?;
                    (key, crate::StateSnapshot::default())
                };
                Managed {
                    configuration: config.clone(),
                    key,
                    scope: None,
                    saved,
                }
            };
            if statuses[index] == ChildStatus::Destroyed {
                // Save every outgoing live page before any destructive lifecycle delivery.
                item.saved = item.save_snapshot()?;
                item.scope = None;
            }
            next.push(item);
        }
        self.ensure_alive()?;
        let mut prepared = Prepared {
            scopes: Vec::new(),
            retained: self.retained.clone(),
            committed: false,
        };
        for (index, item) in next.iter_mut().enumerate() {
            if statuses[index] != ChildStatus::Destroyed && item.scope.is_none() {
                let fresh_retention = self
                    .retained
                    .scopes
                    .borrow()
                    .get(&item.key)
                    .is_none_or(|s| s.instances.borrow().is_destroyed());
                let scope = self.create(
                    item.configuration.clone(),
                    item.key.clone(),
                    item.saved.clone(),
                )?;
                prepared.scopes.push((scope.clone(), fresh_retention));
                item.scope = Some(scope);
            }
        }
        let live_targets: BTreeMap<_, _> = next
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                item.scope
                    .as_ref()
                    .map(|scope| (Rc::as_ptr(scope), statuses[index]))
            })
            .collect();
        // Stop outgoing or demoted scopes before any incoming scope advances.
        for item in &old {
            if let Some(scope) = &item.scope {
                let target = live_targets
                    .get(&Rc::as_ptr(scope))
                    .copied()
                    .unwrap_or(ChildStatus::Created);
                let rank = |s| match s {
                    ChildStatus::Created => 1,
                    ChildStatus::Started => 2,
                    ChildStatus::Resumed => 3,
                    ChildStatus::Destroyed => 0,
                };
                let current = match scope.parts.local.state() {
                    LifecycleState::Started => 2,
                    LifecycleState::Resumed => 3,
                    _ => 1,
                };
                if rank(target) < current {
                    scope.parts.activate(target);
                }
                self.ensure_alive()?;
            }
        }
        let mut cleanup = crate::cleanup::Cleanup::new();
        for item in old.iter().rev() {
            if let Some(scope) = &item.scope
                && !live_targets.contains_key(&Rc::as_ptr(scope))
            {
                cleanup.run(|| scope.parts.end(true));
                cleanup.run(|| self.retained.remove(&item.key));
            }
        }
        // Retention only belongs to desired live entries, including restoration tombstones.
        let retained_keys: std::collections::BTreeSet<_> = next
            .iter()
            .filter(|item| item.scope.is_some())
            .map(|item| item.key.as_str())
            .collect();
        cleanup.run(|| self.retained.retain(|key| retained_keys.contains(key)));
        cleanup.finish();
        self.ensure_alive()?;
        for (index, item) in next.iter().enumerate() {
            if let Some(scope) = &item.scope {
                scope.parts.activate(statuses[index]);
                self.ensure_alive()?;
            }
        }
        if let Some(handle) = self.pop_handle.get() {
            self.back
                .set_enabled(handle, self.model.back_enabled(&state));
        }
        self.ensure_alive()?;
        let committed_state = Rc::new(state.clone());
        self.ensure_alive()?;
        self.next_key.set(counter);
        *self.items.borrow_mut() = next.clone();
        let previous_state = self.navigation_state.replace(committed_state);
        prepared.committed = true;
        drop(previous_state);
        self.ensure_alive()?;
        let snapshot = self
            .model
            .snapshot(next.iter().map(Managed::child).collect(), &state);
        let value = self.state.borrow().clone();
        if let Some(value) = value {
            value.set(snapshot);
        } else {
            *self.state.borrow_mut() = Some(MutableValue::new(snapshot));
        }
        if !self.initializing.get() {
            self.model.on_state_changed(&state, Some(&old_state));
        }
        Ok(())
    }
    fn enqueue(self: &Rc<Self>, event: Event<M::State>) {
        self.enqueue_kind(event, DiagnosticOperation::Navigation);
    }
    fn enqueue_kind(self: &Rc<Self>, event: Event<M::State>, operation: DiagnosticOperation) {
        if self.destroyed.get() {
            return;
        }
        self.queue.borrow_mut().push_back(QueuedEvent {
            event,
            operation,
            diagnose: !crate::settings::is_reporting(),
        });
        self.pump();
    }
    fn pump(self: &Rc<Self>) {
        if self.initializing.get() || self.processing.get() || self.destroyed.get() {
            return;
        }
        if self.back.is_dispatching() {
            if !self.deferred.replace(true) {
                let weak = Rc::downgrade(self);
                struct Deferred<F: Fn()> {
                    cancel: F,
                    armed: bool,
                }
                impl<F: Fn()> Deferred<F> {
                    fn complete(&mut self) {
                        self.armed = false;
                    }
                }
                impl<F: Fn()> Drop for Deferred<F> {
                    fn drop(&mut self) {
                        if self.armed {
                            let mut cleanup = crate::cleanup::Cleanup::new();
                            cleanup.run(|| (self.cancel)());
                            cleanup.finish();
                        }
                    }
                }
                let cancel_weak = weak.clone();
                let mut work = Deferred {
                    cancel: move || {
                        if let Some(engine) = cancel_weak.upgrade() {
                            engine.deferred.set(false);
                            let abandoned = std::mem::take(&mut *engine.queue.borrow_mut());
                            release_events(abandoned);
                        }
                    },
                    armed: true,
                };
                self.back.after_dispatch(move || {
                    work.complete();
                    if let Some(engine) = weak.upgrade() {
                        engine.deferred.set(false);
                        engine.pump();
                    }
                });
            }
            return;
        }
        self.processing.set(true);
        struct Reset<'a, S>(&'a Cell<bool>, &'a RefCell<VecDeque<QueuedEvent<S>>>);
        impl<C> Drop for Reset<'_, C> {
            fn drop(&mut self) {
                self.0.set(false);
                if std::thread::panicking() {
                    let abandoned = std::mem::take(&mut *self.1.borrow_mut());
                    // Abandoned FIFO work must report failure so pending
                    // receipts observe Err instead of an empty Ok. User
                    // completions may panic; never abort an active unwind.
                    release_events(abandoned);
                }
            }
        }
        let _reset = Reset(&self.processing, &self.queue);
        loop {
            let event = self.queue.borrow_mut().pop_front();
            let Some(QueuedEvent {
                event,
                operation,
                diagnose,
            }) = event
            else {
                break;
            };
            let old_stack = self.clone_navigation_state();
            let configs = (event.transformer)(old_stack.clone());
            let result = self.switch(configs, None).map(|()| NavigationResult {
                old_stack,
                new_stack: self.clone_navigation_state(),
            });
            if diagnose && let Err(error) = &result {
                report_navigation_error(self.config.as_ref(), operation, &self.key, error);
            }
            (event.completion)(result);
            if self.destroyed.get() {
                break;
            }
        }
    }
    fn save(&self) -> Result<Option<Vec<u8>>, NavigationError> {
        if self.memory_navigation || !self.model.persistent() {
            return Ok(None);
        }
        let items = self.items.borrow().clone();
        let state = self.navigation_snapshot();
        let next_key = self.next_key.get();
        let state = state.as_ref().clone();
        self.model.encode_with_children(&state, next_key, || {
            let mut children = Vec::with_capacity(items.len());
            for item in &items {
                children.push((item.key.clone(), item.save()?));
            }
            Ok(children)
        })
    }
    fn save_snapshot(&self) -> Result<crate::state::SavedEntry, NavigationError> {
        let items = self.items.borrow().clone();
        let state = self.clone_navigation_state();
        let next_key = self.next_key.get();
        let save_children = || {
            items
                .iter()
                .map(|item| Ok((item.key.clone(), item.save_snapshot()?)))
                .collect::<Result<Vec<_>, NavigationError>>()
        };
        let mut children = None;
        let persistent = if self.memory_navigation {
            children = Some(save_children()?);
            None
        } else if self.model.persistent() {
            self.model.encode_with_children(&state, next_key, || {
                let snapshots = save_children()?;
                let bytes = snapshots
                    .iter()
                    .map(|(key, snapshot)| (key.clone(), snapshot.persistent().clone()))
                    .collect();
                children = Some(snapshots);
                Ok(bytes)
            })?
        } else {
            None
        };
        let memory = if self.memory_navigation || persistent.is_some() {
            children.map(|children| {
                Rc::new(MemoryNavigation {
                    kind: self.model.memory_kind(),
                    configurations: state,
                    children,
                    next_key,
                }) as crate::state::MemoryValue
            })
        } else {
            None
        };
        Ok(crate::state::SavedEntry { persistent, memory })
    }
    fn teardown(&self) {
        if self.destroyed.replace(true) {
            return;
        }
        let mut cleanup = crate::cleanup::Cleanup::new();
        let abandoned = std::mem::take(&mut *self.queue.borrow_mut());
        let subscription = self.source_subscription.borrow_mut().take();
        cleanup.run(|| {
            if let Some(handle) = self.pop_handle.take() {
                self.back.unregister(handle);
            }
        });
        let items = self.items.borrow().clone();
        for item in items.iter().rev() {
            if let Some(scope) = &item.scope {
                cleanup.run(|| scope.parts.end(false));
            }
        }
        cleanup.run(|| {
            if let Some(state) = self.parent_state.upgrade() {
                let removed = state.borrow_mut().take_registration(&self.key);
                drop(removed);
            }
        });
        cleanup.run(|| {
            if let Some(id) = self.destroy_subscription.take() {
                let _ = self.parent.unsubscribe(id);
            }
        });
        // Essenty stops lifecycle notification after a callback panic. Finish
        // the default root's owned retention even when its earlier attach hook
        // cannot be reached; external keepers continue to belong to their host.
        if self.parent.state().is_destroyed() {
            cleanup.run(|| {
                if let Some(instances) = self.parent_instances.upgrade()
                    && instances.borrow().lifecycle_owned
                {
                    crate::InstanceStore::destroy_shared(&instances);
                }
            });
        }
        // Both the token and abandoned events can own arbitrary user captures.
        let mut subscription = subscription;
        cleanup.run(|| drop(subscription.take()));
        cleanup.run(|| release_events(abandoned));
        cleanup.finish();
    }
}

type ControlledValue<S, V> = (Value<V>, NavigationControl<S>);

type EventReceiver<S> = Rc<dyn Fn(Event<S>)>;

// Before the engine owns the token, initialization may unwind through this
// local. Custom cancellation must not turn that original panic into an abort.
struct SetupSubscription(Option<Subscription>);
impl Drop for SetupSubscription {
    fn drop(&mut self) {
        let mut cleanup = crate::cleanup::Cleanup::new();
        let subscription = self.0.take();
        cleanup.run(|| drop(subscription));
        cleanup.finish();
    }
}

pub(crate) fn build_with_subscriber<Ctx, C, T, M>(
    context: &Ctx,
    subscribe: impl FnOnce(Rc<dyn Fn(Event<M::State>)>) -> Subscription,
    initial: impl FnOnce() -> M::State,
    key: String,
    model: M,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<ControlledValue<M::State, M::Snapshot>, NavigationError>
where
    Ctx: ComponentContext + 'static,
    C: Eq + 'static,
    T: 'static,
    M: Model<C, T>,
{
    let diagnostic_key = key.clone();
    let result = build_with_subscriber_inner(context, subscribe, initial, key, model, factory);
    if let Err(error) = &result {
        report_navigation_error(
            context.configuration().as_ref(),
            DiagnosticOperation::Construction,
            &diagnostic_key,
            error,
        );
    }
    result
}

fn build_with_subscriber_inner<Ctx, C, T, M>(
    context: &Ctx,
    subscribe: impl FnOnce(Rc<dyn Fn(Event<M::State>)>) -> Subscription,
    initial: impl FnOnce() -> M::State,
    key: String,
    model: M,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<ControlledValue<M::State, M::Snapshot>, NavigationError>
where
    Ctx: ComponentContext + 'static,
    C: Eq + 'static,
    T: 'static,
    M: Model<C, T>,
{
    if context.lifecycle().state().is_destroyed() {
        return Err(NavigationError::Destroyed);
    }
    let parent_state = context.state_store().inner;
    if parent_state.borrow().is_registered(&key) {
        return Err(DecomposeError::DuplicateKey { key }.into());
    }
    // Subscribe before running any user initialization callback. Synchronous
    // custom-source replay and lazy-initial emissions share the engine queue.
    let pending = Rc::new(RefCell::new(VecDeque::new()));
    let receiver = Rc::new(RefCell::new(None::<EventReceiver<M::State>>));
    let pending_out = pending.clone();
    let receiver_out = receiver.clone();
    let mut subscription = SetupSubscription(Some(subscribe(Rc::new(move |event| {
        let callback = receiver_out.borrow().clone();
        if let Some(callback) = callback {
            callback(event);
        } else {
            pending_out.borrow_mut().push_back(event);
        }
    }))));
    let restored_bytes = parent_state.borrow_mut().consume_bytes(&key);
    let restored_memory = parent_state.borrow_mut().consume_memory(&key);
    let memory_navigation = parent_state.borrow().memory_navigation(&key);
    let instances = context.instance_store().inner;
    let mut construction = Construction {
        state: parent_state.clone(),
        instances: instances.clone(),
        key: key.clone(),
        restored: restored_bytes.clone(),
        restored_memory: restored_memory.clone(),
        new_retention: false,
        retained: None,
        previous_scopes: BTreeMap::new(),
        cleanup: None,
        registered: false,
        committed: false,
    };
    let fallback = restored_bytes.clone();
    parent_state
        .borrow_mut()
        .register_optional(&key, move || fallback.clone())
        .map_err(DecomposeError::from)?;
    construction.registered = true;
    let fallback_bytes = restored_bytes.clone();
    let fallback_memory = restored_memory.clone();
    parent_state
        .borrow_mut()
        .add_snapshot_provider(&key, move || {
            Ok(crate::state::SavedEntry {
                persistent: fallback_bytes.clone(),
                memory: fallback_memory.clone(),
            })
        });
    let memory = restored_memory
        .map(|value| {
            value.downcast::<MemoryNavigation<M::State>>().map_err(|_| {
                NavigationError::state_message("memory navigation state type mismatch".into())
            })
        })
        .transpose()?;
    if memory
        .as_ref()
        .is_some_and(|saved| saved.kind != model.memory_kind())
    {
        return Err(NavigationError::state_message(
            "memory navigation model mismatch".into(),
        ));
    }
    let mut restored = if memory_navigation {
        memory.as_ref().map(|saved| SavedState {
            configurations: saved.configurations.clone(),
            children: saved
                .children
                .iter()
                .map(|(key, snapshot)| (key.clone(), snapshot.persistent().clone()))
                .collect(),
            next_key: saved.next_key,
        })
    } else {
        match restored_bytes.as_ref() {
            Some(bytes) => model.decode(bytes)?,
            None => None,
        }
    };
    if let Some(saved) = &restored {
        model.validate(&saved.configurations)?;
        if model.configurations(&saved.configurations).len() != saved.children.len() {
            return Err(NavigationError::state_message(
                "saved child count does not match configurations".into(),
            ));
        }
        let mut keys = std::collections::BTreeSet::new();
        for (key, _) in &saved.children {
            let counter = key
                .strip_prefix("child/")
                .and_then(|suffix| suffix.parse::<u64>().ok());
            if !counter.is_some_and(|counter| {
                counter < saved.next_key && key == &format!("child/{counter}")
            }) || !keys.insert(key)
            {
                return Err(NavigationError::state_message(
                    "invalid child identity or next-key counter".into(),
                ));
            }
        }
        // A byte codec's predicate applies only to the byte saver, not to the
        // explicitly selected codec-free in-memory saver.
        if !memory_navigation && !model.should_restore(&saved.configurations) {
            restored = None;
        }
    }
    let initial = restored
        .as_ref()
        .map(|state| state.configurations.clone())
        .unwrap_or_else(initial);
    // Initialization (or dropping a skipped initializer) may destroy the parent.
    // Report that before accessing its now-destroyed instance keeper.
    if context.lifecycle().state().is_destroyed() {
        return Err(NavigationError::Destroyed);
    }
    let restore_children = restored.as_ref().map(|saved| {
        saved
            .children
            .iter()
            .map(|(key, bytes)| {
                let snapshot = memory
                    .as_ref()
                    .and_then(|memory| memory.children.iter().find(|(k, _)| k == key))
                    .map(|(_, snapshot)| snapshot.clone())
                    .unwrap_or_else(|| crate::StateSnapshot::from_bytes(bytes.clone()));
                (key.clone(), snapshot)
            })
            .collect()
    });
    let next_key = restored.as_ref().map(|saved| saved.next_key).unwrap_or(0);
    let initial = model.normalize(initial);
    model.validate(&initial)?;
    construction.new_retention = !instances.borrow().contains(&key);
    let retained = instances
        .borrow_mut()
        .get_or_create(&key, RetainedNavigation::default)
        .map_err(|error: InstanceKeeperError| NavigationError::Context(error.into()))?;
    construction.previous_scopes = retained.scopes.borrow().clone();
    construction.retained = Some(retained.clone());
    if let Some(restored) = &restored {
        retained.retain(|key| {
            restored
                .children
                .iter()
                .any(|(saved_key, _)| saved_key == key)
        });
    } else {
        retained.retain(|_| false);
    }
    let engine = Rc::new(Engine {
        parent: context.lifecycle(),
        parent_state: Rc::downgrade(&parent_state),
        parent_instances: Rc::downgrade(&instances),
        back: context.back_handler(),
        retained,
        config: context.configuration(),
        context_factory: context.context_factory(),
        child_factory: Rc::new(factory),
        key,
        model,
        memory_navigation,
        next_key: Cell::new(next_key),
        items: RefCell::new(Vec::new()),
        navigation_state: RefCell::new(Rc::new(initial.clone())),
        state: RefCell::new(None),
        queue: RefCell::new(VecDeque::new()),
        processing: Cell::new(false),
        initializing: Cell::new(true),
        destroyed: Cell::new(false),
        deferred: Cell::new(false),
        source_subscription: RefCell::new(None),
        destroy_subscription: Cell::new(None),
        pop_handle: Cell::new(None),
    });
    let cleanup_engine = engine.clone();
    construction.cleanup = Some(Box::new(move || cleanup_engine.teardown()));
    let weak = Rc::downgrade(&engine);
    *receiver.borrow_mut() = Some(Rc::new(move |event| {
        if let Some(engine) = weak.upgrade() {
            engine.enqueue(event);
        }
    }));
    *engine.queue.borrow_mut() = std::mem::take(&mut *pending.borrow_mut())
        .into_iter()
        .map(|event| QueuedEvent {
            event,
            operation: DiagnosticOperation::Navigation,
            diagnose: !crate::settings::is_reporting(),
        })
        .collect();
    *engine.source_subscription.borrow_mut() = subscription.0.take();
    engine.switch(initial, restore_children)?;
    let weak = Rc::downgrade(&engine);
    let handle = engine
        .back
        .register(
            0,
            engine.model.back_enabled(&engine.navigation_snapshot()),
            move |event, _| {
                if event.phase == crate::BackPhase::Invoked
                    && let Some(engine) = weak.upgrade()
                {
                    engine.enqueue_kind(
                        Event {
                            transformer: {
                                let weak = Rc::downgrade(&engine);
                                Rc::new(move |old| {
                                    weak.upgrade().map_or_else(
                                        || old.clone(),
                                        |engine| engine.model.back_transform(old.clone()),
                                    )
                                })
                            },
                            completion: {
                                let weak = Rc::downgrade(&engine);
                                Rc::new(move |result| {
                                    if let Err(error) = result
                                        && let Some(engine) = weak.upgrade()
                                    {
                                        engine.model.on_back_error(error);
                                    }
                                })
                            },
                        },
                        DiagnosticOperation::Back,
                    );
                }
            },
        )
        .map_err(DecomposeError::from)?;
    engine.pop_handle.set(Some(handle.id()));
    engine.ensure_alive()?;
    // Register a fallible provider so codec failures propagate through Essenty.
    let provider = engine.clone();
    parent_state
        .borrow_mut()
        .replace_reserved(&engine.key, move || {
            provider.save().map_err(|error| {
                report_navigation_error(
                    provider.config.as_ref(),
                    DiagnosticOperation::Save,
                    &provider.key,
                    &error,
                );
                error.to_string()
            })
        })
        .map_err(DecomposeError::from)?;
    let provider = engine.clone();
    parent_state
        .borrow_mut()
        .add_snapshot_provider(&engine.key, move || {
            provider.save_snapshot().map_err(|error| {
                report_navigation_error(
                    provider.config.as_ref(),
                    DiagnosticOperation::Save,
                    &provider.key,
                    &error,
                );
                error.to_string()
            })
        });
    let weak = Rc::downgrade(&engine);
    engine.destroy_subscription.set(Some(
        engine
            .parent
            .do_on_destroy(move || {
                if let Some(engine) = weak.upgrade() {
                    engine.teardown();
                }
            })
            .detach(),
    ));
    let initial_state = engine.navigation_snapshot();
    engine.model.on_state_changed(&initial_state, None);
    engine.initializing.set(false);
    engine.ensure_alive()?;
    engine.pump();
    engine.ensure_alive()?;
    let value = engine
        .state
        .borrow()
        .as_ref()
        .expect("initialized stack")
        .value();
    construction.committed = true;
    let weak = Rc::downgrade(&engine);
    let alive = weak.clone();
    let dispatch = weak.clone();
    let availability = weak.clone();
    let control = NavigationControl {
        alive: Rc::new(move || {
            alive
                .upgrade()
                .ok_or(NavigationError::Destroyed)?
                .ensure_alive()
        }),
        availability: Rc::new(move |key| {
            availability
                .upgrade()
                .ok_or(NavigationError::Destroyed)?
                .ensure_child_available(key)
        }),
        dispatch: Rc::new(move |event| match dispatch.upgrade() {
            Some(engine) if engine.ensure_alive().is_ok() => engine.enqueue(event),
            _ => (event.completion)(Err(NavigationError::Destroyed)),
        }),
        synchronous: Rc::new(move |event| {
            let engine = weak.upgrade().ok_or(NavigationError::Destroyed)?;
            engine.ensure_alive()?;
            if engine.initializing.get()
                || engine.processing.get()
                || engine.back.is_dispatching()
                || engine.deferred.get()
            {
                return Err(NavigationError::NavigationInProgress);
            }
            let result = Rc::new(RefCell::new(None));
            let out = result.clone();
            let completion = event.completion;
            engine.enqueue(Event {
                transformer: event.transformer,
                completion: Rc::new(move |r| {
                    *out.borrow_mut() = Some(r.as_ref().map(|_| ()).map_err(Clone::clone));
                    completion(r);
                }),
            });
            result
                .borrow_mut()
                .take()
                .unwrap_or(Err(NavigationError::ChildUnavailable))
        }),
    };
    Ok((value, control))
}

type ChildAvailability = dyn Fn(&str) -> Result<(), NavigationError>;

/// Weak receiver callbacks share the engine's event queue and ownership.
pub(crate) struct NavigationControl<S> {
    alive: Rc<dyn Fn() -> Result<(), NavigationError>>,
    availability: Rc<ChildAvailability>,
    dispatch: Rc<dyn Fn(Event<S>)>,
    synchronous: Rc<dyn Fn(Event<S>) -> Result<(), NavigationError>>,
}
impl<S> Clone for NavigationControl<S> {
    fn clone(&self) -> Self {
        Self {
            alive: self.alive.clone(),
            availability: self.availability.clone(),
            dispatch: self.dispatch.clone(),
            synchronous: self.synchronous.clone(),
        }
    }
}
impl<S> NavigationControl<S> {
    pub(crate) fn ensure_alive(&self) -> Result<(), NavigationError> {
        (self.alive)()
    }
    pub(crate) fn ensure_child_available(&self, key: &str) -> Result<(), NavigationError> {
        (self.availability)(key)
    }
    pub(crate) fn dispatch(&self, event: Event<S>) {
        (self.dispatch)(event)
    }
    pub(crate) fn dispatch_sync(&self, event: Event<S>) -> Result<(), NavigationError> {
        (self.synchronous)(event)
    }
}
