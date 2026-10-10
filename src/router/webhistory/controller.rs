use super::*;
use essenty_lifecycle::LifecycleRegistry;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};

#[derive(Clone)]
struct Node {
    key: Vec<u8>,
    path: String,
    parameters: Vec<(String, String)>,
    children: Vec<Node>,
    child_identity: Option<*const ()>,
}
impl Node {
    fn saved(&self) -> HistoryNode {
        HistoryNode {
            key: self.key.clone(),
            path: self.path.clone(),
            children: self.children.iter().map(Node::saved).collect(),
        }
    }
    fn url(&self) -> String {
        let mut paths = Vec::new();
        let mut parameters: Vec<(String, String)> = Vec::new();
        let mut node = Some(self);
        while let Some(current) = node {
            if !current.path.is_empty() {
                paths.push(current.path.trim_matches('/'));
            }
            for (key, value) in &current.parameters {
                if let Some((_, old)) = parameters.iter_mut().find(|(name, _)| name == key) {
                    *old = value.clone();
                } else {
                    parameters.push((key.clone(), value.clone()));
                }
            }
            node = current.children.last();
        }
        let mut url = format!("/{}", paths.join("/"));
        if !parameters.is_empty() {
            url.push('?');
            url.push_str(
                &parameters
                    .iter()
                    .map(|(key, value)| {
                        format!("{}={}", percent_encode(key), percent_encode(value))
                    })
                    .collect::<Vec<_>>()
                    .join("&"),
            );
        }
        url
    }
}
fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            output.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(output, "%{byte:02X}").unwrap();
        }
    }
    output
}
fn snapshot(
    navigation: &Rc<dyn WebNavigation>,
    ancestors: &mut Vec<*const ()>,
) -> Result<Vec<Node>, HistoryError> {
    let pointer = Rc::as_ptr(navigation) as *const ();
    if ancestors.contains(&pointer) {
        return Err(HistoryError::CyclicNavigation);
    }
    ancestors.push(pointer);
    let history = navigation.history()?;
    if history.is_empty() {
        return Err(HistoryError::EmptyHistory);
    }
    let nodes = history
        .into_iter()
        .map(|item| {
            Ok(Node {
                key: item.key,
                path: item.path,
                parameters: item.parameters,
                child_identity: item
                    .child
                    .as_ref()
                    .map(|child| Rc::as_ptr(child) as *const ()),
                children: item
                    .child
                    .as_ref()
                    .map(|child| snapshot(child, ancestors))
                    .transpose()?
                    .unwrap_or_default(),
            })
        })
        .collect();
    ancestors.pop();
    nodes
}
fn entry(nodes: &[Node], index: usize) -> HistoryEntry {
    HistoryEntry {
        state: Some(HistoryState {
            index,
            nodes: nodes.iter().map(Node::saved).collect(),
        }),
        url: nodes.last().unwrap().url(),
    }
}
fn expanded(nodes: &[Node]) -> Vec<Vec<Node>> {
    let mut result = Vec::new();
    let mut previous = Vec::new();
    for node in nodes {
        let children = if node.children.is_empty() {
            vec![Vec::new()]
        } else {
            expanded(&node.children)
        };
        for children in children {
            let mut item = node.clone();
            item.children = children;
            let mut history = previous.clone();
            history.push(item);
            result.push(history);
        }
        previous.push(node.clone());
    }
    result
}
enum Change {
    Push(Vec<Vec<Node>>),
    Pop(usize, Vec<Node>),
    Rewrite(Vec<Vec<Node>>),
    Update(Vec<Node>),
    LegacyRewrite(usize, Vec<Vec<Node>>, bool),
}
fn changes(new: &[Node], old: &[Node], emitter: Option<*const ()>, owner: *const ()) -> Change {
    let new_keys: Vec<_> = new.iter().map(|n| &n.key).collect();
    let old_keys: Vec<_> = old.iter().map(|n| &n.key).collect();
    if new_keys == old_keys {
        if let (Some(new_active), Some(old_active)) = (new.last(), old.last())
            && emitter != Some(owner)
            && new_active.child_identity == old_active.child_identity
            && !new_active.children.is_empty()
            && !old_active.children.is_empty()
        {
            let lift = |children: Vec<Node>| {
                let mut outer = new.to_vec();
                outer.last_mut().unwrap().children = children;
                outer
            };
            return match changes(
                &new_active.children,
                &old_active.children,
                emitter,
                new_active.child_identity.unwrap(),
            ) {
                Change::Push(histories) => Change::Push(histories.into_iter().map(lift).collect()),
                Change::Pop(count, children) => Change::Pop(count, lift(children)),
                Change::Rewrite(histories) => {
                    Change::Rewrite(histories.into_iter().map(lift).collect())
                }
                Change::Update(_) => Change::Update(new.to_vec()),
                Change::LegacyRewrite(..) => unreachable!(),
            };
        }
        Change::Update(new.to_vec())
    } else if new_keys.starts_with(&old_keys) {
        let mut previous = old.to_vec();
        let mut histories = Vec::new();
        for item in &new[old.len()..] {
            for suffix in expanded(std::slice::from_ref(item)) {
                histories.push(previous.iter().cloned().chain(suffix).collect());
            }
            previous.push(item.clone());
        }
        Change::Push(histories)
    } else if old_keys.starts_with(&new_keys) {
        let count = expanded(&old[new.len()..]).len();
        Change::Pop(count, new.to_vec())
    } else {
        Change::Rewrite(expanded(new))
    }
}

