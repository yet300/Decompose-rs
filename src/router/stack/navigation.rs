use super::NavigationError;
/// Configuration snapshots before and after successful stack navigation.
pub type NavigationResult<C> = crate::router::navigation::NavigationResult<Vec<Rc<C>>>;
use crate::router::{
    NavigationEvent,
    children::{NavigationSource, SimpleNavigation},
};
use std::rc::Rc;

/// Public stack state transformation and per-receiver completion.
pub type StackEvent<C> = NavigationEvent<Vec<Rc<C>>, NavigationResult<C>>;

/// Standard stack operations shared by sources and custom navigators.
///
/// Implement `navigate_with` to inherit the standard helper policies.
/// Generic callbacks use static dispatch; this trait is not object-safe.
pub trait StackNavigator<C: 'static> {
    /// Transforms the configurations of every connected stack.
    /// Panics if a connected stack rejects navigation; use navigate_with for typed errors.
    fn navigate(&self, transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        self.navigate_with(transformer, |result| {
            if let Err(error) = result {
                panic!("stack navigation failed: {error:?}");
            }
        });
    }
    /// Transforms each connected stack and reports its committed result after publication.
    fn navigate_with(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    );
    /// Appends a new top configuration.
    fn push(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            stack.push(configuration.clone());
            stack
        });
    }
    /// Removes the top child when the stack has more than one child.
    fn pop(&self) {
        self.navigate(|mut stack| {
            if stack.len() > 1 {
                stack.pop();
            }
            stack
        });
    }
    /// Removes matching top children, stopping before the root.
    fn pop_while(&self, predicate: impl Fn(&C) -> bool + 'static) {
        self.navigate(move |mut stack| {
            while stack.len() > 1 && predicate(stack.last().unwrap()) {
                stack.pop();
            }
            stack
        });
    }
    /// Makes the zero-based child at `index` active. Out-of-range indices
    /// clamp to the current top child.
    fn pop_to(&self, index: usize) {
        self.navigate(move |mut stack| {
            if !stack.is_empty() {
                let index = index.min(stack.len() - 1);
                stack.truncate(index + 1);
            }
            stack
        });
    }
    /// Removes every child above the root.
    fn pop_to_first(&self) {
        self.pop_to(0);
    }
    /// Replaces the current top configuration.
    fn replace_current(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            stack.pop();
            stack.push(configuration.clone());
            stack
        });
    }
    /// Replaces the entire stack. Connected stacks reject an empty result.
    fn replace_all(&self, configurations: Vec<C>) {
        let configurations: Vec<_> = configurations.into_iter().map(Rc::new).collect();
        self.navigate(move |_| configurations.clone());
    }
    /// Removes configurations whose selected kind matches the new
    /// configuration, then appends the new configuration.
    fn bring_to_front_by<K: Eq + 'static>(
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
    /// Appends unless the current top configuration is equal.
    fn push_new(&self, configuration: C)
    where
        C: Eq,
    {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            if stack.last().is_none_or(|top| top != &configuration) {
                stack.push(configuration.clone());
            }
            stack
        });
    }
    /// Removes the last equal configuration, if present, then appends it.
    fn push_to_front(&self, configuration: C)
    where
        C: Eq,
    {
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

/// Shared synchronous navigation; each receiver applies events to its own state.
///
/// Events are shared by `Rc`, never replayed, and discarded without receivers.
/// Nested broadcasts are FIFO. Panics abandon queued events and propagate;
/// the source remains usable afterward.
pub struct StackNavigation<C> {
    source: SimpleNavigation<StackEvent<C>>,
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
    /// Creates an empty source. Events without receivers are discarded.
    pub fn new() -> Self {
        Self {
            source: SimpleNavigation::new(),
        }
    }
    /// Transforms the configurations of every connected stack.
    /// Panics if a connected stack rejects navigation; use navigate_with for typed errors.
    pub fn navigate(&self, transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        StackNavigator::navigate(self, transformer)
    }
    /// Transforms each connected stack and reports its committed result after publication.
    /// Panics clear queued events and propagate; the source remains usable afterward.
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        StackNavigator::navigate_with(self, transformer, completion)
    }
    /// Appends a new top configuration.
    pub fn push(&self, configuration: C) {
        StackNavigator::push(self, configuration)
    }
    /// Removes the top child when the stack has more than one child.
    pub fn pop(&self) {
        StackNavigator::pop(self)
    }
    /// Removes matching top children, stopping before the root.
    pub fn pop_while(&self, predicate: impl Fn(&C) -> bool + 'static) {
        StackNavigator::pop_while(self, predicate)
    }
    /// Makes the zero-based child at `index` active. Out-of-range indices
    /// clamp to the current top child.
    pub fn pop_to(&self, index: usize) {
        StackNavigator::pop_to(self, index)
    }
    /// Removes every child above the root.
    pub fn pop_to_first(&self) {
        StackNavigator::pop_to_first(self)
    }
    /// Replaces the current top configuration.
    pub fn replace_current(&self, configuration: C) {
        StackNavigator::replace_current(self, configuration)
    }
    /// Replaces the entire stack. Connected stacks reject an empty result.
    pub fn replace_all(&self, configurations: Vec<C>) {
        StackNavigator::replace_all(self, configurations)
    }
    /// Removes configurations whose selected kind matches the new
    /// configuration, then appends the new configuration.
    pub fn bring_to_front_by<K: Eq + 'static>(
        &self,
        configuration: C,
        selector: impl Fn(&C) -> K + 'static,
    ) {
        StackNavigator::bring_to_front_by(self, configuration, selector)
    }
    /// Appends unless the current top configuration is equal.
    pub fn push_new(&self, configuration: C)
    where
        C: Eq,
    {
        StackNavigator::push_new(self, configuration)
    }
    /// Removes the last equal configuration, if present, then appends it.
    pub fn push_to_front(&self, configuration: C)
    where
        C: Eq,
    {
        StackNavigator::push_to_front(self, configuration)
    }
}
impl<C: 'static> StackNavigator<C> for StackNavigation<C> {
    fn navigate_with(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.source
            .navigate(StackEvent::with_completion(transformer, completion));
    }
}
impl<C: 'static> NavigationSource<StackEvent<C>> for StackNavigation<C> {
    fn subscribe(&self, observer: Rc<dyn Fn(Rc<StackEvent<C>>)>) -> crate::value::Subscription {
        self.source.subscribe(observer)
    }
}
#[cfg(test)]
impl<C: 'static> StackNavigation<C> {
    pub(crate) fn subscribe(
        &self,
        callback: impl Fn(crate::router::navigation::Event<Vec<Rc<C>>>) + 'static,
    ) -> crate::value::Subscription {
        crate::router::navigation::subscribe(
            self,
            Rc::new(callback),
            std::convert::identity,
            std::convert::identity,
            std::convert::identity,
        )
    }
}
