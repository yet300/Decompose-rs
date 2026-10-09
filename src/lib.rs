//! Lifecycle-aware contexts and observable stack, slot, pages and panels navigation.
//!
//! This independent Rust port of Ark Ivanov’s Decompose currently implements
//! [`ComponentContext`], [`DefaultComponentContext`], [`ContextFactory`] and
//! [`router::stack`], [`router::slot`], [`router::pages`] and [`router::panels`] with navigation operations, Back handling and explicit
//! configuration codecs. [`value`] supplies synchronous observable snapshots.
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
//! published `essenty-* 0.1.0` crates. Other navigation families, web history
//! and UI/platform integrations are outside the current delivery. See the
//! repository README and `examples/child_stack.rs`, `examples/child_slot.rs`,
//! `examples/child_pages.rs` and `examples/child_panels.rs` for usage, and `NOTICE`
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
