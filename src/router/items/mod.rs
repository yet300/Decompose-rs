//! Ordered unique membership with independently requested lifecycle targets and lazy activation.
mod map;
mod navigation;
mod state;
use super::engine::{Model, NavigationControl, SavedState};
use super::pages::{PageChild, PageStatus};
pub use super::{Child, NavigationError};
use crate::{
    ComponentContext,
    value::{Subscription, Value},
};
pub use map::{ConfigurationIndex, ItemMap};
pub use navigation::{ItemsEvent, ItemsNavigation, ItemsNavigator, NavigationResult};
pub use state::ItemsCodec;
use std::rc::Rc;
/// Requested child lifecycle, bounded by the hosting lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveLifecycleState {
    /// Created, with no child Back interception.
    Created,
    /// Started, with eligible child Back handling.
    Started,
    /// Resumed, bounded by the parent.
    Resumed,
}
impl ActiveLifecycleState {
    fn status(self) -> PageStatus {
        match self {
            Self::Created => PageStatus::Created,
            Self::Started => PageStatus::Started,
            Self::Resumed => PageStatus::Resumed,
        }
    }
}
/// Ordered membership and independently keyed active targets.
/// Default routing revalidates duplicates on every update. Indexed routing
/// explicitly requires stable equality identity while configurations are stored.
pub struct Items<C> {
    /// Unique ordered configurations, including inactive entries.
    pub items: Vec<Rc<C>>,
    /// Requested active targets; absent members are inactive.
    pub active_items: ItemMap<C, ActiveLifecycleState>,
}
impl<C> Default for Items<C> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            active_items: ItemMap::new(),
        }
    }
}
impl<C> Clone for Items<C> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            active_items: self.active_items.clone(),
        }
    }
}
impl<C: Eq> Items<C> {
    /// Creates inactive owned configurations and rejects equal duplicates.
    pub fn new(items: Vec<C>) -> Result<Self, NavigationError> {
        let state = Self {
            items: items.into_iter().map(Rc::new).collect(),
            active_items: ItemMap::new(),
        };
        state.validate()?;
        Ok(state)
    }
    /// Creates inactive configurations with indexed duplicate validation.
    /// Use the same equality-compatible index when constructing the router.
    pub fn new_indexed(
        items: Vec<C>,
        index: ConfigurationIndex<C>,
    ) -> Result<Self, NavigationError> {
        let items: Vec<_> = items.into_iter().map(Rc::new).collect();
        let mut seen = ItemMap::with_index(index.clone());
        for configuration in &items {
            if seen.insert(configuration.clone(), ()).is_some() {
                return Err(NavigationError::DuplicateConfiguration);
            }
        }
        Ok(Self {
            items,
            active_items: ItemMap::with_index(index),
        })
    }
    fn validate(&self) -> Result<(), NavigationError> {
        if self
            .items
            .iter()
            .enumerate()
            .any(|(i, c)| self.items[..i].contains(c))
        {
            Err(NavigationError::DuplicateConfiguration)
        } else {
            Ok(())
        }
    }
}
/// A live component and its requested target.
pub struct ActiveChild<C, T> {
    /// Created component with stable identity.
    pub child: Child<C, T>,
    /// Requested target, preserved when the parent bounds actual state.
    pub lifecycle_state: ActiveLifecycleState,
}
impl<C, T> Clone for ActiveChild<C, T> {
    fn clone(&self) -> Self {
        Self {
            child: self.child.clone(),
            lifecycle_state: self.lifecycle_state,
        }
    }
}
/// Immutable ordered membership and its live children.
pub struct ChildItems<C, T> {
    /// Ordered canonical configuration handles.
    pub items: Vec<Rc<C>>,
    /// Live active components keyed by canonical membership handles.
    pub active_items: ItemMap<C, ActiveChild<C, T>>,
}
impl<C, T> Clone for ChildItems<C, T> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            active_items: self.active_items.clone(),
        }
    }
}
impl<C, T> Default for ChildItems<C, T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            active_items: ItemMap::new(),
        }
    }
}
/// Hosting options; membership is transient unless a codec is supplied.
pub struct ItemsOptions<C> {
    /// Unique saved-state provider and retained-instance key.
    pub key: String,
    /// Explicit ordered-list configuration serialization.
    pub codec: Option<ItemsCodec<C>>,
}
impl<C> Default for ItemsOptions<C> {
    fn default() -> Self {
        Self {
            key: "DefaultChildItems".into(),
            codec: None,
        }
    }
}
struct ItemsModel<C>(ItemsOptions<C>, Option<ConfigurationIndex<C>>);
impl<C: Eq + 'static, T: 'static> Model<C, T> for ItemsModel<C> {
    type State = Items<C>;
    type Snapshot = ChildItems<C, T>;
    fn memory_kind(&self) -> &'static str {
        "items"
    }
    fn persistent(&self) -> bool {
        self.0.codec.is_some()
    }
    fn configurations(&self, s: &Self::State) -> Vec<Rc<C>> {
        s.items.clone()
    }
    fn canonicalize_reused_configurations(&self) -> bool {
        true
    }
    fn normalize(&self, mut s: Self::State) -> Self::State {
        s.active_items.use_index(self.1.clone());
        let entries = s
            .items
            .iter()
            .filter_map(|c| s.active_items.get(c).map(|target| (c.clone(), *target)))
            .collect();
        s.active_items = ItemMap::from_unique(entries, self.1.clone());
        s
    }
    fn validate(&self, s: &Self::State) -> Result<(), NavigationError> {
        if let Some(index) = &self.1 {
            let mut seen = ItemMap::with_index(index.clone());
            for configuration in &s.items {
                if seen.insert(configuration.clone(), ()).is_some() {
                    return Err(NavigationError::DuplicateConfiguration);
                }
            }
            Ok(())
        } else {
            s.validate()
        }
    }
    fn validate_transition(
        &self,
        state: &Self::State,
        old: Option<&Self::State>,
    ) -> Result<(), NavigationError> {
        // Reuse the committed uniqueness invariant only after comparing values;
        // freshly allocated equal configurations qualify as well as shared handles.
        if self.1.is_some()
            && old.is_some_and(|old| {
                old.items.len() == state.items.len()
                    && old
                        .items
                        .iter()
                        .zip(&state.items)
                        .all(|(old, new)| old.as_ref() == new.as_ref())
            })
        {
            Ok(())
        } else {
            <Self as Model<C, T>>::validate(self, state)
        }
    }
    fn unique_configurations(&self) -> bool {
        // Positional matching may bypass an earlier equal candidate only when
        // the caller explicitly promises stable identity via the index policy.
        self.1.is_some()
    }
    fn configuration_fingerprint(&self, configuration: &C) -> Option<u64> {
        self.1
            .as_ref()
            .map(|index| index.fingerprint(configuration))
    }
    fn statuses(&self, s: &Self::State) -> Vec<PageStatus> {
        s.items
            .iter()
            .map(|c| {
                s.active_items
                    .get(c)
                    .map_or(PageStatus::Destroyed, |v| v.status())
            })
            .collect()
    }
    fn snapshot(&self, children: Vec<PageChild<C, T>>, s: &Self::State) -> Self::Snapshot {
        let active_items = children
            .into_iter()
            .zip(&s.items)
            .filter_map(|(child, c)| match child {
                PageChild::Created(mut child) => {
                    child.configuration = c.clone();
                    Some((
                        c.clone(),
                        ActiveChild {
                            child,
                            lifecycle_state: *s.active_items.get(c).expect("live target"),
                        },
                    ))
                }
                PageChild::Destroyed { .. } => None,
            })
            .collect();
        ChildItems {
            items: s.items.clone(),
            active_items: ItemMap::from_unique(active_items, self.1.clone()),
        }
    }
    fn back_enabled(&self, _: &Self::State) -> bool {
        false
    }
    fn back_transform(&self, s: Self::State) -> Self::State {
        s
    }
    fn decode(&self, b: &[u8]) -> Result<Option<SavedState<Self::State>>, NavigationError> {
        self.0
            .codec
            .as_ref()
            .map(|c| state::decode(c, b))
            .transpose()
    }
    fn should_restore(&self, state: &Self::State) -> bool {
        self.0
            .codec
            .as_ref()
            .is_none_or(|codec| codec.can_restore(state))
    }
    fn encode_with_children(
        &self,
        state: &Self::State,
        next_key: u64,
        children: impl FnOnce() -> Result<
            Vec<(String, std::collections::BTreeMap<String, Vec<u8>>)>,
            NavigationError,
        >,
    ) -> Result<Option<Vec<u8>>, NavigationError> {
        let Some(codec) = &self.0.codec else {
            return Ok(None);
        };
        if !codec.can_save(state) {
            return Ok(None);
        }
        state::encode(codec, state, &children()?, next_key).map(Some)
    }
    fn encode(
        &self,
        s: &Self::State,
        children: &Vec<(String, std::collections::BTreeMap<String, Vec<u8>>)>,
        next_key: u64,
    ) -> Result<Option<Vec<u8>>, NavigationError> {
        self.0
            .codec
            .as_ref()
            .map(|c| state::encode(c, s, children, next_key))
            .transpose()
    }
}
/// Receiver-local observable navigation and synchronous lazy activation.
pub struct LazyChildItems<C, T> {
    value: Value<ChildItems<C, T>>,
    control: NavigationControl<Items<C>>,
}
impl<C, T> Clone for LazyChildItems<C, T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            control: self.control.clone(),
        }
    }
}
impl<C: Eq + 'static, T: 'static> LazyChildItems<C, T> {
    /// Returns the latest snapshot.
    pub fn get(&self) -> Rc<ChildItems<C, T>> {
        self.value.get()
    }
    /// Returns the shared observable value.
    pub fn value(&self) -> Value<ChildItems<C, T>> {
        self.value.clone()
    }
    /// Subscribes to the shared value, synchronously replaying current membership.
    pub fn subscribe(&self, observer: impl Fn(Rc<ChildItems<C, T>>) + 'static) -> Subscription {
        self.value.subscribe(observer)
    }
    /// Returns an active component or synchronously activates an inactive member as Created.
    ///
    /// Active reads preserve the requested target and are allowed during navigation.
    /// Absent membership returns [`NavigationError::UnknownConfiguration`]. An inactive
    /// read during navigation or Back dispatch returns [`NavigationError::NavigationInProgress`]
    /// without queuing activation. If callbacks remove or deactivate the child before
    /// activation finishes, returns [`NavigationError::ChildUnavailable`]. This also
    /// applies to ended child scopes still present in an older snapshot, or when
    /// the child instance keeper is exclusively borrowed and liveness cannot be checked.
    /// Ended hosting
    /// scopes return [`NavigationError::Destroyed`], including for old live snapshots.
    pub fn get_or_create(&self, configuration: &C) -> Result<Rc<T>, NavigationError> {
        self.control.ensure_alive()?;
        let snapshot = self.get();
        if let Some(c) = snapshot.active_items.get(configuration) {
            self.control.ensure_child_available(&c.child.key)?;
            return Ok(c.child.instance.clone());
        }
        let key = snapshot
            .items
            .iter()
            .find(|c| c.as_ref() == configuration)
            .cloned()
            .ok_or(NavigationError::UnknownConfiguration)?;
        self.control.dispatch_sync(super::navigation::Event {
            transformer: Rc::new(move |mut s| {
                s.active_items
                    .insert(key.clone(), ActiveLifecycleState::Created);
                s
            }),
            completion: Rc::new(|_| {}),
        })?;
        self.control.ensure_alive()?;
        let snapshot = self.get();
        let child = snapshot
            .active_items
            .get(configuration)
            .ok_or(NavigationError::ChildUnavailable)?;
        self.control.ensure_child_available(&child.child.key)?;
        Ok(child.child.instance.clone())
    }
    /// See [`ItemsNavigator::navigate_with`].
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Items<C>) -> Items<C> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        ItemsNavigator::navigate_with(self, transformer, completion)
    }
}
impl<C: Eq + 'static, T: 'static> ItemsNavigator<C> for LazyChildItems<C, T> {
    fn navigate_with(
        &self,
        transformer: impl Fn(Items<C>) -> Items<C> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.control.dispatch(super::navigation::Event {
            transformer: Rc::new(transformer),
            completion: Rc::new(move |r| {
                completion(r.map(|r| NavigationResult {
                    old_items: r.old_stack,
                    new_items: r.new_stack,
                }))
            }),
        })
    }
}
/// Builds unique ordered children using the hosting context's factory and services.
pub fn child_items<Ctx: ComponentContext + 'static, C: Eq + 'static, T: 'static>(
    context: &Ctx,
    navigation: &(impl super::children::NavigationSource<ItemsEvent<C>> + ?Sized),
    initial: Items<C>,
    options: ItemsOptions<C>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<LazyChildItems<C, T>, NavigationError> {
    child_items_with_initial(context, navigation, || initial, options, factory)
}

