use super::{ActiveLifecycleState, ItemMap, Items, NavigationError};
use crate::router::{
    NavigationEvent,
    children::{NavigationSource, SimpleNavigation},
};
use std::rc::Rc;
/// Receiver state before and after a committed event.
pub struct NavigationResult<C> {
    /// State before transformation.
    pub old_items: Items<C>,
    /// Normalized committed state.
    pub new_items: Items<C>,
}
impl<C> Clone for NavigationResult<C> {
    fn clone(&self) -> Self {
        Self {
            old_items: self.old_items.clone(),
            new_items: self.new_items.clone(),
        }
    }
}
/// Common shared-source and receiver-local navigation operations.
pub trait ItemsNavigator<C: 'static> {
    /// Applies a transformer, completing after publication.
    fn navigate_with(
        &self,
        transformer: impl Fn(Items<C>) -> Items<C> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    );
    /// Applies a transformer, panicking on errors.
    #[deprecated(note = "use try_navigate or navigate_or_panic")]
    #[track_caller]
    fn navigate(&self, transformer: impl Fn(Items<C>) -> Items<C> + 'static) {
        self.navigate_with(transformer, panic_completion)
    }
    /// Transforms ordered membership.
    #[deprecated(note = "use try_set_items or set_items_or_panic")]
    #[track_caller]
    fn set_items(&self, transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        self.set_items_with(transformer, panic_completion)
    }
    /// Transforms membership and reports the receiver result.
    fn set_items_with(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.navigate_with(
            move |mut state| {
                state.items = transformer(state.items);
                state
            },
            completion,
        )
    }
    /// Transforms requested active targets.
    #[deprecated(note = "use try_set_active_items or set_active_items_or_panic")]
    #[track_caller]
    fn set_active_items(
        &self,
        transformer: impl Fn(ItemMap<C, ActiveLifecycleState>) -> ItemMap<C, ActiveLifecycleState>
        + 'static,
    ) {
        self.set_active_items_with(transformer, panic_completion)
    }
    /// Transforms requested targets and reports the receiver result.
    fn set_active_items_with(
        &self,
        transformer: impl Fn(ItemMap<C, ActiveLifecycleState>) -> ItemMap<C, ActiveLifecycleState>
        + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.navigate_with(
            move |mut state| {
                state.active_items = transformer(state.active_items);
                state
            },
            completion,
        )
    }
    /// Fallible counterpart of `navigate`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_navigate(
        &self,
        transformer: impl Fn(Items<C>) -> Items<C> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| {
            self.navigate_with(transformer, completion)
        })
    }
    /// Fallible counterpart of `set_items`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_set_items(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| {
            self.set_items_with(transformer, completion)
        })
    }
    /// Fallible counterpart of `set_active_items`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_set_active_items(
        &self,
        transformer: impl Fn(ItemMap<C, ActiveLifecycleState>) -> ItemMap<C, ActiveLifecycleState>
        + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| {
            self.set_active_items_with(transformer, completion)
        })
    }
    /// Explicitly panicking compatibility operation. See `try_navigate` for errors.
    #[track_caller]
    fn navigate_or_panic(&self, transformer: impl Fn(Items<C>) -> Items<C> + 'static) {
        self.navigate_with(transformer, panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_set_items` for errors.
    #[track_caller]
    fn set_items_or_panic(&self, transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        self.set_items_with(transformer, panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_set_active_items` for errors.
    #[track_caller]
    fn set_active_items_or_panic(
        &self,
        transformer: impl Fn(ItemMap<C, ActiveLifecycleState>) -> ItemMap<C, ActiveLifecycleState>
        + 'static,
    ) {
        self.set_active_items_with(transformer, panic_completion)
    }
}
fn panic_completion<C>(result: Result<NavigationResult<C>, NavigationError>) {
    if let Err(e) = result {
        panic!("items navigation failed: {e:?}")
    }
}
/// Shared synchronous event source; events without receivers are discarded.
///
/// Events are shared by `Rc` and never replayed. Nested broadcasts are FIFO.
/// Panics abandon queued events and propagate; the source remains usable afterward.
pub struct ItemsNavigation<C> {
    source: SimpleNavigation<ItemsEvent<C>>,
}
impl<C> Clone for ItemsNavigation<C> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
        }
    }
}
impl<C: 'static> Default for ItemsNavigation<C> {
    fn default() -> Self {
        Self::new()
    }
}
impl<C: 'static> ItemsNavigation<C> {
    /// Creates an empty source.
    pub fn new() -> Self {
        Self {
            source: SimpleNavigation::new(),
        }
    }
}
impl<C: 'static> ItemsNavigator<C> for ItemsNavigation<C> {
    fn navigate_with(
        &self,
        transformer: impl Fn(Items<C>) -> Items<C> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        let event = ItemsEvent::with_completion(transformer, completion);
        let abandon = event.clone();
        self.source
            .navigate_rc_abandonable(Rc::new(event), move || {
                abandon.complete(Err(NavigationError::ChildUnavailable))
            });
    }
}
macro_rules! inherent_navigation {
 () => {
 /// Applies a state transformer, panicking on errors.
 #[deprecated(note = "use try_navigate or navigate_or_panic")]
 #[track_caller]
 pub fn navigate(&self,transformer:impl Fn(Items<C>)->Items<C>+'static) {ItemsNavigator::navigate(self,transformer)}
 /// Applies a state transformer, completing after publication.
 pub fn navigate_with(&self,transformer:impl Fn(Items<C>)->Items<C>+'static,completion:impl Fn(Result<NavigationResult<C>,NavigationError>)+'static) {ItemsNavigator::navigate_with(self,transformer,completion)}
 /// Transforms ordered membership.
 #[deprecated(note = "use try_set_items or set_items_or_panic")]
 #[track_caller]
 pub fn set_items(&self,transformer:impl Fn(Vec<Rc<C>>)->Vec<Rc<C>>+'static) {ItemsNavigator::set_items(self,transformer)}
 /// Transforms membership and receives completion.
 pub fn set_items_with(&self,transformer:impl Fn(Vec<Rc<C>>)->Vec<Rc<C>>+'static,completion:impl Fn(Result<NavigationResult<C>,NavigationError>)+'static) {ItemsNavigator::set_items_with(self,transformer,completion)}
 /// Transforms requested active targets.
 #[deprecated(note = "use try_set_active_items or set_active_items_or_panic")]
 #[track_caller]
 pub fn set_active_items(&self,transformer:impl Fn(ItemMap<C,ActiveLifecycleState>)->ItemMap<C,ActiveLifecycleState>+'static) {ItemsNavigator::set_active_items(self,transformer)}
 /// Transforms requested targets and receives completion.
 pub fn set_active_items_with(&self,transformer:impl Fn(ItemMap<C,ActiveLifecycleState>)->ItemMap<C,ActiveLifecycleState>+'static,completion:impl Fn(Result<NavigationResult<C>,NavigationError>)+'static) {ItemsNavigator::set_active_items_with(self,transformer,completion)}

    /// Fallible counterpart of `navigate`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_navigate(
        &self,
        transformer: impl Fn(Items<C>) -> Items<C> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        ItemsNavigator::try_navigate(self, transformer)
    }
    /// Fallible counterpart of `set_items`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_set_items(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        ItemsNavigator::try_set_items(self, transformer)
    }
    /// Fallible counterpart of `set_active_items`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_set_active_items(
        &self,
        transformer: impl Fn(ItemMap<C, ActiveLifecycleState>) -> ItemMap<C, ActiveLifecycleState>
        + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        ItemsNavigator::try_set_active_items(self, transformer)
    }
    /// Explicitly panicking compatibility operation. See `try_navigate` for errors.
    #[track_caller]
    pub fn navigate_or_panic(&self, transformer: impl Fn(Items<C>) -> Items<C> + 'static) {
        ItemsNavigator::navigate_or_panic(self, transformer)
    }
    /// Explicitly panicking compatibility operation. See `try_set_items` for errors.
    #[track_caller]
    pub fn set_items_or_panic(&self, transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        ItemsNavigator::set_items_or_panic(self, transformer)
    }
    /// Explicitly panicking compatibility operation. See `try_set_active_items` for errors.
    #[track_caller]
    pub fn set_active_items_or_panic(
        &self,
        transformer: impl Fn(ItemMap<C, ActiveLifecycleState>) -> ItemMap<C, ActiveLifecycleState>
        + 'static,
    ) {
        ItemsNavigator::set_active_items_or_panic(self, transformer)
    }
 };
}
pub(crate) use inherent_navigation;
impl<C: 'static> ItemsNavigation<C> {
    inherent_navigation!();
}

/// Public Items state transformation and per-receiver completion.
pub type ItemsEvent<C> = NavigationEvent<Items<C>, NavigationResult<C>>;
impl<C: 'static> NavigationSource<ItemsEvent<C>> for ItemsNavigation<C> {
    fn subscribe(&self, observer: Rc<dyn Fn(Rc<ItemsEvent<C>>)>) -> crate::value::Subscription {
        self.source.subscribe(observer)
    }
}
