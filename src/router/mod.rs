//! Runtime-independent component navigation.
mod engine;
mod navigation;
pub mod slot;
pub mod stack;
use crate::DecomposeError;
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

/// Failure while constructing or changing child navigation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NavigationError {
    /// Every stack must have an active child.
    #[error("configuration stack must not be empty")]
    EmptyStack,
    /// Equal configurations require the per-stack opt-in.
    #[error("duplicate configurations are disabled")]
    DuplicateConfiguration,
    /// The hosting navigation scope already ended.
    #[error("navigation scope is destroyed")]
    Destroyed,
    /// Saved bytes or user codec could not be processed.
    #[error("navigation state: {0}")]
    State(String),
    /// Context primitive could not be constructed.
    #[error(transparent)]
    Context(#[from] DecomposeError),
}