/// Builds children, evaluating `initial` once only when no saved state is restored.
/// The initializer may borrow local data and is dropped during construction.
pub fn child_items_with_initial<Ctx: ComponentContext + 'static, C: Eq + 'static, T: 'static>(
    context: &Ctx,
    navigation: &(impl super::children::NavigationSource<ItemsEvent<C>> + ?Sized),
    initial: impl FnOnce() -> Items<C>,
    options: ItemsOptions<C>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<LazyChildItems<C, T>, NavigationError> {
    build_items(context, navigation, initial, options, None, factory)
}

/// Builds children with indexed duplicate validation, maps and engine matching.
/// Equal configurations must share a stable fingerprint; collisions use `Eq`.
pub fn child_items_indexed<Ctx: ComponentContext + 'static, C: Eq + 'static, T: 'static>(
    context: &Ctx,
    navigation: &(impl super::children::NavigationSource<ItemsEvent<C>> + ?Sized),
    initial: Items<C>,
    options: ItemsOptions<C>,
    index: ConfigurationIndex<C>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<LazyChildItems<C, T>, NavigationError> {
    child_items_with_initial_indexed(context, navigation, || initial, options, index, factory)
}

/// Indexed counterpart of [`child_items_with_initial`], preserving lazy initialization.
pub fn child_items_with_initial_indexed<
    Ctx: ComponentContext + 'static,
    C: Eq + 'static,
    T: 'static,
>(
    context: &Ctx,
    navigation: &(impl super::children::NavigationSource<ItemsEvent<C>> + ?Sized),
    initial: impl FnOnce() -> Items<C>,
    options: ItemsOptions<C>,
    index: ConfigurationIndex<C>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<LazyChildItems<C, T>, NavigationError> {
    build_items(context, navigation, initial, options, Some(index), factory)
}

fn build_items<Ctx: ComponentContext + 'static, C: Eq + 'static, T: 'static>(
    context: &Ctx,
    navigation: &(impl super::children::NavigationSource<ItemsEvent<C>> + ?Sized),
    initial: impl FnOnce() -> Items<C>,
    options: ItemsOptions<C>,
    index: Option<ConfigurationIndex<C>>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<LazyChildItems<C, T>, NavigationError> {
    let (value, control) = super::engine::build_with_subscriber(
        context,
        |receiver| {
            super::navigation::subscribe(
                navigation,
                receiver,
                std::convert::identity,
                std::convert::identity,
                |result| NavigationResult {
                    old_items: result.old_stack,
                    new_items: result.new_stack,
                },
            )
        },
        initial,
        options.key.clone(),
        ItemsModel(options, index),
        factory,
    )?;
    Ok(LazyChildItems { value, control })
}
