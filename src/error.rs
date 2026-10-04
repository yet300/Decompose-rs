//! Phase-1 error surface.
//!
//! Every variant has exactly one real producing path in Phase 1, one test,
//! and one documentation entry. No speculative variants exist: there is no
//! `ContextDestroyed` — a destroyed *lifecycle* does not by itself forbid
//! child creation (the scope is simply born destroyed) — BUT if the relevant
//! parent `InstanceKeeper` scope has already been destroyed, construction
//! fails with `InstanceKeeperError::Destroyed` (for default root contexts,
//! lifecycle destroy drives the attached keeper's `destroy_all`, so later
//! child creation on that root fails). There is likewise no `TypeMismatch`
//! wrapper (child-scope construction uses fixed internal types; genuine
//! keeper mismatches surface through the transparent
//! [`DecomposeError::InstanceKeeper`] passthrough).
//!
//! Programmer-contract violations that surface through Essenty-owned calls
//! are panics, not `Result`s (see
//! [`ComponentContext::child_context_with_lifecycle`](crate::ComponentContext::child_context_with_lifecycle)).

use essenty_instance_keeper::InstanceKeeperError;
use essenty_state_keeper::StateKeeperError;
use thiserror::Error;

/// Error returned when a new back callback is registered while that same
/// dispatcher is dispatching.
///
/// Registration during dispatch is rejected (rather than queued) because
/// Essenty exposes no id reservation for queued registration, and a
/// wrapper-level id-indirection map was rejected as permanent complexity for
/// a path the engine never needs. All other same-dispatcher mutations from
/// inside a callback ([`SharedBack::set_enabled`](crate::SharedBack::set_enabled),
/// [`SharedBack::set_priority`](crate::SharedBack::set_priority),
/// [`SharedBack::unregister`](crate::SharedBack::unregister), listener
/// add/remove) queue safely and apply deterministically after the current
/// dispatch completes.
///
/// At `ComponentContext` construction boundaries this maps to
/// [`DecomposeError::BackDispatchInProgress`]: building a child context from
/// inside a back callback of the same parent dispatcher fails atomically —
/// no partial child scope is left behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("back callback registration requested while that dispatcher is dispatching")]
pub struct DispatchInProgress;

/// Phase-1 producing errors for `ComponentContext` child-scope construction.
///
/// Each variant documents its producing path:
///
/// - [`DuplicateKey`](DecomposeError::DuplicateKey): `child_context*` with a
///   key already registered in the parent state keeper. Checked before any
///   restored bytes are consumed, so the original bytes remain consumable.
/// - [`ChildLifecycleDestroyed`](DecomposeError::ChildLifecycleDestroyed):
///   `child_context_with_lifecycle*` with an already-destroyed supplied
///   lifecycle, detected before any registration side effect.
/// - [`BackDispatchInProgress`](DecomposeError::BackDispatchInProgress):
///   child construction attempted from inside a back callback of the same
///   parent dispatcher (registration into the busy dispatcher was rejected).
///   All resources created by the attempt are rolled back.
/// - [`StateKeeper`](DecomposeError::StateKeeper): transparent Essenty
///   passthrough, including `Decode` for malformed `DCS1` child-state frames.
/// - [`InstanceKeeper`](DecomposeError::InstanceKeeper): transparent Essenty
///   passthrough, including `Destroyed` when the relevant parent keeper
///   scope has already ended, and `TypeMismatch` when the key holds a foreign
///   type.
///
/// There is deliberately no `Lifecycle` passthrough: no Phase-1 producing
/// path yields a `LifecycleError` (the merged driver swallows
/// `AlreadyDestroyed` from reentrant terminal paths as idempotent success),
/// so the variant would be speculative.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecomposeError {
    /// A state provider is already registered under this key in the parent
    /// scope. Producing path: duplicate `child_context*` key.
    #[error("duplicate child key: '{key}'")]
    DuplicateKey {
        /// The rejected key.
        key: String,
    },
    /// The supplied manual lifecycle was already destroyed at creation time.
    /// Producing path: `child_context_with_lifecycle*` with a destroyed
    /// input. No registration side effect has happened, so recovery is safe.
    #[error("supplied child lifecycle is already destroyed")]
    ChildLifecycleDestroyed,
    /// Child construction required registering into the parent back
    /// dispatcher while it was dispatching. Producing path: `child_context*`
    /// called from inside a back callback of the same parent dispatcher.
    #[error("parent back dispatcher is dispatching; child construction rejected")]
    BackDispatchInProgress,
    /// Transparent Essenty state-keeper passthrough (incl. `DCS1` decode).
    #[error(transparent)]
    StateKeeper(#[from] StateKeeperError),
    /// Transparent Essenty instance-keeper passthrough (incl. `Destroyed`
    /// when the parent scope already ended, and `TypeMismatch`).
    #[error(transparent)]
    InstanceKeeper(#[from] InstanceKeeperError),
}

impl From<DispatchInProgress> for DecomposeError {
    fn from(_: DispatchInProgress) -> Self {
        DecomposeError::BackDispatchInProgress
    }
}
