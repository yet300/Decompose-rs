//! Custom immutable navigation state using the shared child lifecycle engine.
mod navigation;
mod state;
mod types;
pub use super::{Child, NavigationError};
use crate::{ComponentContext, value::Value};
pub use navigation::{NavigationResult, NavigationSource, SimpleNavigation};
pub use state::NavStateCodec;
use std::{cell::RefCell, marker::PhantomData, rc::Rc};
pub use types::{ChildItem, ChildNavState, ChildStatus, NavState};

/// Observer of committed navigation state; the initial callback has no old state.
pub type StateChanged<N> = dyn Fn(&N, Option<&N>);
/// Completion for an explicit event at each connected receiver.
pub type EventComplete<E, N> = dyn Fn(&E, Result<NavigationResult<N>, NavigationError>);
/// A cached Back action producing the next immutable navigation state.
pub type BackAction<N> = dyn Fn() -> N;
/// Computes a Back action once for each committed navigation state.
pub type BackTransformer<N> = dyn Fn(&N) -> Option<Rc<BackAction<N>>>;
type StateMapper<C, T, N, S> = dyn Fn(&N, Vec<ChildItem<C, T>>) -> S;

/// Generic navigation construction options. Persistence is explicit and optional.
pub struct ChildrenOptions<E, N> {
    /// Unique state-provider and retained-instance key.
    pub key: String,
    /// Equal configurations use stable occurrence matching when enabled.
    pub allow_duplicates: bool,
    /// Whole-navigation-state serialization; absent means transient navigation.
    pub codec: Option<NavStateCodec<N>>,
    /// Runs after publication, including initialization and unchanged transitions.
    pub on_state_changed: Option<Rc<StateChanged<N>>>,
    /// Runs once per explicit event after publication and the state observer.
    /// If absent, rejected explicit events panic with their error.
    pub on_event_complete: Option<Rc<EventComplete<E, N>>>,
    /// Optional conditional automatic Back handling at priority zero.
    pub back_transformer: Option<Rc<BackTransformer<N>>>,
}
impl<E, N> Default for ChildrenOptions<E, N> {
    fn default() -> Self {
        Self {
            key: "DefaultChildren".into(),
            allow_duplicates: false,
            codec: None,
            on_state_changed: None,
            on_event_complete: None,
            back_transformer: None,
        }
    }
}
struct ChildrenModel<C, T, E, N, S> {
    options: ChildrenOptions<E, N>,
    mapper: Rc<StateMapper<C, T, N, S>>,
    back_action: RefCell<Option<Rc<BackAction<N>>>>,
    marker: PhantomData<fn(C, T, S)>,
}
impl<C: Eq + 'static, T: 'static, E: 'static, N: NavState<C>, S: 'static> super::engine::Model<C, T>
    for ChildrenModel<C, T, E, N, S>
{
    type State = N;
    type Snapshot = S;
    fn canonicalize_reused_configurations(&self) -> bool {
        true
    }
    fn persistent(&self) -> bool {
        self.options.codec.is_some()
    }
    fn configurations(&self, state: &N) -> Vec<Rc<C>> {
        state
            .children()
            .into_iter()
            .map(|c| c.configuration)
            .collect()
    }
    fn statuses(&self, state: &N) -> Vec<ChildStatus> {
        state.children().into_iter().map(|c| c.status).collect()
    }
    fn validate(&self, state: &N) -> Result<(), NavigationError> {
        let children = state.children();
        if !self.options.allow_duplicates
            && children.iter().enumerate().any(|(i, c)| {
                children[..i]
                    .iter()
                    .any(|p| p.configuration == c.configuration)
            })
        {
            return Err(NavigationError::DuplicateConfiguration);
        }
        Ok(())
    }
    fn snapshot(&self, children: Vec<ChildItem<C, T>>, state: &N) -> S {
        (self.mapper)(state, children)
    }
    fn on_state_changed(&self, state: &N, old: Option<&N>) {
        if let Some(callback) = &self.options.on_state_changed {
            callback(state, old);
        }
    }
    fn back_enabled(&self, state: &N) -> bool {
        let action = self
            .options
            .back_transformer
            .as_ref()
            .and_then(|transformer| transformer(state));
        let enabled = action.is_some();
        let old = self.back_action.replace(action);
        drop(old);
        enabled
    }
    fn back_transform(&self, state: N) -> N {
        let action = self.back_action.borrow().clone();
        action.map_or(state, |action| action())
    }
    fn on_back_error(&self, error: NavigationError) {
        panic!("{error}");
    }
    fn decode(
        &self,
        bytes: &[u8],
    ) -> Result<Option<super::engine::SavedState<N>>, NavigationError> {
        self.options
            .codec
            .as_ref()
            .map_or(Ok(None), |codec| state::decode::<C, N>(codec, bytes))
    }
    fn encode_with_children(
        &self,
        state: &N,
        next_key: u64,
        children: impl FnOnce() -> Result<
            Vec<(String, std::collections::BTreeMap<String, Vec<u8>>)>,
            NavigationError,
        >,
    ) -> Result<Option<Vec<u8>>, NavigationError> {
        let Some(codec) = &self.options.codec else {
            return Ok(None);
        };
        let Some(payload) = codec.encode_payload(state)? else {
            return Ok(None);
        };
        state::encode_payload(&payload, &children()?, next_key).map(Some)
    }
    fn encode(
        &self,
        state: &N,
        children: &Vec<(String, std::collections::BTreeMap<String, Vec<u8>>)>,
        next_key: u64,
    ) -> Result<Option<Vec<u8>>, NavigationError> {
        self.encode_with_children(state, next_key, || Ok(children.clone()))
    }
}

