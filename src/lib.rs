//! `decompose-rs`: independent Rust port of Ark Ivanov's Decompose.
//!
//! Phase 1 implements the `ComponentContext` foundation only:
//!
//! - [`ComponentContext`] trait with provided `child_context` constructors
//! - [`DefaultComponentContext`] with upstream-equivalent defaults
//! - [`ContextFactory`] / [`ContextParts`] detached-factory assembly
//! - [`MergedLifecycle`] (`min(parent, manual)` as a lifecycle)
//! - [`SharedBack`] reentrancy-safe back-dispatcher handle
//! - [`DecomposeError`] minimal producing-error surface
//! - [`DEFAULT_BACK_PRIORITY`] + LIFO tiebreak contract
//!
//! Child `StateKeeper` / `InstanceKeeper` / back-handler scoping,
//! construction atomicity with factory-panic rollback, and root
//! `InstanceKeeper` attachment all live behind the `child_context`
//! constructors.
//!
//! Explicitly NOT implemented (Phase 2+): `Value` / `MutableValue` / `Relay`,
//! `ChildrenNavigator`, `ChildStack`, `ChildSlot`, `ChildPages`,
//! `ChildPanels`, `ChildItems`, web history, and any UI integration.
//!
//! This crate is an independent implementation preserving observable
//! behavioral semantics of upstream Decompose
//! (`arkivanov/Decompose@00840bc`). No claim of official affiliation.
//! See `NOTICE` for attribution. The core is synchronous,
//! runtime-agnostic, and single-threaded (`!Send + !Sync`, `Rc`-based like
//! `essenty-*`), with registry-only `essenty-* 0.1.0` dependencies.

mod child;
mod context;
mod error;
mod framing;
mod merged;
mod shared_back;
#[cfg(test)]
mod testing;

pub use context::{
    ComponentContext, ContextFactory, ContextParts, DEFAULT_BACK_PRIORITY, DefaultComponentContext,
    MANUAL_DESTROY_PANIC_MESSAGE, SharedInstances, SharedKeeper,
};
pub use error::{DecomposeError, DispatchInProgress};
pub use essenty_back_handler::{BackError, BackEvent, BackHandle, BackPhase, GesturePosition};
pub use essenty_instance_keeper::{InstanceKeeper, InstanceKeeperError};
pub use essenty_lifecycle::{LifecycleError, LifecycleRegistry, LifecycleState};
pub use essenty_state_keeper::{StateKeeper, StateKeeperError};
pub use merged::MergedLifecycle;
pub use shared_back::SharedBack;
