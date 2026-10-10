//! An optional lifecycle-aware child component.
mod navigation;
mod state;
pub use super::{Child, NavigationError};
use crate::{ComponentContext, value::Value};
pub use navigation::{SlotEvent, SlotNavigation, SlotNavigator};
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

struct SlotModel<C>(SlotOptions<C>);
impl<C: Eq + 'static, T: 'static> super::engine::Model<C, T> for SlotModel<C> {
    type State = Vec<Rc<C>>;
    fn configurations(&self, state: &Self::State) -> Vec<Rc<C>> {
        state.clone()
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
    fn should_restore(&self, state: &Self::State) -> bool {
        self.0
            .codec
            .as_ref()
            .is_none_or(|codec| codec.can_restore(state.first().map(Rc::as_ref)))
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
        if !codec.can_save(state.first().map(Rc::as_ref)) {
            return Ok(None);
        }
        state::encode(codec, state, &children()?, next_key).map(Some)
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
    navigation: &(impl super::children::NavigationSource<SlotEvent<C>> + ?Sized),
    initial: Option<C>,
    options: SlotOptions<C>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<Value<ChildSlot<C, T>>, NavigationError> {
    child_slot_with_initial(context, navigation, || initial, options, factory)
}

/// Builds children, evaluating `initial` once only when no saved state is restored.
/// The initializer may borrow local data and is dropped during construction.
pub fn child_slot_with_initial<Ctx: ComponentContext + 'static, C: Eq + 'static, T: 'static>(
    context: &Ctx,
    navigation: &(impl super::children::NavigationSource<SlotEvent<C>> + ?Sized),
    initial: impl FnOnce() -> Option<C>,
    options: SlotOptions<C>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<Value<ChildSlot<C, T>>, NavigationError> {
    super::engine::build_with_subscriber(
        context,
        |receiver| {
            super::navigation::subscribe(
                navigation,
                receiver,
                |mut state: Vec<Rc<C>>| state.pop(),
                |state| state.into_iter().collect(),
                |mut result| NavigationResult {
                    old_configuration: result.old_stack.pop(),
                    new_configuration: result.new_stack.pop(),
                },
            )
        },
        || initial().into_iter().map(Rc::new).collect(),
        options.key.clone(),
        SlotModel(options),
        factory,
    )
    .map(|(value, _)| value)
}
