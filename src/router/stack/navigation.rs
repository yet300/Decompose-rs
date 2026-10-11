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
    /// Deprecated alias, use `try_bring_to_front_by`.
    /// Removes all selected-kind matches and appends the provided configuration.
    #[doc(hidden)]
    fn try_replace_kind_and_push<K: Eq>(
        &self,
        configuration: C,
        selector: impl Fn(&C) -> K + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        self.try_bring_to_front_by(configuration, selector)
    }
    /// Transforms each connected stack and reports its committed result after publication.
    fn navigate_with(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    );
    /// Fallible counterpart of `navigate`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_navigate(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| {
            self.navigate_with(transformer, completion)
        })
    }
    /// Fallible counterpart of `push`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_push(
        &self,
        configuration: C,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        let configuration = Rc::new(configuration);
        self.try_navigate(move |mut stack| {
            stack.push(configuration.clone());
            stack
        })
    }
    /// Fallible counterpart of `pop`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_pop(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        self.try_navigate(|mut stack| {
            if stack.len() > 1 {
                stack.pop();
            }
            stack
        })
    }
    /// Fallible counterpart of `pop_while`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_pop_while(
        &self,
        predicate: impl Fn(&C) -> bool + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        self.try_navigate(move |mut stack| {
            while stack.len() > 1 && stack.last().is_some_and(|top| predicate(top)) {
                stack.pop();
            }
            stack
        })
    }
    /// Fallible counterpart of `pop_to`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_pop_to(
        &self,
        index: usize,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        self.try_navigate(move |mut stack| {
            if !stack.is_empty() {
                let index = index.min(stack.len() - 1);
                stack.truncate(index + 1);
            }
            stack
        })
    }
    /// Fallible counterpart of `pop_to_first`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_pop_to_first(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        self.try_pop_to(0)
    }
    /// Fallible counterpart of `replace_current`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_replace_current(
        &self,
        configuration: C,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        let configuration = Rc::new(configuration);
        self.try_navigate(move |mut stack| {
            stack.pop();
            stack.push(configuration.clone());
            stack
        })
    }
    /// Fallible counterpart of `replace_all`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_replace_all(
        &self,
        configurations: Vec<C>,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        let configurations: Vec<_> = configurations.into_iter().map(Rc::new).collect();
        self.try_navigate(move |_| configurations.clone())
    }
    /// Removes all configurations matching the selected kind and appends the provided one.
    /// Equal surviving configurations retain their child identity.
    /// Fallible counterpart of `bring_to_front_by`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_bring_to_front_by<K: Eq>(
        &self,
        configuration: C,
        selector: impl Fn(&C) -> K + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        let configuration = Rc::new(configuration);
        self.try_navigate(move |mut stack| {
            let kind = selector(&configuration);
            stack.retain(|entry| selector(entry) != kind);
            stack.push(configuration.clone());
            stack
        })
    }
    /// Fallible counterpart of `push_new`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_push_new(
        &self,
        configuration: C,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError>
    where
        C: Eq,
    {
        let configuration = Rc::new(configuration);
        self.try_navigate(move |mut stack| {
            if stack.last().is_none_or(|top| top != &configuration) {
                stack.push(configuration.clone());
            }
            stack
        })
    }
    /// Fallible counterpart of `push_to_front`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_push_to_front(
        &self,
        configuration: C,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError>
    where
        C: Eq,
    {
        let configuration = Rc::new(configuration);
        self.try_navigate(move |mut stack| {
            if let Some(index) = stack.iter().rposition(|entry| entry == &configuration) {
                stack.remove(index);
            }
            stack.push(configuration.clone());
            stack
        })
    }
    /// Explicitly panicking compatibility operation. See `try_navigate` for errors.
    #[track_caller]
    fn navigate_or_panic(&self, transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        self.navigate_with(transformer, |result| {
            if let Err(error) = result {
                panic!("stack navigation failed: {error:?}");
            }
        });
    }
    /// Explicitly panicking compatibility operation. See `try_push` for errors.
    #[track_caller]
    fn push_or_panic(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate_or_panic(move |mut stack| {
            stack.push(configuration.clone());
            stack
        });
    }
    /// Explicitly panicking compatibility operation. See `try_pop` for errors.
    #[track_caller]
    fn pop_or_panic(&self) {
        self.navigate_or_panic(|mut stack| {
            if stack.len() > 1 {
                stack.pop();
            }
            stack
        });
    }
    /// Explicitly panicking compatibility operation. See `try_pop_while` for errors.
    #[track_caller]
    fn pop_while_or_panic(&self, predicate: impl Fn(&C) -> bool + 'static) {
        self.navigate_or_panic(move |mut stack| {
            while stack.len() > 1 && stack.last().is_some_and(|top| predicate(top)) {
                stack.pop();
            }
            stack
        });
    }
    /// Explicitly panicking compatibility operation. See `try_pop_to` for errors.
    #[track_caller]
    fn pop_to_or_panic(&self, index: usize) {
        self.navigate_or_panic(move |mut stack| {
            if !stack.is_empty() {
                let index = index.min(stack.len() - 1);
                stack.truncate(index + 1);
            }
            stack
        });
    }
    /// Explicitly panicking compatibility operation. See `try_pop_to_first` for errors.
    #[track_caller]
    fn pop_to_first_or_panic(&self) {
        self.pop_to_or_panic(0);
    }
    /// Explicitly panicking compatibility operation. See `try_replace_current` for errors.
    #[track_caller]
    fn replace_current_or_panic(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate_or_panic(move |mut stack| {
            stack.pop();
            stack.push(configuration.clone());
            stack
        });
    }
    /// Explicitly panicking compatibility operation. See `try_replace_all` for errors.
    #[track_caller]
    fn replace_all_or_panic(&self, configurations: Vec<C>) {
        let configurations: Vec<_> = configurations.into_iter().map(Rc::new).collect();
        self.navigate_or_panic(move |_| configurations.clone());
    }
    /// Explicitly panicking compatibility operation. See `try_bring_to_front_by` for errors.
    #[track_caller]
    fn bring_to_front_by_or_panic<K: Eq>(
        &self,
        configuration: C,
        selector: impl Fn(&C) -> K + 'static,
    ) {
        let configuration = Rc::new(configuration);
        self.navigate_or_panic(move |mut stack| {
            let kind = selector(&configuration);
            stack.retain(|entry| selector(entry) != kind);
            stack.push(configuration.clone());
            stack
        });
    }
    /// Explicitly panicking compatibility operation. See `try_push_new` for errors.
    #[track_caller]
    fn push_new_or_panic(&self, configuration: C)
    where
        C: Eq,
    {
        let configuration = Rc::new(configuration);
        self.navigate_or_panic(move |mut stack| {
            if stack.last().is_none_or(|top| top != &configuration) {
                stack.push(configuration.clone());
            }
            stack
        });
    }
    /// Explicitly panicking compatibility operation. See `try_push_to_front` for errors.
    #[track_caller]
    fn push_to_front_or_panic(&self, configuration: C)
    where
        C: Eq,
    {
        let configuration = Rc::new(configuration);
        self.navigate_or_panic(move |mut stack| {
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
    /// See [`StackNavigator::navigate_with`].
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        StackNavigator::navigate_with(self, transformer, completion)
    }
}
impl<C: 'static> StackNavigator<C> for StackNavigation<C> {
    fn navigate_with(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        let event = StackEvent::with_completion(transformer, completion);
        let abandon = event.clone();
        self.source
            .navigate_rc_abandonable(Rc::new(event), move || {
                abandon.complete(Err(NavigationError::ChildUnavailable))
            });
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
