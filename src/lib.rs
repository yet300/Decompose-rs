//! Lifecycle-aware contexts, standard routers and generic child navigation.
//!
//! This independent Rust port of Ark Ivanov’s Decompose currently implements
//! [`ComponentContext`], [`DefaultComponentContext`], [`ContextFactory`] and
//! [`router::stack`], [`router::slot`], [`router::pages`], [`router::panels`] and
//! [`router::items`] with navigation operations, Back handling and explicit
//! configuration codecs. Standard factories also accept typed custom navigation
//! sources, and navigator traits provide reusable family operations.
//! [`router::children`] connects custom immutable state
//! and event sources to the same child lifecycle engine. [`value`] supplies
//! synchronous observable snapshots, cached mapping, identity updates and
//! lifecycle observation.
//!
//! The core uses `Rc` and is single-threaded, synchronous and runtime-agnostic.
//! Contexts, configurations and components need no Clone implementation.
//! Custom contexts propagate services through their detached factory. Child
//! lifecycles are bounded by their parent; retained scopes belong to the parent
//! instance keeper. Dropping an observable view does not end navigation.
//!
//! [`StateKeeper`] adapts Essenty state storage to preserve consumed input on
//! construction rollback. Convert owned Essenty keepers with `StateKeeper::from`;
//! [`EssentyStateKeeper`] reexports the original primitive.
//!
//! Behavior is checked against Decompose revision `00840bc`, using the four
//! published `essenty-* 0.1.0` crates. Web history
//! and UI/platform integrations are outside the current delivery. See the
//! repository README and `examples/child_stack.rs`, `examples/child_slot.rs`,
//! `examples/child_pages.rs`, `examples/child_panels.rs` and
//! `examples/child_items.rs`, `examples/generic_navigation.rs` and
//! `examples/observable_values.rs` and `examples/router_extension_points.rs`
//! for usage, and `NOTICE`
//! for attribution. This project has no official upstream affiliation.

mod child;
mod context;
mod error;
mod framing;
mod merged;
mod retention;
pub mod router;
mod shared_back;
mod state;
#[cfg(test)]
mod testing;
pub mod value;

pub use context::{
    ComponentContext, ContextFactory, ContextParts, DEFAULT_BACK_PRIORITY, DefaultComponentContext,
    MANUAL_DESTROY_PANIC_MESSAGE, SharedInstances, SharedKeeper,
};
pub use error::{DecomposeError, DispatchInProgress};
pub use essenty_back_handler::{BackError, BackEvent, BackHandle, BackPhase, GesturePosition};
pub use essenty_instance_keeper::{InstanceKeeper, InstanceKeeperError};
pub use essenty_lifecycle::{LifecycleError, LifecycleRegistry, LifecycleState};
pub use essenty_state_keeper::{StateKeeper as EssentyStateKeeper, StateKeeperError};
pub use merged::MergedLifecycle;
pub use shared_back::SharedBack;
pub use state::StateKeeper;