fn legacy_changes(new: &[Node], old: &[Node]) -> Change {
    let common = new
        .iter()
        .zip(old)
        .take_while(|(a, b)| a.key == b.key)
        .count();
    if common == new.len() && common == old.len() {
        Change::Update(new.to_vec())
    } else if common == old.len() {
        Change::Push(expanded(new).into_iter().skip(common).collect())
    } else if common == new.len() {
        Change::Pop(old.len() - new.len(), new.to_vec())
    } else if common == old.len() - 1 {
        Change::LegacyRewrite(0, expanded(new).into_iter().skip(common).collect(), true)
    } else if common > 0 {
        Change::LegacyRewrite(
            old.len() - common,
            expanded(new).into_iter().skip(common).collect(),
            false,
        )
    } else {
        Change::LegacyRewrite(old.len() - 1, expanded(new), true)
    }
}
struct HostWrite {
    histories: Vec<Vec<Node>>,
    replace_first: bool,
}

enum Event {
    Changed(Option<*const ()>),
    Pop(Option<HistoryState>),
    Restored {
        id: u64,
        index: usize,
        result: Result<(), HistoryError>,
    },
    Moved(Result<(), HistoryError>, Option<HostWrite>),
}
type ErrorHandler = dyn Fn(HistoryError);
struct Binding {
    navigation: RefCell<Option<Rc<dyn WebNavigation>>>,
    legacy: bool,
    backend: Rc<dyn HistoryBackend>,
    previous: RefCell<Vec<Node>>,
    subscriptions: RefCell<Vec<Subscription>>,
    host: RefCell<Option<Subscription>>,
    lifecycle: RefCell<Option<essenty_lifecycle::Subscription>>,
    queue: RefCell<VecDeque<Event>>,
    pending_pop: RefCell<VecDeque<Option<HistoryState>>>,
    restore_id: Cell<u64>,
    active_restore: Cell<Option<u64>>,
    processing: Cell<bool>,
    restoring: Cell<bool>,
    moving: Cell<bool>,
    closed: Cell<bool>,
    committed_index: Cell<usize>,
    changed_while_moving: Cell<bool>,
    errors: Value<Option<HistoryError>>,
    error_source: crate::value::MutableValue<Option<HistoryError>>,
    on_error: RefCell<Option<Rc<ErrorHandler>>>,
}

