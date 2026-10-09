use super::navigation::{Event, NavigationResult, NavigationSource};
type ChildFactory<Ctx, C, T> = Rc<dyn Fn(&C, Ctx) -> T>;
type SavedChildren = Vec<(String, BTreeMap<String, Vec<u8>>)>;
use super::{Child, NavigationError};

pub(crate) struct SavedState<S> {
    pub(crate) configurations: S,
    pub(crate) children: SavedChildren,
    pub(crate) next_key: u64,
}
pub(crate) trait Model<C, T>: 'static {
    type State: Clone + 'static;
    type Snapshot: 'static;
    fn persistent(&self) -> bool;
    fn configurations(&self, state: &Self::State) -> Vec<Rc<C>>;
    fn on_state_changed(&self, _state: &Self::State, _old: Option<&Self::State>) {}
    fn validate(&self, state: &Self::State) -> Result<(), NavigationError>;
    fn statuses(&self, state: &Self::State) -> Vec<PageStatus>;
    fn snapshot(&self, children: Vec<PageChild<C, T>>, state: &Self::State) -> Self::Snapshot;
    fn back_enabled(&self, state: &Self::State) -> bool;
    fn back_transform(&self, state: Self::State) -> Self::State;
    fn decode(&self, bytes: &[u8]) -> Result<Option<SavedState<Self::State>>, NavigationError>;
    fn encode(
        &self,
        state: &Self::State,
        children: &SavedChildren,
        next_key: u64,
    ) -> Result<Option<Vec<u8>>, NavigationError>;
}
use super::pages::{PageChild, PageStatus};
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
        drop(removed);
    }
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
        self.parent_back.unregister(self.handle);
        self.back.remove_enabled_changed_listener(self.listener);
        let _ = self.merged.lifecycle().unsubscribe(self.lifecycle_listener);
        // Cleanup still runs when a lifecycle observer panics during destroy.
        struct Cleanup<'a> {
            parts: &'a ScopeParts,
            retention: bool,
        }
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                self.parts.merged.shutdown();
                if self.retention {
                    self.parts.instances.borrow_mut().destroy_all();
                }
            }
        }
        let _cleanup = Cleanup {
            parts: self,
            retention: end_retention,
        };
        if !self.local.state().is_destroyed() {
            let _ = self.local.destroy();
        }
    }
    fn activate(&self, status: PageStatus) {
        let active = matches!(status, PageStatus::Started | PageStatus::Resumed);
        self.gate.set(active);
        let effective = matches!(
            self.merged.state(),
            LifecycleState::Started | LifecycleState::Resumed
        );
        self.parent_back
            .set_enabled(self.handle, active && effective && self.back.can_handle());
        let target = match status {
            PageStatus::Created => LifecycleState::Created,
            PageStatus::Started => LifecycleState::Started,
            PageStatus::Resumed => LifecycleState::Resumed,
            PageStatus::Destroyed => LifecycleState::Destroyed,
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
    child: Child<C, T>,
    parts: ScopeParts,
}

struct Managed<C, T> {
    configuration: Rc<C>,
    key: String,
    scope: Option<Rc<Scope<C, T>>>,
    saved: BTreeMap<String, Vec<u8>>,
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
    fn child(&self) -> PageChild<C, T> {
        self.scope.as_ref().map_or_else(
            || PageChild::Destroyed {
                configuration: self.configuration.clone(),
                key: self.key.clone(),
            },
            |s| PageChild::Created(s.child.clone()),
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
                .map_err(|_| NavigationError::State("child state keeper is borrowed".into()))?
                .save()
                .map_err(|error| NavigationError::State(error.to_string())),
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
        for (scope, fresh_retention) in self.scopes.iter().rev() {
            scope.parts.end(*fresh_retention);
            if *fresh_retention {
                self.retained.remove(&scope.child.key);
            }
        }
    }
}

struct Construction {
    state: SharedKeeper,
    instances: SharedInstances,
    key: String,
    restored: Option<Vec<u8>>,
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
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
        if self.registered {
            self.state.borrow_mut().unregister(&self.key);
        }
        if let Some(retained) = &self.retained {
            let replaced = retained
                .scopes
                .replace(std::mem::take(&mut self.previous_scopes));
            drop(replaced);
        }
        if self.new_retention {
            self.instances.borrow_mut().destroy(&self.key);
        }
        if let Some(bytes) = self.restored.take() {
            self.state
                .borrow_mut()
                .return_bytes(self.key.clone(), bytes);
        }
    }
}

struct Engine<Ctx, C, T, M: Model<C, T>> {
    parent: LifecycleRegistry,
    parent_state: Weak<RefCell<StateKeeper>>,
    back: SharedBack,
    retained: Rc<RetainedNavigation>,
    context_factory: ContextFactory<Ctx>,
    child_factory: ChildFactory<Ctx, C, T>,
    key: String,
    model: M,
    next_key: Cell<u64>,
    items: RefCell<Vec<Managed<C, T>>>,
    navigation_state: RefCell<M::State>,
    state: RefCell<Option<MutableValue<M::Snapshot>>>,
    queue: RefCell<VecDeque<Event<M::State>>>,
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
    fn ensure_alive(&self) -> Result<(), NavigationError> {
        if self.destroyed.get() || self.parent.state().is_destroyed() {
            Err(NavigationError::Destroyed)
        } else {
            Ok(())
        }
    }
    fn create(
        &self,
        config: Rc<C>,
        key: String,
        saved: BTreeMap<String, Vec<u8>>,
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
        let state = Rc::new(RefCell::new(StateKeeper::with_restored(saved)));
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
            child: Child {
                configuration: config,
                instance,
                key: key.clone(),
            },
            parts,
        });
        let replaced = self.retained.scopes.borrow_mut().insert(key, owner);
        drop(replaced);
        Ok(scope)
    }
    fn switch(
        &self,
        state: M::State,
        restored: Option<SavedChildren>,
    ) -> Result<(), NavigationError> {
        self.model.validate(&state)?;
        self.ensure_alive()?;
        // Policy callbacks precede preparation, so a policy panic owns no fresh scopes.
        let statuses = self.model.statuses(&state);
        self.ensure_alive()?;
        let configs = self.model.configurations(&state);
        let old_state = self.navigation_state.borrow().clone();
        let old = self.items.borrow().clone();
        let mut matched = vec![false; old.len()];
        let mut next = Vec::with_capacity(configs.len());
        let mut counter = self.next_key.get();
        for (index, config) in configs.iter().enumerate() {
            let existing = old
                .iter()
                .enumerate()
                .find(|(i, item)| !matched[*i] && item.reusable() && item.configuration == *config);
            let mut item = if let Some((i, item)) = existing {
                matched[i] = true;
                item.clone()
            } else {
                let (key, saved) = if let Some(restored) = &restored {
                    restored[index].clone()
                } else {
                    let key = format!("child/{counter}");
                    counter = counter.checked_add(1).ok_or_else(|| {
                        NavigationError::State("child key counter exhausted".into())
                    })?;
                    (key, BTreeMap::new())
                };
                Managed {
                    configuration: config.clone(),
                    key,
                    scope: None,
                    saved,
                }
            };
            if statuses[index] == PageStatus::Destroyed {
                // Save every outgoing live page before any destructive lifecycle delivery.
                item.saved = item.save()?;
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
            if statuses[index] != PageStatus::Destroyed && item.scope.is_none() {
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
        // Stop outgoing or demoted scopes before any incoming scope advances.
        for item in &old {
            if let Some(scope) = &item.scope {
                let target = next
                    .iter()
                    .enumerate()
                    .find(|(_, item)| {
                        item.scope
                            .as_ref()
                            .is_some_and(|other| Rc::ptr_eq(other, scope))
                    })
                    .map(|(i, _)| statuses[i])
                    .unwrap_or(PageStatus::Created);
                let rank = |s| match s {
                    PageStatus::Created => 1,
                    PageStatus::Started => 2,
                    PageStatus::Resumed => 3,
                    PageStatus::Destroyed => 0,
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
        for item in old.iter().rev() {
            if let Some(scope) = &item.scope
                && !next.iter().any(|item| {
                    item.scope
                        .as_ref()
                        .is_some_and(|other| Rc::ptr_eq(other, scope))
                })
            {
                scope.parts.end(true);
                self.ensure_alive()?;
                self.retained.remove(&item.key);
            }
        }
        // Retention only belongs to desired live entries, including restoration tombstones.
        self.retained.retain(|key| {
            next.iter()
                .any(|item| item.key == key && item.scope.is_some())
        });
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
        self.next_key.set(counter);
        *self.items.borrow_mut() = next.clone();
        *self.navigation_state.borrow_mut() = state.clone();
        prepared.committed = true;
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
        if self.destroyed.get() {
            return;
        }
        self.queue.borrow_mut().push_back(event);
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
                            (self.cancel)();
                        }
                    }
                }
                let cancel_weak = weak.clone();
                let mut work = Deferred {
                    cancel: move || {
                        if let Some(engine) = cancel_weak.upgrade() {
                            engine.deferred.set(false);
                            let abandoned = std::mem::take(&mut *engine.queue.borrow_mut());
                            drop(abandoned);
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
        struct Reset<'a, S>(&'a Cell<bool>, &'a RefCell<VecDeque<Event<S>>>);
        impl<C> Drop for Reset<'_, C> {
            fn drop(&mut self) {
                self.0.set(false);
                if std::thread::panicking() {
                    let abandoned = std::mem::take(&mut *self.1.borrow_mut());
                    drop(abandoned);
                }
            }
        }
        let _reset = Reset(&self.processing, &self.queue);
        loop {
            let event = self.queue.borrow_mut().pop_front();
            let Some(event) = event else { break };
            let old_stack = self.navigation_state.borrow().clone();
            let configs = (event.transformer)(old_stack.clone());
            let result = self.switch(configs, None).map(|()| NavigationResult {
                old_stack,
                new_stack: self.navigation_state.borrow().clone(),
            });
            (event.completion)(result);
            if self.destroyed.get() {
                break;
            }
        }
    }
    fn save(&self) -> Result<Option<Vec<u8>>, NavigationError> {
        if !self.model.persistent() {
            return Ok(None);
        }
        let items = self.items.borrow().clone();
        let state = self.navigation_state.borrow().clone();
        let next_key = self.next_key.get();
        let mut children = Vec::with_capacity(items.len());
        for item in &items {
            children.push((item.key.clone(), item.save()?));
        }
        self.model.encode(&state, &children, next_key)
    }
    fn teardown(&self) {
        if self.destroyed.replace(true) {
            return;
        }
        let abandoned = std::mem::take(&mut *self.queue.borrow_mut());
        drop(abandoned);
        self.source_subscription.borrow_mut().take();
        if let Some(handle) = self.pop_handle.take() {
            self.back.unregister(handle);
        }
        for item in self.items.borrow().iter().rev() {
            if let Some(scope) = &item.scope {
                scope.parts.end(false);
            }
        }
        if let Some(state) = self.parent_state.upgrade() {
            state.borrow_mut().unregister(&self.key);
        }
        if let Some(id) = self.destroy_subscription.take() {
            let _ = self.parent.unsubscribe(id);
        }
    }
}

pub(crate) fn build<Ctx, C, T, M>(
    context: &Ctx,
    navigation: &NavigationSource<M::State>,
    initial: M::State,
    key: String,
    model: M,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<Value<M::Snapshot>, NavigationError>
where
    Ctx: ComponentContext + 'static,
    C: Eq + 'static,
    T: 'static,
    M: Model<C, T>,
{
    if context.lifecycle().state().is_destroyed() {
        return Err(NavigationError::Destroyed);
    }
    let parent_state = context.state_keeper();
    if parent_state.borrow().is_registered(&key) {
        return Err(DecomposeError::DuplicateKey { key }.into());
    }
    let restored_bytes = parent_state.borrow_mut().consume_bytes(&key);
    let instances = context.instance_keeper();
    let mut construction = Construction {
        state: parent_state.clone(),
        instances: instances.clone(),
        key: key.clone(),
        restored: restored_bytes.clone(),
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
    let restored = match restored_bytes.as_ref() {
        Some(bytes) => model.decode(bytes)?,
        None => None,
    };
    let initial = restored
        .as_ref()
        .map(|state| state.configurations.clone())
        .unwrap_or(initial);
    let restore_children = restored.as_ref().map(|saved| saved.children.clone());
    let next_key = restored.as_ref().map(|saved| saved.next_key).unwrap_or(0);
    model.validate(&initial)?;
    if let Some(restored) = &restored {
        for (key, _) in &restored.children {
            let counter = key
                .strip_prefix("child/")
                .and_then(|suffix| suffix.parse::<u64>().ok());
            if !counter
                .is_some_and(|counter| counter < next_key && key == &format!("child/{counter}"))
            {
                return Err(NavigationError::State(
                    "invalid child identity or next-key counter".into(),
                ));
            }
        }
    }
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
        back: context.back_handler(),
        retained,
        context_factory: context.context_factory(),
        child_factory: Rc::new(factory),
        key,
        model,
        next_key: Cell::new(next_key),
        items: RefCell::new(Vec::new()),
        navigation_state: RefCell::new(initial.clone()),
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
    *engine.source_subscription.borrow_mut() = Some(navigation.subscribe(move |event| {
        if let Some(engine) = weak.upgrade() {
            engine.enqueue(event);
        }
    }));
    engine.switch(initial, restore_children)?;
    let weak = Rc::downgrade(&engine);
    let handle = engine
        .back
        .register(
            0,
            engine.model.back_enabled(&engine.navigation_state.borrow()),
            move |event, _| {
                if event.phase == crate::BackPhase::Invoked
                    && let Some(engine) = weak.upgrade()
                {
                    engine.enqueue(Event {
                        transformer: {
                            let weak = Rc::downgrade(&engine);
                            Rc::new(move |old| {
                                weak.upgrade().map_or_else(
                                    || old.clone(),
                                    |engine| engine.model.back_transform(old.clone()),
                                )
                            })
                        },
                        completion: Rc::new(|_| {}),
                    });
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
            provider.save().map_err(|error| error.to_string())
        })
        .map_err(DecomposeError::from)?;
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
    let initial_state = engine.navigation_state.borrow().clone();
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
    Ok(value)
}