/// Builds arbitrary ordered children from immutable navigation state.
///
/// The source subscribes before lazy initialization, factories and hooks run.
/// Setup emissions are buffered; all reentrant events then use the shared FIFO.
/// The parent owns the scopes until its lifecycle ends, even if this view or the
/// source is dropped. Configuration equality reuses children by occurrence.
pub fn children<Ctx, C, T, E, N, S>(
    context: &Ctx,
    source: &(impl NavigationSource<E> + ?Sized),
    initial_state: impl FnOnce() -> N,
    options: ChildrenOptions<E, N>,
    nav_transformer: impl Fn(N, &E) -> N + 'static,
    state_mapper: impl Fn(&N, Vec<ChildItem<C, T>>) -> S + 'static,
    child_factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<Value<S>, NavigationError>
where
    Ctx: ComponentContext + 'static,
    C: Eq + 'static,
    T: 'static,
    E: 'static,
    N: NavState<C>,
    S: 'static,
{
    let completion = options.on_event_complete.clone();
    let transformer = Rc::new(nav_transformer);
    let key = options.key.clone();
    let model = ChildrenModel {
        options,
        mapper: Rc::new(state_mapper),
        back_action: RefCell::new(None),
        marker: PhantomData,
    };
    super::engine::build_with_subscriber(
        context,
        move |receiver| {
            source.subscribe(Rc::new(move |event| {
                let transform = transformer.clone();
                let input = event.clone();
                let complete = completion.clone();
                receiver(super::navigation::Event {
                    transformer: Rc::new(move |state| transform(state, &input)),
                    completion: Rc::new(move |result| {
                        if let Some(callback) = &complete {
                            callback(
                                &event,
                                result.map(|r| NavigationResult {
                                    old_state: r.old_stack,
                                    new_state: r.new_stack,
                                }),
                            );
                        } else if let Err(error) = result {
                            panic!("{error}");
                        }
                    }),
                });
            }))
        },
        initial_state,
        key,
        model,
        child_factory,
    )
    .map(|(value, _)| value)
}
