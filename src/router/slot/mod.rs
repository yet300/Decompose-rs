//! An optional lifecycle-aware child component.
mod state;
pub use super::{Child, NavigationError};
use crate::{ComponentContext, value::Value};
pub use state::SlotCodec;
use std::rc::Rc;

/// Immutable view of an optional child. Snapshots retain its instance, not its lifecycle.
pub struct ChildSlot<C, T> {
    /// The active child, or None when dismissed.
    pub child: Option<Child<C, T>>,
}

/// Slot construction options. A codec enables persistent membership.
pub struct SlotOptions<C> {
    /// Unique state and retained-instance key within the hosting context.
    pub key: String,
    /// Dismiss on unhandled Back while populated.
    pub handle_back_button: bool,
    /// Explicit configuration codec; None selects transient navigation.
    pub codec: Option<SlotCodec<C>>,
}
impl<C> Default for SlotOptions<C> {
    fn default() -> Self {
        Self {
            key: "DefaultChildSlot".into(),
            handle_back_button: false,
            codec: None,
        }
    }
}

/// Configuration snapshots before and after successful slot navigation.
pub struct NavigationResult<C> {
    /// Configuration before navigation; None denotes an empty slot.
    pub old_configuration: Option<Rc<C>>,
    /// Configuration after navigation; None denotes an empty slot.
    pub new_configuration: Option<Rc<C>>,
}
impl<C> Clone for NavigationResult<C> {
    fn clone(&self) -> Self {
        Self {
            old_configuration: self.old_configuration.clone(),
            new_configuration: self.new_configuration.clone(),
        }
    }
}

/// Shared, single-threaded navigation source. Each connected slot reports its own result.
/// Reentrant events are delivered FIFO after the current broadcast completes.
pub struct SlotNavigation<C> {
    navigation: super::navigation::NavigationSource<Vec<Rc<C>>>,
}
impl<C> Clone for SlotNavigation<C> {
    fn clone(&self) -> Self {
        Self {
            navigation: self.navigation.clone(),
        }
    }
}
impl<C: 'static> Default for SlotNavigation<C> {
    fn default() -> Self {
        Self::new()
    }
}
impl<C: 'static> SlotNavigation<C> {
    /// Creates an empty source. Events without connected slots are discarded.
    pub fn new() -> Self {
        Self {
            navigation: super::navigation::NavigationSource::new(),
        }
    }
    /// Transforms every connected slot. Panics on transition errors.
    pub fn navigate(&self, transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static) {
        self.navigate_with(transformer, |result| {
            if let Err(error) = result {
                panic!("slot navigation failed: {error:?}");
            }
        });
    }
    /// Transforms each connected slot, reporting its committed result after publication.
    /// A panic clears queued events and propagates; the source remains usable.
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Option<Rc<C>>) -> Option<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.navigation.navigate_with(
            move |mut old| transformer(old.pop()).into_iter().collect(),
            move |result| {
                completion(result.map(|mut result| NavigationResult {
                    old_configuration: result.old_stack.pop(),
                    new_configuration: result.new_stack.pop(),
                }))
            },
        );
    }
    /// Activates a configuration; equality preserves the child identity and instance.
    pub fn activate(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |_| Some(configuration.clone()));
    }
    /// Activates a configuration and reports the old and new configurations.
    pub fn activate_with(
        &self,
        configuration: C,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        let configuration = Rc::new(configuration);
        self.navigate_with(move |_| Some(configuration.clone()), completion);
    }
    /// Dismisses the active child, if any.
    pub fn dismiss(&self) {
        self.navigate(|_| None);
    }
    /// Dismisses, reporting true if a child existed before this event.
    pub fn dismiss_with(&self, completion: impl Fn(Result<bool, NavigationError>) + 'static) {
        self.navigate_with(
            |_| None,
            move |result| completion(result.map(|result| result.old_configuration.is_some())),
        );
    }
}
struct SlotModel<C>(SlotOptions<C>);
impl<C: Eq + 'static, T: 'static> super::engine::Model<C, T> for SlotModel<C> {
    type State = Vec<Rc<C>>;
    fn configurations<'a>(&self, state: &'a Self::State) -> &'a [Rc<C>] {
        state
    }
    fn statuses(&self, state: &Self::State) -> Vec<super::pages::PageStatus> {
        vec![super::pages::PageStatus::Resumed; state.len()]
    }
    type Snapshot = ChildSlot<C, T>;
    fn persistent(&self) -> bool {
        self.0.codec.is_some()
    }
    fn validate(&self, configurations: &Vec<Rc<C>>) -> Result<(), NavigationError> {
        if configurations.len() > 1 {
            return Err(NavigationError::State(
                "slot requires zero or one configuration".into(),
            ));
        }
        Ok(())
    }
    fn snapshot(
        &self,
        children: Vec<super::pages::PageChild<C, T>>,
        _: &Self::State,
    ) -> Self::Snapshot {
        ChildSlot {
            child: children.into_iter().next().map(|child| match child {
                super::pages::PageChild::Created(c) => c,
                _ => unreachable!("slot children always live"),
            }),
        }
    }
    fn back_enabled(&self, state: &Self::State) -> bool {
        self.0.handle_back_button && state.len() == 1
    }
    fn back_transform(&self, _: Vec<Rc<C>>) -> Vec<Rc<C>> {
        Vec::new()
    }
    fn decode(
        &self,
        bytes: &[u8],
    ) -> Result<Option<super::engine::SavedState<Vec<Rc<C>>>>, NavigationError> {
        self.0
            .codec
            .as_ref()
            .map(|codec| state::decode(codec, bytes))
            .transpose()
    }
    fn encode(
        &self,
        configs: &Vec<Rc<C>>,
        children: &Vec<(String, std::collections::BTreeMap<String, Vec<u8>>)>,
        next_key: u64,
    ) -> Result<Option<Vec<u8>>, NavigationError> {
        self.0
            .codec
            .as_ref()
            .map(|codec| state::encode(codec, configs, children, next_key))
            .transpose()
    }
}

/// Builds an optional child with the hosting context's factory and services.
/// The hosting lifecycle owns navigation even when all observable views are dropped.
pub fn child_slot<Ctx: ComponentContext + 'static, C: Eq + 'static, T: 'static>(
    context: &Ctx,
    navigation: &SlotNavigation<C>,
    initial: Option<C>,
    options: SlotOptions<C>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<Value<ChildSlot<C, T>>, NavigationError> {
    super::engine::build(
        context,
        &navigation.navigation,
        initial.into_iter().map(Rc::new).collect(),
        options.key.clone(),
        SlotModel(options),
        factory,
    )
}
