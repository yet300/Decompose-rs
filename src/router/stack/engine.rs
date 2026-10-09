use super::navigation::Event;
type ChildFactory<Ctx, C, T> = Rc<dyn Fn(&C, Ctx) -> T>;
type SavedChildren = Vec<(String, BTreeMap<String, Vec<u8>>)>;
use super::{
    Child, ChildStack, NavigationError, NavigationResult, StackCodec, StackNavigation, StackOptions,
};
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
struct RetainedStack {
    scopes: RefCell<BTreeMap<String, Rc<RetainedScope>>>,
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
}
impl ScopeParts {
    fn end(&self, end_retention: bool) {
        self.parent_back.unregister(self.handle);
        self.back.remove_enabled_changed_listener(self.listener);
        if !self.local.state().is_destroyed() {
            let _ = self.local.destroy();
        }
        self.merged.shutdown();
        if end_retention {
            self.instances.borrow_mut().destroy_all();
        }
    }
    fn activate(&self, active: bool) {
        // Back-stack entries are stopped before any new active child resumes.
        self.gate.set(active);
        self.parent_back
            .set_enabled(self.handle, active && self.back.can_handle());
        let target = if active {
            LifecycleState::Resumed
        } else {
            LifecycleState::Created
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

struct Prepared<C, T> {
    scopes: Vec<(Rc<Scope<C, T>>, bool)>,
    retained: Rc<RetainedStack>,
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
                let removed = self.retained.scopes.borrow_mut().remove(&scope.child.key);
                drop(removed);
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
    retained: Option<Rc<RetainedStack>>,
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

struct Engine<Ctx, C, T> {
    parent: LifecycleRegistry,
    parent_state: Weak<RefCell<StateKeeper>>,
    back: SharedBack,
    retained: Rc<RetainedStack>,
    context_factory: ContextFactory<Ctx>,
    child_factory: ChildFactory<Ctx, C, T>,
    key: String,
    duplicates: bool,
    handle_back: bool,
    codec: Option<StackCodec<C>>,
    next_key: Cell<u64>,
    items: RefCell<Vec<Rc<Scope<C, T>>>>,
    state: RefCell<Option<MutableValue<ChildStack<C, T>>>>,
    queue: RefCell<VecDeque<Event<C>>>,
    processing: Cell<bool>,
    initializing: Cell<bool>,
    destroyed: Cell<bool>,
    deferred: Cell<bool>,
    source_subscription: RefCell<Option<Subscription>>,
    destroy_subscription: Cell<Option<u64>>,
    pop_handle: Cell<Option<u64>>,
}

impl<Ctx: ComponentContext + 'static, C: Eq + 'static, T: 'static> Engine<Ctx, C, T> {
    fn ensure_alive(&self) -> Result<(), NavigationError> {
        if self.destroyed.get() || self.parent.state().is_destroyed() {
            Err(NavigationError::Destroyed)
        } else {
            Ok(())
        }
    }
    fn validate(&self, configs: &[Rc<C>]) -> Result<(), NavigationError> {
        if configs.is_empty() {
            return Err(NavigationError::EmptyStack);
        }
        if !self.duplicates
            && configs
                .iter()
                .enumerate()
                .any(|(i, c)| configs[..i].iter().any(|old| old == c))
        {
            return Err(NavigationError::DuplicateConfiguration);
        }
        Ok(())
    }
    fn configs(&self) -> Vec<Rc<C>> {
        self.items
            .borrow()
            .iter()
            .map(|item| item.child.configuration.clone())
            .collect()
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
        let listener_gate = gate.clone();
        let listener = back.add_enabled_changed_listener(move |enabled| {
            if let Some(parent) = parent.upgrade() {
                parent.set_enabled(id, listener_gate.get() && enabled);
            }
        });
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
        self.retained.scopes.borrow_mut().insert(key, owner);
        Ok(scope)
    }
    fn switch(
        &self,
        configs: Vec<Rc<C>>,
        restored: Option<SavedChildren>,
    ) -> Result<(), NavigationError> {
        self.validate(&configs)?;
        if self.destroyed.get() {
            return Err(NavigationError::Destroyed);
        }
        let old = self.items.borrow().clone();
        let mut matched = vec![false; old.len()];
        let mut next = Vec::with_capacity(configs.len());
        let mut prepared = Prepared {
            scopes: Vec::new(),
            retained: self.retained.clone(),
            committed: false,
        };
        // Prepared children are owned locally until membership is committed.
        // Their scope guards clean up on a factory panic or a later error.
        for (index, config) in configs.into_iter().enumerate() {
            let existing = old
                .iter()
                .enumerate()
                .find(|(i, item)| !matched[*i] && item.child.configuration == config);
            if let Some((i, item)) = existing {
                matched[i] = true;
                next.push(item.clone());
            } else {
                let (key, saved) = if let Some(restored) = restored.as_ref() {
                    restored[index].clone()
                } else {
                    let counter = self.next_key.get();
                    let next_counter = counter.checked_add(1).ok_or_else(|| {
                        NavigationError::State("child key counter exhausted".into())
                    })?;
                    self.next_key.set(next_counter);
                    (format!("child/{counter}"), BTreeMap::new())
                };
                let fresh_retention = self
                    .retained
                    .scopes
                    .borrow()
                    .get(&key)
                    .is_none_or(|scope| scope.instances.borrow().is_destroyed());
                let scope = self.create(config, key, saved)?;
                prepared.scopes.push((scope.clone(), fresh_retention));
                next.push(scope);
            }
        }
        // First stop every child that will be inactive, including old top.
        for item in &old {
            if !next.last().is_some_and(|top| Rc::ptr_eq(top, item)) {
                item.parts.activate(false);
            }
            self.ensure_alive()?;
        }
        for (index, item) in old.iter().enumerate().rev() {
            if !matched[index] {
                item.parts.end(true);
                self.ensure_alive()?;
                self.retained.scopes.borrow_mut().remove(&item.child.key);
            }
        }
        for item in next.iter().take(next.len() - 1) {
            item.parts.activate(false);
            self.ensure_alive()?;
        }
        // Commit only after activation succeeds. Reentrant callbacks append
        // events and cannot observe partly prepared membership.
        next.last()
            .expect("validated nonempty stack")
            .parts
            .activate(true);
        self.ensure_alive()?;
        if let Some(handle) = self.pop_handle.get() {
            self.back
                .set_enabled(handle, self.handle_back && next.len() > 1);
        }
        self.ensure_alive()?;
        *self.items.borrow_mut() = next.clone();
        prepared.committed = true;
        let snapshot = Self::snapshot(&next);
        let state = self.state.borrow().clone();
        if let Some(state) = state {
            state.set(snapshot);
        } else {
            *self.state.borrow_mut() = Some(MutableValue::new(snapshot));
        }
        Ok(())
    }
    fn snapshot(items: &[Rc<Scope<C, T>>]) -> ChildStack<C, T> {
        ChildStack {
            active: items.last().expect("nonempty stack").child.clone(),
            back_stack: items[..items.len() - 1]
                .iter()
                .map(|item| item.child.clone())
                .collect(),
        }
    }
    fn enqueue(self: &Rc<Self>, event: Event<C>) {
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
        struct Reset<'a, C>(&'a Cell<bool>, &'a RefCell<VecDeque<Event<C>>>);
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
            let old_stack = self.configs();
            let configs = (event.transformer)(old_stack.clone());
            let result = self.switch(configs, None).map(|()| NavigationResult {
                old_stack,
                new_stack: self.configs(),
            });
            (event.completion)(result);
            if self.destroyed.get() {
                break;
            }
        }
    }
    fn save(&self) -> Result<Option<Vec<u8>>, NavigationError> {
        let Some(codec) = &self.codec else {
            return Ok(None);
        };
        let items = self.items.borrow().clone();
        let configs: Vec<_> = items
            .iter()
            .map(|item| item.child.configuration.clone())
            .collect();
        let next_key = self.next_key.get();
        let mut children = Vec::with_capacity(items.len());
        for item in &items {
            let state = item
                .parts
                .state
                .try_borrow()
                .map_err(|_| NavigationError::State("child state keeper is borrowed".into()))?;
            let saved = state
                .save()
                .map_err(|error| NavigationError::State(error.to_string()))?;
            children.push((item.child.key.clone(), saved));
        }
        super::state::encode(codec, &configs, &children, next_key).map(Some)
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
            item.parts.end(false);
        }
        if let Some(state) = self.parent_state.upgrade() {
            state.borrow_mut().unregister(&self.key);
        }
        if let Some(id) = self.destroy_subscription.take() {
            let _ = self.parent.unsubscribe(id);
        }
    }
}

pub(super) fn build<Ctx, C, T>(
    context: &Ctx,
    navigation: &StackNavigation<C>,
    initial: Vec<C>,
    options: StackOptions<C>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<Value<ChildStack<C, T>>, NavigationError>
where
    Ctx: ComponentContext + 'static,
    C: Eq + 'static,
    T: 'static,
{
    if context.lifecycle().state().is_destroyed() {
        return Err(NavigationError::Destroyed);
    }
    let parent_state = context.state_keeper();
    if parent_state.borrow().is_registered(&options.key) {
        return Err(DecomposeError::DuplicateKey { key: options.key }.into());
    }
    let restored_bytes = parent_state.borrow_mut().consume_bytes(&options.key);
    let instances = context.instance_keeper();
    let mut construction = Construction {
        state: parent_state.clone(),
        instances: instances.clone(),
        key: options.key.clone(),
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
        .register_optional(&options.key, move || fallback.clone())
        .map_err(DecomposeError::from)?;
    construction.registered = true;
    let restored = match (&options.codec, restored_bytes.as_ref()) {
        (Some(codec), Some(bytes)) => match super::state::decode(codec, bytes) {
            Ok(saved) => Some(saved),
            Err(error) => {
                return Err(error);
            }
        },
        _ => None,
    };
    let initial = restored
        .as_ref()
        .map(|state| state.configurations.clone())
        .unwrap_or_else(|| initial.into_iter().map(Rc::new).collect());
    let restore_children = restored.as_ref().map(|saved| saved.children.clone());
    let next_key = restored.as_ref().map(|saved| saved.next_key).unwrap_or(0);
    if initial.is_empty() {
        return Err(NavigationError::EmptyStack);
    }
    if !options.allow_duplicates
        && initial
            .iter()
            .enumerate()
            .any(|(i, c)| initial[..i].contains(c))
    {
        return Err(NavigationError::DuplicateConfiguration);
    }
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
    construction.new_retention = !instances.borrow().contains(&options.key);
    let retained = instances
        .borrow_mut()
        .get_or_create(&options.key, RetainedStack::default)
        .map_err(|error: InstanceKeeperError| NavigationError::Context(error.into()))?;
    construction.previous_scopes = retained.scopes.borrow().clone();
    construction.retained = Some(retained.clone());
    if let Some(restored) = &restored {
        retained.scopes.borrow_mut().retain(|key, _| {
            restored
                .children
                .iter()
                .any(|(saved_key, _)| saved_key == key)
        });
    } else {
        retained.scopes.borrow_mut().clear();
    }
    let engine = Rc::new(Engine {
        parent: context.lifecycle(),
        parent_state: Rc::downgrade(&parent_state),
        back: context.back_handler(),
        retained,
        context_factory: context.context_factory(),
        child_factory: Rc::new(factory),
        key: options.key,
        duplicates: options.allow_duplicates,
        handle_back: options.handle_back_button,
        codec: options.codec,
        next_key: Cell::new(next_key),
        items: RefCell::new(Vec::new()),
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
            engine.handle_back && engine.items.borrow().len() > 1,
            move |event, _| {
                if event.phase == crate::BackPhase::Invoked
                    && let Some(engine) = weak.upgrade()
                {
                    engine.enqueue(Event {
                        transformer: Rc::new(|mut old| {
                            if old.len() > 1 {
                                old.pop();
                            }
                            old
                        }),
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
