use super::{ChildStack, NavigationError, StackEvent, StackOptions};
use crate::{
    ComponentContext,
    router::{
        Child,
        engine::{Model, SavedState},
    },
    value::Value,
};
use std::rc::Rc;
struct StackModel<C>(StackOptions<C>);
impl<C: Eq + 'static, T: 'static> Model<C, T> for StackModel<C> {
    type State = Vec<Rc<C>>;
    fn configurations(&self, state: &Self::State) -> Vec<Rc<C>> {
        state.clone()
    }
    fn statuses(&self, state: &Self::State) -> Vec<crate::router::pages::PageStatus> {
        state
            .iter()
            .enumerate()
            .map(|(i, _)| {
                if i + 1 == state.len() {
                    crate::router::pages::PageStatus::Resumed
                } else {
                    crate::router::pages::PageStatus::Created
                }
            })
            .collect()
    }
    type Snapshot = ChildStack<C, T>;
    fn persistent(&self) -> bool {
        self.0.codec.is_some()
    }
    fn validate(&self, configs: &Vec<Rc<C>>) -> Result<(), NavigationError> {
        if configs.is_empty() {
            return Err(NavigationError::EmptyStack);
        }
        if !self.0.allow_duplicates
            && configs
                .iter()
                .enumerate()
                .any(|(i, c)| configs[..i].contains(c))
        {
            return Err(NavigationError::DuplicateConfiguration);
        }
        Ok(())
    }
    fn snapshot(
        &self,
        children: Vec<crate::router::pages::PageChild<C, T>>,
        _: &Self::State,
    ) -> Self::Snapshot {
        let mut children: Vec<Child<C, T>> = children
            .into_iter()
            .map(|child| match child {
                crate::router::pages::PageChild::Created(c) => c,
                _ => unreachable!("stack children always live"),
            })
            .collect();
        ChildStack {
            active: children.pop().expect("validated nonempty stack"),
            back_stack: children,
        }
    }
    fn back_enabled(&self, state: &Self::State) -> bool {
        self.0.handle_back_button && state.len() > 1
    }
    fn back_transform(&self, mut old: Vec<Rc<C>>) -> Vec<Rc<C>> {
        if old.len() > 1 {
            old.pop();
        }
        old
    }
    fn decode(&self, bytes: &[u8]) -> Result<Option<SavedState<Vec<Rc<C>>>>, NavigationError> {
        self.0
            .codec
            .as_ref()
            .map(|codec| super::state::decode(codec, bytes))
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
        super::state::encode(codec, state, &children()?, next_key).map(Some)
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
            .map(|codec| super::state::encode(codec, configs, children, next_key))
            .transpose()
    }
}
pub(super) fn build<Ctx: ComponentContext + 'static, C: Eq + 'static, T: 'static>(
    context: &Ctx,
    navigation: &(impl crate::router::children::NavigationSource<StackEvent<C>> + ?Sized),
    initial: impl FnOnce() -> Vec<C>,
    options: StackOptions<C>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<Value<ChildStack<C, T>>, NavigationError> {
    crate::router::engine::build_with_subscriber(
        context,
        |receiver| {
            crate::router::navigation::subscribe(
                navigation,
                receiver,
                std::convert::identity,
                std::convert::identity,
                std::convert::identity,
            )
        },
        || initial().into_iter().map(Rc::new).collect(),
        options.key.clone(),
        StackModel(options),
        factory,
    )
    .map(|(value, _)| value)
}
