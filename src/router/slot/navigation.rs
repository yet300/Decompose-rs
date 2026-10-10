use super::{NavigationError, NavigationResult};
use crate::router::{
    NavigationEvent,
    children::{NavigationSource, SimpleNavigation},
};
use std::rc::Rc;
/// Public slot state transformation and per-receiver completion.
pub type SlotEvent<C> = NavigationEvent<Option<Rc<C>>, NavigationResult<C>>;

/// Standard slot operations shared by sources and custom navigators.
///
/// Implement `navigate_with` to inherit the standard helper policies.
/// Generic callbacks use static dispatch; this trait is not object-safe.
pub trait SlotNavigator<C: 'static> {
    /// Transforms every connected slot. Panics on transition errors.
    fn navigate(&self, transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static) {
        self.navigate_with(transformer, |result| {
            if let Err(error) = result {
                panic!("slot navigation failed: {error:?}");
            }
        });
    }
    /// Transforms each connected slot, reporting its committed result after publication.
    fn navigate_with(
        &self,
        transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    );
    /// Activates a configuration; equality preserves the child identity and instance.
    fn activate(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |_| Some(configuration.clone()));
    }
    /// Activates a configuration and reports the old and new configurations.
    fn activate_with(
        &self,
        configuration: C,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        let configuration = Rc::new(configuration);
        self.navigate_with(move |_| Some(configuration.clone()), completion);
    }
    /// Dismisses the active child, if any.
    fn dismiss(&self) {
        self.navigate(|_| None);
    }
    /// Dismisses, reporting true if a child existed before this event.
    fn dismiss_with(&self, completion: impl Fn(Result<bool, NavigationError>) + 'static) {
        self.navigate_with(
            |_| None,
            move |result| completion(result.map(|result| result.old_configuration.is_some())),
        );
    }
}

/// Shared synchronous navigation; each receiver applies events to its own state.
///
/// Events are shared by `Rc`, never replayed, and discarded without receivers.
/// Nested broadcasts are FIFO. Panics abandon queued events and propagate;
/// the source remains usable afterward.
pub struct SlotNavigation<C> {
    source: SimpleNavigation<SlotEvent<C>>,
}
impl<C> Clone for SlotNavigation<C> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
        }
    }
}
impl<C: 'static> Default for SlotNavigation<C> {
    fn default() -> Self {
        Self::new()
    }
}
impl<C: 'static> SlotNavigation<C> {
    /// Creates an empty source. Events without receivers are discarded.
    pub fn new() -> Self {
        Self {
            source: SimpleNavigation::new(),
        }
    }
    /// Transforms every connected slot. Panics on transition errors.
    pub fn navigate(&self, transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static) {
        SlotNavigator::navigate(self, transformer)
    }
    /// Transforms each connected slot, reporting its committed result after publication.
    /// A panic clears queued events and propagates; the source remains usable.
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        SlotNavigator::navigate_with(self, transformer, completion)
    }
    /// Activates a configuration; equality preserves the child identity and instance.
    pub fn activate(&self, configuration: C) {
        SlotNavigator::activate(self, configuration)
    }
    /// Activates a configuration and reports the old and new configurations.
    pub fn activate_with(
        &self,
        configuration: C,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        SlotNavigator::activate_with(self, configuration, completion)
    }
    /// Dismisses the active child, if any.
    pub fn dismiss(&self) {
        SlotNavigator::dismiss(self)
    }
    /// Dismisses, reporting true if a child existed before this event.
    pub fn dismiss_with(&self, completion: impl Fn(Result<bool, NavigationError>) + 'static) {
        SlotNavigator::dismiss_with(self, completion)
    }
}
impl<C: 'static> SlotNavigator<C> for SlotNavigation<C> {
    fn navigate_with(
        &self,
        transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.source
            .navigate(SlotEvent::with_completion(transformer, completion));
    }
}
impl<C: 'static> NavigationSource<SlotEvent<C>> for SlotNavigation<C> {
    fn subscribe(&self, observer: Rc<dyn Fn(Rc<SlotEvent<C>>)>) -> crate::value::Subscription {
        self.source.subscribe(observer)
    }
}
