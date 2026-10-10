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
    /// Transforms each connected slot, reporting its committed result after publication.
    fn navigate_with(
        &self,
        transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    );
    /// Activates a configuration and reports the old and new configurations.
    fn activate_with(
        &self,
        configuration: C,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        let configuration = Rc::new(configuration);
        self.navigate_with(move |_| Some(configuration.clone()), completion);
    }
    /// Dismisses, reporting true if a child existed before this event.
    fn dismiss_with(&self, completion: impl Fn(Result<bool, NavigationError>) + 'static) {
        self.navigate_with(
            |_| None,
            move |result| completion(result.map(|result| result.old_configuration.is_some())),
        );
    }
    /// Fallible counterpart of `navigate`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_navigate(
        &self,
        transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| {
            self.navigate_with(transformer, completion)
        })
    }
    /// Fallible counterpart of `activate`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_activate(
        &self,
        configuration: C,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| {
            self.activate_with(configuration, completion)
        })
    }
    /// Fallible counterpart of `dismiss`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_dismiss(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        self.try_navigate(|_| None)
    }
    /// Explicitly panicking compatibility operation. See `try_navigate` for errors.
    #[track_caller]
    fn navigate_or_panic(&self, transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static) {
        self.navigate_with(transformer, |result| {
            if let Err(error) = result {
                panic!("slot navigation failed: {error:?}");
            }
        });
    }
    /// Explicitly panicking compatibility operation. See `try_activate` for errors.
    #[track_caller]
    fn activate_or_panic(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate_or_panic(move |_| Some(configuration.clone()));
    }
    /// Explicitly panicking compatibility operation. See `try_dismiss` for errors.
    #[track_caller]
    fn dismiss_or_panic(&self) {
        self.navigate_or_panic(|_| None);
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
    /// Transforms each connected slot, reporting its committed result after publication.
    /// A panic clears queued events and propagates; the source remains usable.
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        SlotNavigator::navigate_with(self, transformer, completion)
    }
    /// Activates a configuration and reports the old and new configurations.
    pub fn activate_with(
        &self,
        configuration: C,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        SlotNavigator::activate_with(self, configuration, completion)
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
        let event = SlotEvent::with_completion(transformer, completion);
        let abandon = event.clone();
        self.source
            .navigate_rc_abandonable(Rc::new(event), move || {
                abandon.complete(Err(NavigationError::ChildUnavailable))
            });
    }
}
impl<C: 'static> NavigationSource<SlotEvent<C>> for SlotNavigation<C> {
    fn subscribe(&self, observer: Rc<dyn Fn(Rc<SlotEvent<C>>)>) -> crate::value::Subscription {
        self.source.subscribe(observer)
    }
}

impl<C: 'static> SlotNavigation<C> {
    /// Fallible counterpart of `navigate`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_navigate(
        &self,
        transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        SlotNavigator::try_navigate(self, transformer)
    }
    /// Fallible counterpart of `activate`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_activate(
        &self,
        configuration: C,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        SlotNavigator::try_activate(self, configuration)
    }
    /// Fallible counterpart of `dismiss`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_dismiss(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        SlotNavigator::try_dismiss(self)
    }
    /// Explicitly panicking compatibility operation. See `try_navigate` for errors.
    #[track_caller]
    pub fn navigate_or_panic(
        &self,
        transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static,
    ) {
        SlotNavigator::navigate_or_panic(self, transformer)
    }
    /// Explicitly panicking compatibility operation. See `try_activate` for errors.
    #[track_caller]
    pub fn activate_or_panic(&self, configuration: C) {
        SlotNavigator::activate_or_panic(self, configuration)
    }
    /// Explicitly panicking compatibility operation. See `try_dismiss` for errors.
    #[track_caller]
    pub fn dismiss_or_panic(&self) {
        SlotNavigator::dismiss_or_panic(self)
    }
}