/// RAII connection between recursive navigation and a host history backend.
/// Dropping or canceling disconnects every observer. A lifecycle binding also
/// cancels on Destroy, even while a deferred host/model completion is pending.
#[must_use = "keep the controller alive while history synchronization is needed"]
pub struct HistoryController {
    binding: Rc<Binding>,
}
impl HistoryController {
    /// Attaches without restoring an existing entry on startup, matching upstream.
    /// With an unmanaged or absent entry, replaces it with the entire current
    /// tree; an initial stack of several items still creates just one host entry.
    pub fn attach(
        navigation: Rc<dyn WebNavigation>,
        backend: Rc<dyn HistoryBackend>,
    ) -> Result<Self, HistoryError> {
        Self::attach_with_error_handler(navigation, backend, |_| {})
    }
    /// Attaches and receives typed failures after construction. The latest error
    /// is also observable through [`Self::errors`]. Reentrant handlers are queued.
    pub fn attach_with_error_handler(
        navigation: Rc<dyn WebNavigation>,
        backend: Rc<dyn HistoryBackend>,
        on_error: impl Fn(HistoryError) + 'static,
    ) -> Result<Self, HistoryError> {
        Self::attach_impl(navigation, backend, on_error, false)
    }
    pub(super) fn attach_legacy(
        navigation: Rc<dyn WebNavigation>,
        backend: Rc<dyn HistoryBackend>,
    ) -> Result<Self, HistoryError> {
        Self::attach_impl(navigation, backend, |_| {}, true)
    }
    fn attach_impl(
        navigation: Rc<dyn WebNavigation>,
        backend: Rc<dyn HistoryBackend>,
        on_error: impl Fn(HistoryError) + 'static,
        legacy: bool,
    ) -> Result<Self, HistoryError> {
        let previous = snapshot(&navigation, &mut Vec::new())?;
        if backend.current().and_then(|entry| entry.state).is_none() {
            if legacy {
                for (index, nodes) in expanded(&previous).into_iter().enumerate() {
                    if index == 0 {
                        backend.replace(entry(&nodes, index))?;
                    } else {
                        backend.push(entry(&nodes, index))?;
                    }
                }
            } else {
                backend.replace(entry(&previous, 0))?;
            }
        }
        let error_source = crate::value::MutableValue::new(None);
        let committed_index = backend
            .current()
            .and_then(|entry| entry.state)
            .map_or(0, |state| state.index);
        validate_index(committed_index)?;
        let binding = Rc::new(Binding {
            legacy,
            navigation: RefCell::new(Some(navigation)),
            backend,
            previous: RefCell::new(previous),
            subscriptions: RefCell::new(Vec::new()),
            host: RefCell::new(None),
            lifecycle: RefCell::new(None),
            queue: RefCell::new(VecDeque::new()),
            pending_pop: RefCell::new(VecDeque::new()),
            restore_id: Cell::new(0),
            active_restore: Cell::new(None),
            processing: Cell::new(false),
            restoring: Cell::new(false),
            moving: Cell::new(false),
            closed: Cell::new(false),
            committed_index: Cell::new(committed_index),
            changed_while_moving: Cell::new(false),
            errors: error_source.value(),
            error_source,
            on_error: RefCell::new(Some(Rc::new(on_error))),
        });
        let weak = Rc::downgrade(&binding);
        let token = binding.backend.subscribe(Rc::new(move |state| {
            if let Some(binding) = weak.upgrade()
                && !binding.moving.get()
            {
                binding.enqueue(Event::Pop(state));
            }
        }));
        *binding.host.borrow_mut() = Some(token);
        binding.watch()?;
        Ok(Self { binding })
    }
    /// Cancels this binding when the lifecycle is destroyed.
    pub fn with_lifecycle(self, lifecycle: &LifecycleRegistry) -> Self {
        let weak = Rc::downgrade(&self.binding);
        let token = lifecycle.do_on_destroy(move || {
            if let Some(binding) = weak.upgrade() {
                binding.cancel();
            }
        });
        *self.binding.lifecycle.borrow_mut() = Some(token);
        self
    }
    /// Idempotently disconnects all model, host, and lifecycle subscriptions.
    pub fn cancel(&self) {
        self.binding.cancel();
    }
    /// Latest typed failure after attachment. Successful events do not erase it.
    pub fn errors(&self) -> Value<Option<HistoryError>> {
        self.binding.errors.clone()
    }
}
impl Drop for HistoryController {
    fn drop(&mut self) {
        self.cancel();
    }
}
// Complete independent cleanup even when several custom cancellation callbacks
// panic. Preserve the first panic, or the original unwind already in progress.
type PanicPayload = Box<dyn std::any::Any + Send>;
fn cleanup(first: &mut Option<PanicPayload>, action: impl FnOnce()) {
    if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(action))
        && first.is_none()
    {
        *first = Some(panic);
    }
}
fn finish_cleanup(first: Option<PanicPayload>) {
    if !std::thread::panicking()
        && let Some(panic) = first
    {
        std::panic::resume_unwind(panic);
    }
}
fn drop_tokens(tokens: Vec<Subscription>) {
    let mut first = None;
    for token in tokens {
        cleanup(&mut first, || drop(token));
    }
    finish_cleanup(first);
}
impl Binding {
    fn cancel(&self) {
        if self.closed.replace(true) {
            return;
        }
        let tokens = self.subscriptions.take();
        let host = self.host.borrow_mut().take();
        let lifecycle = self.lifecycle.borrow_mut().take();
        let navigation = self.navigation.borrow_mut().take();
        let error_handler = self.on_error.borrow_mut().take();
        let previous = self.previous.take();
        self.queue.borrow_mut().clear();
        self.pending_pop.borrow_mut().clear();
        self.active_restore.set(None);
        let mut first = None;
        for token in tokens {
            cleanup(&mut first, || drop(token));
        }
        cleanup(&mut first, || drop(host));
        cleanup(&mut first, || drop(lifecycle));
        cleanup(&mut first, || drop(navigation));
        cleanup(&mut first, || drop(error_handler));
        cleanup(&mut first, || drop(previous));
        finish_cleanup(first);
    }
    fn report(&self, error: HistoryError) {
        self.error_source.set(Some(error.clone()));
        let handler = self.on_error.borrow().clone();
        if !self.closed.get()
            && let Some(handler) = handler
        {
            handler(error);
        }
    }
    fn navigation(&self) -> Result<Rc<dyn WebNavigation>, HistoryError> {
        self.navigation
            .borrow()
            .clone()
            .ok_or_else(|| super::super::NavigationError::Destroyed.into())
    }
    fn watch(self: &Rc<Self>) -> Result<(), HistoryError> {
        let mut navigations = Vec::new();
        let mut current = Some(self.navigation()?);
        let mut visited = Vec::new();
        while let Some(navigation) = current {
            let pointer = Rc::as_ptr(&navigation) as *const ();
            if visited.contains(&pointer) {
                return Err(HistoryError::CyclicNavigation);
            }
            visited.push(pointer);
            let history = navigation.history()?;
            current = history
                .last()
                .ok_or(HistoryError::EmptyHistory)?
                .child
                .clone();
            navigations.push(navigation);
        }
        let mut tokens = Vec::new();
        for navigation in navigations {
            let weak = Rc::downgrade(self);
            let emitter = Rc::as_ptr(&navigation) as *const ();
            tokens.push(navigation.subscribe(Rc::new(move || {
                if let Some(binding) = weak.upgrade() {
                    binding.enqueue(Event::Changed(Some(emitter)));
                }
            })));
        }
        let previous = self.subscriptions.replace(tokens);
        drop_tokens(previous);
        Ok(())
    }
    fn enqueue(self: &Rc<Self>, event: Event) {
        if self.closed.get() {
            return;
        }
        self.queue.borrow_mut().push_back(event);
        if self.processing.replace(true) {
            return;
        }
        struct Guard<'a>(&'a Binding);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.processing.set(false);
                if std::thread::panicking() {
                    self.0.restoring.set(false);
                    self.0.active_restore.set(None);
                    self.0.pending_pop.borrow_mut().clear();
                    self.0.moving.set(false);
                    self.0.queue.borrow_mut().clear();
                }
            }
        }
        let _guard = Guard(self);
        loop {
            let event = self.queue.borrow_mut().pop_front();
            let Some(event) = event else {
                break;
            };
            if self.closed.get() {
                break;
            }
            if let Err(error) = self.process(event) {
                self.report(error);
            }
            if !self.restoring.get() && !self.moving.get() && !self.closed.get() {
                let next = self.pending_pop.borrow_mut().pop_front();
                if let Some(state) = next {
                    self.queue.borrow_mut().push_back(Event::Pop(state));
                }
            }
        }
    }
    fn index(&self) -> Result<usize, HistoryError> {
        let index = self
            .backend
            .current()
            .and_then(|entry| entry.state)
            .map_or(0, |state| state.index);
        validate_index(index)?;
        Ok(index)
    }
    fn write(&self, histories: Vec<Vec<Node>>, replace_first: bool) -> Result<(), HistoryError> {
        for (i, nodes) in histories.into_iter().enumerate() {
            if self.closed.get() {
                return Ok(());
            }
            if replace_first && i == 0 {
                self.backend.replace(entry(&nodes, self.index()?))?;
            } else {
                let index = self
                    .index()?
                    .checked_add(1)
                    .ok_or_else(|| HistoryError::Backend("history index exhausted".into()))?;
                validate_index(index)?;
                self.backend.push(entry(&nodes, index))?;
            }
            self.committed_index.set(self.index()?);
        }
        Ok(())
    }
    fn move_host(self: &Rc<Self>, delta: isize, rewrite: Option<HostWrite>) {
        self.moving.set(true);
        let weak = Rc::downgrade(self);
        self.backend.go(
            delta,
            Box::new(move |result| {
                if let Some(binding) = weak.upgrade() {
                    binding.enqueue(Event::Moved(result, rewrite));
                }
            }),
        );
    }
    fn process(self: &Rc<Self>, event: Event) -> Result<(), HistoryError> {
        if self.restoring.get()
            && let Event::Pop(state) = event
        {
            self.pending_pop.borrow_mut().push_back(state);
            return Ok(());
        }
        match event {
            Event::Changed(emitter) => {
                let navigation = self.navigation()?;
                let nodes = snapshot(&navigation, &mut Vec::new())?;
                if self.moving.get() {
                    self.changed_while_moving.set(true);
                    self.watch()?;
                    return Ok(());
                }
                let old = self.previous.replace(nodes.clone());
                self.watch()?;
                if self.restoring.get() {
                    return Ok(());
                }
                let change = if self.legacy {
                    legacy_changes(&nodes, &old)
                } else {
                    changes(&nodes, &old, emitter, Rc::as_ptr(&navigation) as *const ())
                };
                match change {
                    Change::Push(histories) => self.write(histories, false)?,
                    Change::Pop(count, nodes) => {
                        let available = count.min(self.index()?);
                        if available > 0 {
                            self.move_host(
                                -(available as isize),
                                (available < count).then_some(HostWrite {
                                    histories: vec![nodes],
                                    replace_first: true,
                                }),
                            );
                        } else {
                            self.backend.replace(entry(&nodes, self.index()?))?;
                            self.committed_index.set(self.index()?);
                        }
                    }
                    Change::Rewrite(histories) => {
                        let index = self.index()?;
                        if index > 0 {
                            self.move_host(
                                -(index as isize),
                                Some(HostWrite {
                                    histories,
                                    replace_first: true,
                                }),
                            );
                        } else {
                            self.write(histories, true)?;
                        }
                    }
                    Change::LegacyRewrite(count, histories, replace_first) => {
                        if count > 0 {
                            self.move_host(
                                -(count.min(self.index()?) as isize),
                                Some(HostWrite {
                                    histories,
                                    replace_first,
                                }),
                            );
                        } else {
                            self.write(histories, replace_first)?;
                        }
                    }
                    Change::Update(nodes) => {
                        self.backend.replace(entry(&nodes, self.index()?))?;
                        self.committed_index.set(self.index()?);
                    }
                }
            }
            Event::Pop(Some(state)) => {
                validate_index(state.index)?;
                if state.nodes.is_empty() {
                    return Err(HistoryError::EmptyHistory);
                }
                if !before_recursive(&self.navigation()?, &state.nodes)? {
                    let delta = self.committed_index.get() as isize - self.index()? as isize;
                    if delta != 0 {
                        self.move_host(delta, None);
                    }
                    return Ok(());
                }
                self.restoring.set(true);
                let id =
                    self.restore_id.get().checked_add(1).ok_or_else(|| {
                        HistoryError::Backend("restoration counter exhausted".into())
                    })?;
                self.restore_id.set(id);
                self.active_restore.set(Some(id));
                let index = state.index;
                let weak = Rc::downgrade(self);
                let completion: HistoryCompletion = Box::new(move |result| {
                    if let Some(binding) = weak.upgrade() {
                        binding.enqueue(Event::Restored { id, index, result });
                    }
                });
                if self.legacy {
                    self.navigation()?.navigate_history(state.nodes, completion);
                } else {
                    restore(
                        Rc::downgrade(self),
                        self.navigation()?,
                        state.nodes,
                        completion,
                    );
                }
            }
            Event::Pop(None) => {}
            Event::Restored { id, index, result } => {
                if self.active_restore.get() != Some(id) {
                    return Ok(());
                }
                self.active_restore.set(None);
                self.restoring.set(false);
                if result.is_ok() {
                    self.committed_index.set(index);
                }
                let has_next = !self.pending_pop.borrow().is_empty();
                let nodes = snapshot(&self.navigation()?, &mut Vec::new())?;
                *self.previous.borrow_mut() = nodes.clone();
                self.watch()?;
                if !has_next
                    && self
                        .backend
                        .current()
                        .and_then(|entry| entry.state)
                        .is_some()
                {
                    let current = self.index()?;
                    if current == index {
                        self.backend.replace(entry(&nodes, current))?;
                        self.committed_index.set(current);
                    } else if result.is_ok() {
                        self.move_host(
                            index as isize - current as isize,
                            Some(HostWrite {
                                histories: vec![nodes],
                                replace_first: true,
                            }),
                        );
                    }
                }
                result?;
            }
            Event::Moved(result, rewrite) => {
                self.moving.set(false);
                result?;
                if let Some(write) = rewrite {
                    self.write(write.histories, write.replace_first)?;
                }
                self.committed_index.set(self.index()?);
                if self.changed_while_moving.replace(false) {
                    self.enqueue(Event::Changed(None));
                }
            }
        }
        Ok(())
    }
}
fn validate_index(index: usize) -> Result<(), HistoryError> {
    if index > isize::MAX as usize {
        return Err(HistoryError::Backend(
            "invalid managed history index".into(),
        ));
    }
    Ok(())
}
fn before_recursive(
    navigation: &Rc<dyn WebNavigation>,
    nodes: &[HistoryNode],
) -> Result<bool, HistoryError> {
    before_recursive_checked(navigation, nodes, &mut Vec::new())
}
fn before_recursive_checked(
    navigation: &Rc<dyn WebNavigation>,
    nodes: &[HistoryNode],
    ancestors: &mut Vec<*const ()>,
) -> Result<bool, HistoryError> {
    let pointer = Rc::as_ptr(navigation) as *const ();
    if ancestors.contains(&pointer) {
        return Err(HistoryError::CyclicNavigation);
    }
    ancestors.push(pointer);
    let history = navigation.history()?;
    if let Some(child) = history
        .last()
        .ok_or(HistoryError::EmptyHistory)?
        .child
        .as_ref()
        && !before_recursive_checked(
            child,
            nodes.last().map_or(&[], |node| node.children.as_slice()),
            ancestors,
        )?
    {
        return Ok(false);
    }
    ancestors.pop();
    navigation.before_navigate_to(nodes)
}
struct RestoreUnwindGuard<'a>(&'a Binding);
impl Drop for RestoreUnwindGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.processing.set(false);
            self.0.restoring.set(false);
            self.0.active_restore.set(None);
            self.0.pending_pop.borrow_mut().clear();
            self.0.moving.set(false);
            self.0.queue.borrow_mut().clear();
        }
    }
}

