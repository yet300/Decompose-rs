use super::{ActiveLifecycleState, ItemMap, Items, NavigationError};
use crate::router::navigation::NavigationSource;
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
    fn navigate(&self, transformer: impl Fn(Items<C>) -> Items<C> + 'static) {
        self.navigate_with(transformer, panic_completion)
    }
    /// Transforms ordered membership.
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
}
fn panic_completion<C>(result: Result<NavigationResult<C>, NavigationError>) {
    if let Err(e) = result {
        panic!("items navigation failed: {e:?}")
    }
}
/// Shared synchronous event source; events without receivers are discarded.
pub struct ItemsNavigation<C> {
    pub(crate) source: NavigationSource<Items<C>>,
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
            source: NavigationSource::new(),
        }
    }
}
impl<C: 'static> ItemsNavigator<C> for ItemsNavigation<C> {
    fn navigate_with(
        &self,
        transformer: impl Fn(Items<C>) -> Items<C> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.source.navigate_with(transformer, move |r| {
            completion(r.map(|r| NavigationResult {
                old_items: r.old_stack,
                new_items: r.new_stack,
            }))
        });
    }
}
macro_rules! inherent_navigation {
 () => {
 /// Applies a state transformer, panicking on errors.
 pub fn navigate(&self,transformer:impl Fn(Items<C>)->Items<C>+'static) {ItemsNavigator::navigate(self,transformer)}
 /// Applies a state transformer, completing after publication.
 pub fn navigate_with(&self,transformer:impl Fn(Items<C>)->Items<C>+'static,completion:impl Fn(Result<NavigationResult<C>,NavigationError>)+'static) {ItemsNavigator::navigate_with(self,transformer,completion)}
 /// Transforms ordered membership.
 pub fn set_items(&self,transformer:impl Fn(Vec<Rc<C>>)->Vec<Rc<C>>+'static) {ItemsNavigator::set_items(self,transformer)}
 /// Transforms membership and receives completion.
 pub fn set_items_with(&self,transformer:impl Fn(Vec<Rc<C>>)->Vec<Rc<C>>+'static,completion:impl Fn(Result<NavigationResult<C>,NavigationError>)+'static) {ItemsNavigator::set_items_with(self,transformer,completion)}
 /// Transforms requested active targets.
 pub fn set_active_items(&self,transformer:impl Fn(ItemMap<C,ActiveLifecycleState>)->ItemMap<C,ActiveLifecycleState>+'static) {ItemsNavigator::set_active_items(self,transformer)}
 /// Transforms requested targets and receives completion.
 pub fn set_active_items_with(&self,transformer:impl Fn(ItemMap<C,ActiveLifecycleState>)->ItemMap<C,ActiveLifecycleState>+'static,completion:impl Fn(Result<NavigationResult<C>,NavigationError>)+'static) {ItemsNavigator::set_active_items_with(self,transformer,completion)}
 };
}
pub(crate) use inherent_navigation;
impl<C: 'static> ItemsNavigation<C> {
    inherent_navigation!();
}
