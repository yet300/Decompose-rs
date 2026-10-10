//! In-crate test utilities (NOT a published crate).
//!
//! [`TestContext`] mirrors upstream `TestComponentContext`: a controllable
//! harness for driving lifecycles, saving/restoring bytes, config-change vs
//! process-death recreation, back dispatch, nested child construction, and
//! custom contexts.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use essenty_lifecycle::LifecycleRegistry;

use crate::context::{
    ComponentContext, ContextFactory, ContextParts, SharedInstances, SharedKeeper,
};
use crate::shared_back::SharedBack;

/// Test component context: `Resumed` lifecycle, empty keepers/dispatchers,
/// `TestContext` factory. Mirrors upstream `TestComponentContext`.
#[derive(Debug, Clone)]
pub(crate) struct TestContext {
    lifecycle: LifecycleRegistry,
    state: SharedKeeper,
    instances: SharedInstances,
    back: SharedBack,
}

impl TestContext {
    /// Fresh harness with a `Resumed` lifecycle and empty collaborators.
    pub(crate) fn new() -> Self {
        let lifecycle = LifecycleRegistry::new();
        lifecycle.create().unwrap();
        lifecycle.start().unwrap();
        lifecycle.resume().unwrap();
        Self {
            lifecycle,
            state: Rc::new(RefCell::new(crate::StateKeeper::new())),
            instances: Rc::new(RefCell::new(essenty_instance_keeper::InstanceKeeper::new())),
            back: SharedBack::new(),
        }
    }

    /// Fresh harness that stays `Initialized` (for min-table tests).
    pub(crate) fn initialized() -> Self {
        Self {
            lifecycle: LifecycleRegistry::new(),
            state: Rc::new(RefCell::new(crate::StateKeeper::new())),
            instances: Rc::new(RefCell::new(essenty_instance_keeper::InstanceKeeper::new())),
            back: SharedBack::new(),
        }
    }

    /// Inherent constructor from prepared parts (test factory target).
    pub(crate) fn from_parts(parts: ContextParts) -> Self {
        Self {
            lifecycle: parts.lifecycle().clone(),
            state: parts.state_store().inner,
            instances: parts.instance_store().inner,
            back: parts.back_handler(),
        }
    }

    /// Saves this scope's state bytes (parent save).
    pub(crate) fn save_bytes(&self) -> BTreeMap<String, Vec<u8>> {
        self.state.borrow().save().unwrap()
    }

    /// Raw state handle for white-box borrow-discipline tests only.
    pub(crate) fn raw_state_keeper(&self) -> SharedKeeper {
        Rc::clone(&self.state)
    }

    /// Recreates the harness from saved bytes, mirroring upstream
    /// `TestComponentContext.recreate`: `config_change = true` retains the
    /// instance keeper (same identity); `false` simulates process death
    /// (fresh keeper, restored state only).
    pub(crate) fn recreate(&self, config_change: bool, saved: BTreeMap<String, Vec<u8>>) -> Self {
        let lifecycle = LifecycleRegistry::new();
        lifecycle.create().unwrap();
        lifecycle.start().unwrap();
        lifecycle.resume().unwrap();
        let instances = if config_change {
            Rc::clone(&self.instances)
        } else {
            Rc::new(RefCell::new(essenty_instance_keeper::InstanceKeeper::new()))
        };
        Self {
            lifecycle,
            state: Rc::new(RefCell::new(crate::StateKeeper::with_restored(saved))),
            instances,
            back: SharedBack::new(),
        }
    }
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentContext for TestContext {
    fn lifecycle(&self) -> LifecycleRegistry {
        self.lifecycle.clone()
    }

    fn state_store(&self) -> crate::StateStore {
        crate::StateStore {
            inner: Rc::clone(&self.state),
        }
    }

    fn instance_store(&self) -> crate::InstanceStore {
        crate::InstanceStore {
            inner: Rc::clone(&self.instances),
        }
    }

    fn back_handler(&self) -> SharedBack {
        self.back.clone()
    }

    fn context_factory(&self) -> ContextFactory<Self> {
        Rc::new(Self::from_parts)
    }
}