fn restore(
    binding: std::rc::Weak<Binding>,
    navigation: Rc<dyn WebNavigation>,
    nodes: Vec<HistoryNode>,
    completion: HistoryCompletion,
) {
    if binding.upgrade().is_none_or(|binding| binding.closed.get()) {
        return;
    }
    let target = Rc::downgrade(&navigation);
    navigation.navigate_history(
        nodes.clone(),
        Box::new(move |result| {
            let Some(active) = binding.upgrade().filter(|binding| !binding.closed.get()) else {
                return;
            };
            // Deferred completions run after the event-loop unwind guard has
            // gone out of scope. Preserve the panic while releasing suppression.
            let _guard = RestoreUnwindGuard(&active);
            if let Err(error) = result {
                completion(Err(error));
                return;
            }
            let Some(target) = target.upgrade() else {
                completion(Err(super::super::NavigationError::Destroyed.into()));
                return;
            };
            let items = target.history();
            if active.closed.get() {
                return;
            }
            let items = match items {
                Ok(items) => items,
                Err(error) => {
                    completion(Err(error));
                    return;
                }
            };
            if items.len() != nodes.len() {
                completion(Err(HistoryError::Codec(
                    "restored route count mismatch".into(),
                )));
                return;
            }
            let children = items
                .into_iter()
                .zip(nodes)
                .filter_map(|(item, node)| item.child.map(|child| (child, node.children)))
                .collect();
            restore_children(binding, children, completion);
        }),
    );
}
fn restore_children(
    binding: std::rc::Weak<Binding>,
    mut children: VecDeque<(Rc<dyn WebNavigation>, Vec<HistoryNode>)>,
    completion: HistoryCompletion,
) {
    if binding.upgrade().is_none_or(|binding| binding.closed.get()) {
        return;
    }
    if let Some((child, nodes)) = children.pop_front() {
        let child_binding = binding.clone();
        restore(
            child_binding,
            child,
            nodes,
            Box::new(move |result| {
                if let Err(error) = result {
                    completion(Err(error));
                } else {
                    restore_children(binding, children, completion);
                }
            }),
        );
    } else {
        completion(Ok(()));
    }
}
