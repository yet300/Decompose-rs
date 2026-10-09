//! A nonempty, observable stack of lifecycle-aware child components.
mod engine;
mod navigation;
mod state;

use crate::value::Value;
use crate::{ComponentContext, DecomposeError};
pub use navigation::{NavigationResult, StackNavigation};
pub use state::StackCodec;
use std::rc::Rc;

/// A created child. Holding a snapshot retains the component, not its active lifecycle.
pub struct Child<C, T> {
    /// Immutable configuration.
    pub configuration: Rc<C>,
    /// Shared component instance. The component need not implement Clone.
    pub instance: Rc<T>,
    /// Stable identity preserved while this child survives navigation.
    pub key: String,
}
impl<C, T> Clone for Child<C, T> {
    fn clone(&self) -> Self {
        Self {
            configuration: self.configuration.clone(),
            instance: self.instance.clone(),
            key: self.key.clone(),
        }
    }
}

/// Immutable view of a nonempty child stack, ordered from oldest to newest.
pub struct ChildStack<C, T> {
    /// The top child.
    pub active: Child<C, T>,
    /// Inactive children, in bottom-to-top order.
    pub back_stack: Vec<Child<C, T>>,
}
impl<C, T> ChildStack<C, T> {
    /// All children in bottom-to-top order.
    pub fn items(&self) -> impl DoubleEndedIterator<Item = &Child<C, T>> {
        self.back_stack.iter().chain(std::iter::once(&self.active))
    }
}

/// Failure while constructing or changing a stack.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NavigationError {
    /// Every stack must have an active child.
    #[error("configuration stack must not be empty")]
    EmptyStack,
    /// Equal configurations require the per-stack opt-in.
    #[error("duplicate configurations are disabled")]
    DuplicateConfiguration,
    /// The hosting scope already ended.
    #[error("stack scope is destroyed")]
    Destroyed,
    /// Saved bytes or user codec could not be processed.
    #[error("stack state: {0}")]
    State(String),
    /// Context primitive could not be constructed.
    #[error(transparent)]
    Context(#[from] DecomposeError),
}

/// Stack construction options. Defaults match upstream's transient stack.
pub struct StackOptions<C> {
    /// Must be unique among providers and stacks within the hosting context.
    pub key: String,
    /// Pop on unhandled Back when there is more than one child.
    pub handle_back_button: bool,
    /// Allow equal configurations; identity is then matched by occurrence.
    pub allow_duplicates: bool,
    /// Explicit configuration codec; None selects transient navigation.
    pub codec: Option<StackCodec<C>>,
}
impl<C> Default for StackOptions<C> {
    fn default() -> Self {
        Self {
            key: "DefaultChildStack".into(),
            handle_back_button: false,
            allow_duplicates: false,
            codec: None,
        }
    }
}

/// Builds a stack using the hosting context's factory, preserving custom services.
///
/// Recursive navigation is queued. The hosting lifecycle owns the stack;
/// dropping an observable view does not destroy child scopes.
pub fn child_stack<Ctx, C, T>(
    context: &Ctx,
    navigation: &StackNavigation<C>,
    initial: Vec<C>,
    options: StackOptions<C>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<Value<ChildStack<C, T>>, NavigationError>
where
    Ctx: ComponentContext + 'static,
    C: Eq + 'static,
    T: 'static,
{
    engine::build(context, navigation, initial, options, factory)
}
