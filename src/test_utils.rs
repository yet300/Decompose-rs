//! Public test harness using the same services and routers as application code.
//!
//! No extra dependencies or feature gate are needed. Lifecycle, state keeper,
//! instance keeper and predictive Back are controlled through the normal
//! [`ComponentContext`] handles. Keepers are externally owned, matching
//! upstream: destroying just the lifecycle leaves retained instances intact;
//! [`TestComponentContext::recreate`] explicitly ends them for process death.

use crate::{
    ComponentContext, ContextFactory, ContextParts, InstanceKeeper, InstanceStore, LifecycleError,
    LifecycleRegistry, LifecycleState, SharedBack, StateKeeper, StateKeeperError, StateStore,
};
use std::{collections::BTreeMap, rc::Rc};

/// Failure while saving or recreating a test scope.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TestContextError {
    /// A provider could not save state; no lifecycle teardown has occurred.
    #[error(transparent)]
    State(#[from] StateKeeperError),
    /// The old lifecycle ended or rejected a transition.
    #[error(transparent)]
    Lifecycle(#[from] LifecycleError),
}

/// Controllable component context, initially Resumed with empty collaborators.
#[derive(Debug, Clone)]
pub struct TestComponentContext {
    lifecycle: LifecycleRegistry,
    state: StateStore,
    instances: InstanceStore,
    back: SharedBack,
}

impl TestComponentContext {
    /// Fresh resumed context.
    #[must_use]
    pub fn new() -> Self {
        Self::with_lifecycle_state(LifecycleState::Resumed)
    }

    /// Fresh context whose lifecycle has not been created.
    #[must_use]
    pub fn initialized() -> Self {
        Self::with_lifecycle_state(LifecycleState::Initialized)
    }

    /// Fresh collaborators and a lifecycle at the requested state.
    #[must_use]
    pub fn with_lifecycle_state(state: LifecycleState) -> Self {
        let lifecycle = LifecycleRegistry::new();
        lifecycle
            .move_to(state)
            .expect("fresh lifecycle accepts every state");
        Self::with_stores(
            lifecycle,
            StateStore::from(StateKeeper::new()),
            InstanceStore::from(InstanceKeeper::new()),
            SharedBack::new(),
        )
    }

    /// Uses supplied services verbatim without lifecycle attach hooks.
    #[must_use]
    pub fn with_stores(
        lifecycle: LifecycleRegistry,
        state: StateStore,
        instances: InstanceStore,
        back: SharedBack,
    ) -> Self {
        Self {
            lifecycle,
            state,
            instances,
            back,
        }
    }

    /// Assembles a test context through the standard detached child factory.
    #[must_use]
    pub fn from_parts(parts: ContextParts) -> Self {
        Self::with_stores(
            parts.lifecycle().clone(),
            parts.state_store(),
            parts.instance_store(),
            parts.back_handler(),
        )
    }

    /// Saves durable state bytes, excluding memory-only navigation data.
    pub fn save_bytes(&self) -> Result<BTreeMap<String, Vec<u8>>, StateKeeperError> {
        self.state.save().map_err(|error| match error {
            crate::StateAccessError::Keeper(error) => error,
            crate::StateAccessError::Borrowed => StateKeeperError::Encode {
                key: String::new(),
                reason: "state store is borrowed".into(),
            },
        })
    }

    /// Saves first, then destroys the old lifecycle and builds a fresh scope
    /// at its previous state. Configuration changes reuse retained instances
    /// and memory-only saved data. Process death uses durable bytes and ends
    /// the old instance keeper. Both paths supply a fresh Back dispatcher.
    ///
    /// Saving failures leave the old lifecycle and instance keeper unchanged.
    /// A destroyed lifecycle returns `AlreadyDestroyed` without saving.
    pub fn recreate(&self, configuration_change: bool) -> Result<Self, TestContextError> {
        let state = self.lifecycle.state();
        if state == LifecycleState::Destroyed {
            return Err(LifecycleError::AlreadyDestroyed.into());
        }
        let map_to_keeper = |error: crate::StateAccessError| match error {
            crate::StateAccessError::Keeper(error) => error,
            crate::StateAccessError::Borrowed => StateKeeperError::Encode {
                key: String::new(),
                reason: "state store is borrowed".into(),
            },
        };
        let restored = if configuration_change {
            StateKeeper::from_snapshot(self.state.save_snapshot().map_err(map_to_keeper)?)
        } else {
            StateKeeper::with_restored(self.save_bytes()?)
        };
        if state == LifecycleState::Initialized {
            self.lifecycle.create()?;
        }
        if !configuration_change {
            self.instances
                .destroy_all()
                .map_err(|_| StateKeeperError::Encode {
                    key: String::new(),
                    reason: "instance store is borrowed".into(),
                })?;
        }
        self.lifecycle.destroy()?;
        let instances = if configuration_change {
            self.instances.clone()
        } else {
            InstanceStore::from(InstanceKeeper::new())
        };
        let lifecycle = LifecycleRegistry::new();
        lifecycle.move_to(state)?;
        Ok(Self::with_stores(
            lifecycle,
            StateStore::from(restored),
            instances,
            SharedBack::new(),
        ))
    }
}
impl Default for TestComponentContext {
    fn default() -> Self {
        Self::new()
    }
}
impl ComponentContext for TestComponentContext {
    fn lifecycle(&self) -> LifecycleRegistry {
        self.lifecycle.clone()
    }
    fn state_store(&self) -> StateStore {
        self.state.clone()
    }
    fn instance_store(&self) -> InstanceStore {
        self.instances.clone()
    }
    fn back_handler(&self) -> SharedBack {
        self.back.clone()
    }
    fn context_factory(&self) -> ContextFactory<Self> {
        Rc::new(Self::from_parts)
    }
}
