use super::NavigationError;
#[cfg(test)]
use crate::router::navigation::Event;
/// Configuration snapshots before and after successful stack navigation.
pub type NavigationResult<C> = crate::router::navigation::NavigationResult<Vec<Rc<C>>>;
use crate::router::navigation::NavigationSource;
#[cfg(test)]
use crate::value::Subscription;
use std::rc::Rc;

/// A shared, single-threaded source of stack navigation events.
/// Each connected stack applies events independently; reentrant events are FIFO.
pub struct StackNavigation<C> {
    pub(crate) source: NavigationSource<Vec<Rc<C>>>,
}
impl<C> Clone for StackNavigation<C> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
        }
    }
}
impl<C: 'static> Default for StackNavigation<C> {
    fn default() -> Self {
        Self::new()
    }
}
impl<C: 'static> StackNavigation<C> {
    /// Creates an empty navigation source, ready to connect to child stacks.
    pub fn new() -> Self {
        Self {
            source: NavigationSource::new(),
        }
    }
    /// Transforms the configurations of every connected stack.
    /// Panics if a connected stack rejects navigation; use navigate_with for typed errors.
    pub fn navigate(&self, transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        self.navigate_with(transformer, |result| {
            if let Err(error) = result {
                panic!("stack navigation failed: {error:?}");
            }
        });
    }
    /// Transforms each connected stack and reports its committed result after publication.
    /// Panics clear queued events and propagate; the source remains usable afterward.
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.source.navigate_with(transformer, completion);
    }
    #[cfg(test)]
    pub(crate) fn subscribe(&self, callback: impl Fn(Event<Vec<Rc<C>>>) + 'static) -> Subscription {
        self.source.subscribe(callback)
    }
    /// Appends a new top configuration.
    pub fn push(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            stack.push(configuration.clone());
            stack
        });
    }
    /// Removes the top child when the stack has more than one child.
    pub fn pop(&self) {
        self.navigate(|mut stack| {
            if stack.len() > 1 {
                stack.pop();
            }
            stack
        });
    }
    /// Removes matching top children, stopping before the root.
    pub fn pop_while(&self, predicate: impl Fn(&C) -> bool + 'static) {
        self.navigate(move |mut stack| {
            while stack.len() > 1 && predicate(stack.last().unwrap()) {
                stack.pop();
            }
            stack
        });
    }
    /// Makes the zero-based child at `index` active. Out-of-range indices
    /// clamp to the current top child.
    pub fn pop_to(&self, index: usize) {
        self.navigate(move |mut stack| {
            if !stack.is_empty() {
                let index = index.min(stack.len() - 1);
                stack.truncate(index + 1);
            }
            stack
        });
    }
    /// Removes every child above the root.
    pub fn pop_to_first(&self) {
        self.pop_to(0);
    }
    /// Replaces the current top configuration.
    pub fn replace_current(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            stack.pop();
            stack.push(configuration.clone());
            stack
        });
    }
    /// Replaces the entire stack. Connected stacks reject an empty result.
    pub fn replace_all(&self, configurations: Vec<C>) {
        let configurations: Vec<_> = configurations.into_iter().map(Rc::new).collect();
        self.navigate(move |_| configurations.clone());
    }
    /// Removes configurations whose selected kind matches the new
    /// configuration, then appends the new configuration.
    pub fn bring_to_front_by<K: Eq + 'static>(
        &self,
        configuration: C,
        selector: impl Fn(&C) -> K + 'static,
    ) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            let kind = selector(&configuration);
            stack.retain(|entry| selector(entry) != kind);
            stack.push(configuration.clone());
            stack
        });
    }
}
impl<C: Eq + 'static> StackNavigation<C> {
    /// Appends unless the current top configuration is equal.
    pub fn push_new(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            if stack.last().is_none_or(|top| top != &configuration) {
                stack.push(configuration.clone());
            }
            stack
        });
    }
    /// Removes the last equal configuration, if present, then appends it.
    pub fn push_to_front(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            if let Some(index) = stack.iter().rposition(|entry| entry == &configuration) {
                stack.remove(index);
            }
            stack.push(configuration.clone());
            stack
        });
    }
}
