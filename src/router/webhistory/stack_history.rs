//! Portable behavior of the older stack-only WebHistoryController.
use super::*;
use crate::router::stack::{ChildStack, StackNavigator};
use std::cell::Cell;

type Path<C> = dyn Fn(&C) -> String;
type Configuration<C> = dyn Fn(&str) -> Result<C, HistoryError>;
type Guard<C> = dyn Fn(&[Rc<C>], &[Rc<C>]) -> bool;

/// Stack-only history facade, retaining the older upstream controller semantics.
///
/// Unlike [`HistoryController::attach`], attachment creates an entry for every
/// initial configuration. Changed suffixes preserve the shared prefix, guards
/// receive the desired and current stacks, and Forward reconstructs missing
/// configurations from saved paths. Construct this facade before creating the
/// router to read [`Self::history_paths`] for an initial stack after host reload.
/// The navigator must have exactly one connected router.
pub struct StackHistoryController<C> {
    backend: Rc<dyn HistoryBackend>,
    codec: RouteCodec<C>,
    path: Rc<Path<C>>,
    configuration: Rc<Configuration<C>>,
}
impl<C: Eq + 'static> StackHistoryController<C> {
    /// Creates a facade without registering observers or modifying host history.
    pub fn new(
        backend: Rc<dyn HistoryBackend>,
        codec: RouteCodec<C>,
        get_path: impl Fn(&C) -> String + 'static,
        get_configuration: impl Fn(&str) -> Result<C, HistoryError> + 'static,
    ) -> Self {
        Self {
            backend,
            codec,
            path: Rc::new(get_path),
            configuration: Rc::new(get_configuration),
        }
    }
    /// Paths in the current managed entry, excluding the browser's Forward list.
    /// Returns an empty list when the host has no managed entry.
    pub fn history_paths(&self) -> Result<Vec<String>, HistoryError> {
        Ok(self
            .backend
            .current()
            .and_then(|entry| entry.state)
            .map_or_else(Vec::new, |state| {
                state.nodes.into_iter().map(|node| node.path).collect()
            }))
    }
    /// Attaches a stack and returns an observer-owning RAII binding.
    /// `on_web_navigation` receives decoded target configurations first, then
    /// current configurations, and may veto a browser-initiated transition.
    pub fn attach<T: 'static>(
        &self,
        navigator: impl StackNavigator<C> + 'static,
        stack: Value<ChildStack<C, T>>,
        on_web_navigation: impl Fn(&[Rc<C>], &[Rc<C>]) -> bool + 'static,
    ) -> Result<HistoryController, HistoryError> {
        let navigate = move |keys: Vec<Rc<C>>, done| {
            let done = super::adapters::completion_bridge(done);
            navigator.navigate_with(
                move |_| keys.clone(),
                move |result| done(result.map(|_| ())),
            );
        };
        HistoryController::attach_legacy(
            Rc::new(LegacyNavigation {
                stack,
                codec: self.codec.clone(),
                path: self.path.clone(),
                configuration: self.configuration.clone(),
                guard: Box::new(on_web_navigation),
                navigate: Box::new(navigate),
            }),
            self.backend.clone(),
        )
    }
}
struct LegacyNavigation<C, T> {
    stack: Value<ChildStack<C, T>>,
    codec: RouteCodec<C>,
    path: Rc<Path<C>>,
    configuration: Rc<Configuration<C>>,
    guard: Box<Guard<C>>,
    navigate: Box<Navigate<C>>,
}
impl<C: Eq + 'static, T: 'static> LegacyNavigation<C, T> {
    fn current(&self) -> Vec<Rc<C>> {
        self.stack
            .get()
            .items()
            .map(|child| child.configuration.clone())
            .collect()
    }
    fn decode(&self, nodes: &[HistoryNode]) -> Result<Vec<Rc<C>>, HistoryError> {
        nodes
            .iter()
            .map(|node| self.codec.decode(&node.key).map(Rc::new))
            .collect()
    }
}
impl<C: Eq + 'static, T: 'static> WebNavigation for LegacyNavigation<C, T> {
    fn history(&self) -> Result<Vec<HistoryItem>, HistoryError> {
        self.current()
            .into_iter()
            .map(|key| {
                Ok(HistoryItem {
                    key: self.codec.encode(&key)?,
                    path: (self.path)(&key),
                    parameters: Vec::new(),
                    child: None,
                })
            })
            .collect()
    }
    fn subscribe(&self, observer: Rc<dyn Fn()>) -> Subscription {
        let initial = Cell::new(true);
        self.stack.subscribe(move |_| {
            if !initial.replace(false) {
                observer()
            }
        })
    }
    fn before_navigate_to(&self, nodes: &[HistoryNode]) -> Result<bool, HistoryError> {
        Ok((self.guard)(&self.decode(nodes)?, &self.current()))
    }
    fn navigate(&self, keys: Vec<Vec<u8>>, done: HistoryCompletion) {
        match keys
            .iter()
            .map(|key| self.codec.decode(key).map(Rc::new))
            .collect()
        {
            Ok(keys) => (self.navigate)(keys, done),
            Err(error) => done(Err(error)),
        }
    }
    fn navigate_history(&self, nodes: Vec<HistoryNode>, done: HistoryCompletion) {
        let target = match self.decode(&nodes) {
            Ok(target) => target,
            Err(error) => {
                done(Err(error));
                return;
            }
        };
        let Some(active) = target.last() else {
            done(Err(HistoryError::EmptyHistory));
            return;
        };
        let mut current = self.current();
        if current.last() == Some(active) {
            done(Ok(()));
            return;
        }
        if let Some(index) = current
            .iter()
            .rposition(|configuration| configuration == active)
        {
            current.truncate(index + 1);
        } else {
            for node in nodes.into_iter().skip(current.len()) {
                match (self.configuration)(&node.path) {
                    Ok(configuration) => current.push(Rc::new(configuration)),
                    Err(error) => {
                        done(Err(error));
                        return;
                    }
                }
            }
        }
        (self.navigate)(current, done);
    }
}
