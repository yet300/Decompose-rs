//! Runtime-independent component navigation.
pub mod children;
mod engine;
pub mod items;
mod navigation;
pub use navigation::NavigationEvent;
pub mod pages;
pub mod panels;
pub mod slot;
pub mod stack;
pub mod webhistory;
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
    /// A nonempty pages state requires an in-range selected index.
    #[error("invalid pages selection {selected_index:?} for {item_count} items")]
    InvalidSelection {
        selected_index: Option<usize>,
        item_count: usize,
    },
    /// Equal configurations violate this navigation model's duplicate policy.
    #[error("duplicate configurations are disabled")]
    DuplicateConfiguration,
    /// The hosting navigation scope already ended.
    #[error("navigation scope is destroyed")]
    Destroyed,
    /// Lazy lookup requested an absent configuration.
    #[error("unknown item configuration")]
    UnknownConfiguration,
    /// Inactive lazy activation cannot run during navigation or Back dispatch.
    #[error("navigation is in progress")]
    NavigationInProgress,
    /// Queued work removed or deactivated the requested child.
    #[error("requested child is unavailable")]
    ChildUnavailable,
    /// Saved bytes or user codec could not be processed.
    #[error("navigation state: {0}")]
    State(String),
    /// Context primitive could not be constructed.
    #[error(transparent)]
    Context(#[from] DecomposeError),
}
