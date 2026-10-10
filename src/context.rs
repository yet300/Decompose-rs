//! The approved `ComponentContext` contract.
//!
//! A context is exactly five things: `lifecycle`, `state_keeper`,
//! `instance_keeper`, `back_handler`, plus `context_factory` — a detached
//! factory that mints siblings of the same type with no parent link.
//! Attachment (child scoping) is always the caller's job (`child_context`,
//! later the navigation engine).
//!
//! Custom contexts flow through untouched: child construction always goes
//! through `self.context_factory()`, so no `Clone` bound and no casts are
//! ever needed. The factory is computed from `&self` per call and never
//! stored, so no self-referential cycle is possible by construction.

use std::cell::RefCell;
use std::rc::Rc;

use crate::StateKeeper;
use essenty_instance_keeper::InstanceKeeper;
use essenty_lifecycle::LifecycleRegistry;

use crate::child::build_child;
use crate::error::DecomposeError;
use crate::shared_back::SharedBack;
mod stores;
pub use stores::{InstanceAccessError, InstanceStore, StateAccessError, StateStore};

/// Shared parent-side state-keeper handle (crate-internal).
/// Mutated through short `borrow_mut()` calls that never cross observer
/// delivery, back dispatch, user callbacks, or factory invocation.
pub(crate) type SharedKeeper = Rc<RefCell<StateKeeper>>;

/// Shared parent-side instance-keeper handle (crate-internal).
pub(crate) type SharedInstances = Rc<RefCell<InstanceKeeper>>;

/// Detached context factory: builds a context of type `C` from prepared
/// child-scope parts, closing over any extra services.
///
/// The factory is explicitly detached (same contract as upstream
/// `componentContextFactory`): scoping inputs are prepared by
/// `child_context`/the engine; the factory only assembles. Because retained
/// provider closures must be `'static`, the public child-construction
/// boundary takes `&str` keys but everything captured is an owned `String`.
///
/// Canonical custom-context pattern — delegate, never destructure:
///
/// ```rust
/// # use std::rc::Rc;
/// # use decompose_rs::{ContextFactory, ContextParts, DefaultComponentContext, ComponentContext};
/// # struct Logger;
/// # struct AppContext { delegate: DefaultComponentContext, logger: Rc<Logger> }
/// # fn example(base_factory: ContextFactory<DefaultComponentContext>, logger: Rc<Logger>) {
/// let base = base_factory.clone();
/// let factory: ContextFactory<AppContext> = Rc::new(move |parts| AppContext {
///     delegate: base(parts),
///     logger: Rc::clone(&logger),
/// });
/// # }
/// ```
pub type ContextFactory<C> = Rc<dyn Fn(ContextParts) -> C>;

/// Prepared child-scope parts consumed by a [`ContextFactory`].
///
/// There is deliberately no public constructor and no public fields:
/// downstream code can inspect/clone the four usable handles but cannot
/// construct or destructure `ContextParts` manually. The sanctioned sink for
/// the whole value is a base-factory call (`base(parts)`) or a concrete
/// inherent `from_parts(parts)`.
#[derive(Debug, Clone)]
pub struct ContextParts {
    lifecycle: LifecycleRegistry,
    state: SharedKeeper,
    instances: SharedInstances,
    back: SharedBack,
}

impl ContextParts {
    pub(crate) fn new(
        lifecycle: LifecycleRegistry,
        state: SharedKeeper,
        instances: SharedInstances,
        back: SharedBack,
    ) -> Self {
        Self {
            lifecycle,
            state,
            instances,
            back,
        }
    }

    /// The prepared child lifecycle (merged, or shared with the parent for
    /// fixed children).
    #[must_use]
    pub fn lifecycle(&self) -> &LifecycleRegistry {
        &self.lifecycle
    }

    /// Opaque child state service; no borrow guard is exposed.
    pub fn state_store(&self) -> StateStore {
        StateStore {
            inner: self.state.clone(),
        }
    }

    /// Opaque child instance service with owned typed results.
    pub fn instance_store(&self) -> InstanceStore {
        InstanceStore {
            inner: self.instances.clone(),
        }
    }

    /// The prepared child back handler (bridged into the parent).
    #[must_use]
    pub fn back_handler(&self) -> SharedBack {
        self.back.clone()
    }
}

/// Internal child key: owned, unique-within-parent only.
///
/// No Phase-1 external API consumes this type; it stays crate-private.
/// Upstream enforces only uniqueness within the parent scope (no
/// length/content validation), so `""` is legal if unique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChildKey(String);

impl ChildKey {
    pub(crate) fn new(key: &str) -> Self {
        Self(key.to_owned())
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Decompose default back-callback priority, mirroring upstream
/// `BackCallback.PRIORITY_DEFAULT = 0`. Higher priority wins; ties break
/// toward the later registration (LIFO).
pub const DEFAULT_BACK_PRIORITY: i32 = 0;

/// Message contract for the manual-destroy programmer error.
///
/// Destroying an attached child lifecycle via
/// [`child_context_with_lifecycle`](ComponentContext::child_context_with_lifecycle)
/// panics with exactly this message after functional cleanup hooks have run.
pub const MANUAL_DESTROY_PANIC_MESSAGE: &str =
    "The lifecycle of a child ComponentContext must never be destroyed manually.";

/// Component context: the five load-bearing capabilities plus provided
/// child-scope constructors.
///
/// There is deliberately NO `Clone` bound (a custom context may hold
/// non-`Clone` services) and NO trait-level `from_parts` (a generic
/// constructor cannot synthesize custom services — `from_parts` exists only
/// as an inherent constructor on concrete types). The trait's fifth member
/// is the factory, mirroring upstream's `componentContextFactory`.
///
/// Child construction always uses `self.context_factory()`, so custom types
/// are preserved with zero casts. All provided constructors take `&self`:
/// parent keepers are shared handles mutated via short borrows that never
/// cross observer delivery, back dispatch, user callbacks, or factory
/// invocation.
pub trait ComponentContext: Sized {
    /// Explicit diagnostic configuration inherited by descendants. `None`
    /// preserves legacy thread-local diagnostics for existing custom contexts.
    fn configuration(&self) -> Option<crate::DecomposeConfig> {
        None
    }
    /// The component lifecycle.
    fn lifecycle(&self) -> LifecycleRegistry;
    /// Opaque state service. A clone shares the same provider scope.
    fn state_store(&self) -> StateStore;
    /// Opaque retention service. Stored types use Essenty's `Any` registry.
    fn instance_store(&self) -> InstanceStore;
    /// The component back handler. Access goes exclusively through
    /// [`SharedBack`]; raw `RefCell<BackDispatcher>` borrowing is
    /// crate-private.
    fn back_handler(&self) -> SharedBack;
    /// Upstream `componentContextFactory`: builds the SAME custom type from
    /// prepared child scope parts, closing over any extra services.
    /// Computed from `&self` per call; never stored.
    fn context_factory(&self) -> ContextFactory<Self>;

    /// Assembles an immediate child from prepared parts. Override this to avoid
    /// allocating a detached factory for synchronous `child_context*` calls.
    /// Routers still use `context_factory` for future child creation; existing
    /// custom implementations inherit their factory behavior unchanged.
    fn make_context(&self, parts: ContextParts) -> Self {
        self.context_factory()(parts)
    }

    /// Fixed-child helper ("lives as long as the parent").
    ///
    /// The child shares the parent lifecycle reference (no merge), restores
    /// namespaced state under `key`, retains its instance scope under `key`,
    /// and bridges back handling at [`DEFAULT_BACK_PRIORITY`].
    ///
    /// # Errors
    ///
    /// - [`DecomposeError::DuplicateKey`] when `key` is already registered
    ///   in the parent state keeper (checked before any restored bytes are
    ///   consumed).
    /// - [`DecomposeError::InstanceKeeper`] when the parent instance scope
    ///   already ended (`Destroyed`) or holds a foreign type.
    /// - [`DecomposeError::StateKeeper`] for malformed restored child state.
    /// - [`DecomposeError::BackDispatchInProgress`] when called from inside
    ///   a back callback of the same parent dispatcher.
    ///
    /// # Panics
    ///
    /// Opaque [`StateStore`]/[`InstanceStore`] handles never expose borrow
    /// guards, so no caller-held borrow can conflict with this call.
    fn child_context(&self, key: &str) -> Result<Self, DecomposeError> {
        self.child_context_with_priority(key, DEFAULT_BACK_PRIORITY)
    }

    /// Manually-controlled-lifecycle child.
    ///
    /// The child lifecycle is `min(parent, manual)` (see
    /// [`MergedLifecycle`](crate::MergedLifecycle)); back handling is gated
    /// on the manual lifecycle reaching `Started` and bridged at
    /// [`DEFAULT_BACK_PRIORITY`].
    ///
    /// # Errors
    ///
    /// Same as [`child_context`](ComponentContext::child_context), plus
    /// [`DecomposeError::ChildLifecycleDestroyed`] when `lifecycle` is
    /// already destroyed — detected before any registration side effect.
    ///
    /// # Panics
    ///
    /// Destroying `lifecycle` after successful attachment panics with
    /// [`MANUAL_DESTROY_PANIC_MESSAGE`]. The destroy-contract guard is
    /// subscribed first so it fires last (Essenty delivers destroy observers
    /// in reverse subscription order): functional cleanup hooks run first,
    /// then the panic. Post-panic observer cleanup is unspecified — use
    /// fresh fixtures when testing this path.
    ///
    /// No caller-held borrow can conflict with this call; services are opaque.
    fn child_context_with_lifecycle(
        &self,
        key: &str,
        lifecycle: &LifecycleRegistry,
    ) -> Result<Self, DecomposeError> {
        self.child_context_with_lifecycle_and_priority(key, lifecycle, DEFAULT_BACK_PRIORITY)
    }

    /// Fixed-child helper with an explicit back priority override.
    ///
    /// Experimental: mirrors the upstream `backHandlerPriority` overload.
    /// Higher priority wins globally; default ordering is reverse creation
    /// order (LIFO on ties).
    ///
    /// # Errors
    ///
    /// Same as [`child_context`](ComponentContext::child_context).
    ///
    /// # Panics
    ///
    /// No caller-held borrow can conflict with this call.
    fn child_context_with_priority(
        &self,
        key: &str,
        priority: i32,
    ) -> Result<Self, DecomposeError> {
        build_child(
            &self.lifecycle(),
            &self.state_store().inner,
            &self.instance_store().inner,
            &self.back_handler(),
            ChildKey::new(key),
            None,
            priority,
            |parts| self.make_context(parts),
        )
    }

    /// Manually-controlled-lifecycle child with an explicit back priority
    /// override. Experimental; see
    /// [`child_context_with_priority`](ComponentContext::child_context_with_priority)
    /// for priority semantics.
    ///
    /// # Errors
    ///
    /// Same as
    /// [`child_context_with_lifecycle`](ComponentContext::child_context_with_lifecycle).
    ///
    /// # Panics
    ///
    /// Same manual-destroy contract as
    /// [`child_context_with_lifecycle`](ComponentContext::child_context_with_lifecycle).
    fn child_context_with_lifecycle_and_priority(
        &self,
        key: &str,
        lifecycle: &LifecycleRegistry,
        priority: i32,
    ) -> Result<Self, DecomposeError> {
        build_child(
            &self.lifecycle(),
            &self.state_store().inner,
            &self.instance_store().inner,
            &self.back_handler(),
            ChildKey::new(key),
            Some(lifecycle),
            priority,
            |parts| self.make_context(parts),
        )
    }
}

/// The default component context.
///
/// Only `lifecycle` is required. When omitted, construction supplies:
/// a fresh `StateKeeper`; a fresh `InstanceKeeper` attached to the lifecycle
/// (root destroy ends it via one detached destroy subscription); a fresh
/// back dispatcher; and the `DefaultComponentContext` constructor path as
/// the context factory.
///
/// `DefaultComponentContext` itself implements `Clone` as a shared-handle
/// convenience (clones share one scope). Custom `ComponentContext` types do
/// not need to. Dropping a context — default, custom, or child — never
/// unregisters child state/back/instance resources; teardown is
/// lifecycle-driven.
#[derive(Debug, Clone)]
pub struct DefaultComponentContext {
    lifecycle: LifecycleRegistry,
    state: SharedKeeper,
    instances: SharedInstances,
    back: SharedBack,
    config: Option<crate::DecomposeConfig>,
}

impl DefaultComponentContext {
    fn assemble(
        lifecycle: LifecycleRegistry,
        state: Option<SharedKeeper>,
        instances: Option<SharedInstances>,
        back: Option<SharedBack>,
    ) -> Self {
        let state = state.unwrap_or_else(|| Rc::new(RefCell::new(StateKeeper::new())));
        let back = back.unwrap_or_default();
        let instances = match instances {
            Some(instances) => instances,
            None => {
                // Root attach: a fresh default keeper gets exactly one
                // detached destroy subscription ending it with the root
                // lifecycle. Supplied external keepers get no hook —
                // ownership stays with the caller. The hook captures only
                // the keeper (never the lifecycle it is subscribed to), so
                // no reference cycle is possible.
                let fresh: SharedInstances = Rc::new(RefCell::new(InstanceKeeper::new()));
                let owned = Rc::clone(&fresh);
                let _attach_id = lifecycle
                    .do_on_destroy(move || owned.borrow_mut().destroy_all())
                    .detach();
                fresh
            }
        };
        Self {
            lifecycle,
            state,
            instances,
            back,
            config: None,
        }
    }

    /// Creates a root context: fresh state keeper, fresh instance keeper
    /// attached to `lifecycle`, fresh back dispatcher.
    #[must_use]
    pub fn new(lifecycle: LifecycleRegistry) -> Self {
        Self::assemble(lifecycle, None, None, None)
    }

    /// Selects explicit diagnostics for this context and descendants, bypassing
    /// thread-local handlers. Construct router options with the same config to
    /// select their duplicate defaults; explicit option fields still win.
    #[must_use]
    pub fn with_config(mut self, config: crate::DecomposeConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Creates a context with a supplied back handler; fresh state keeper
    /// and fresh (attached) instance keeper.
    #[must_use]
    pub fn with_back_handler(lifecycle: LifecycleRegistry, back: SharedBack) -> Self {
        Self::assemble(lifecycle, None, None, Some(back))
    }

    /// Creates a context using an opaque supplied state service.
    #[must_use]
    pub fn with_state_store(lifecycle: LifecycleRegistry, state: StateStore) -> Self {
        Self::assemble(lifecycle, Some(state.inner), None, None)
    }

    /// Creates a context using an opaque supplied retention service. The caller
    /// owns its teardown, as with the legacy externally supplied keeper.
    #[must_use]
    pub fn with_instance_store(lifecycle: LifecycleRegistry, instances: InstanceStore) -> Self {
        Self::assemble(lifecycle, None, Some(instances.inner), None)
    }

    /// Supplies all services; external retention is not attached to the lifecycle.
    #[must_use]
    pub fn with_stores(
        lifecycle: LifecycleRegistry,
        state: StateStore,
        instances: InstanceStore,
        back: SharedBack,
    ) -> Self {
        Self::assemble(
            lifecycle,
            Some(state.inner),
            Some(instances.inner),
            Some(back),
        )
    }

    /// Inherent (non-trait) constructor from prepared parts. This is the
    /// default context-factory target; it installs no attach hooks (child
    /// scopes are owned by their parent registration, not by lifecycle
    /// attachment).
    #[must_use]
    pub fn from_parts(parts: ContextParts) -> Self {
        Self {
            lifecycle: parts.lifecycle().clone(),
            state: parts.state_store().inner,
            instances: parts.instance_store().inner,
            back: parts.back_handler(),
            config: None,
        }
    }

    /// Current lifecycle state (convenience for tests and hosts).
    #[must_use]
    pub fn state(&self) -> essenty_lifecycle::LifecycleState {
        self.lifecycle.state()
    }
}

impl ComponentContext for DefaultComponentContext {
    fn make_context(&self, parts: ContextParts) -> Self {
        let mut context = Self::from_parts(parts);
        context.config = self.config.clone();
        context
    }
    fn configuration(&self) -> Option<crate::DecomposeConfig> {
        self.config.clone()
    }
    fn lifecycle(&self) -> LifecycleRegistry {
        self.lifecycle.clone()
    }

    fn state_store(&self) -> StateStore {
        StateStore {
            inner: Rc::clone(&self.state),
        }
    }

    fn instance_store(&self) -> InstanceStore {
        InstanceStore {
            inner: Rc::clone(&self.instances),
        }
    }

    fn back_handler(&self) -> SharedBack {
        self.back.clone()
    }

    fn context_factory(&self) -> ContextFactory<Self> {
        let config = self.config.clone();
        Rc::new(move |parts| {
            let mut context = Self::from_parts(parts);
            context.config = config.clone();
            context
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use essenty_lifecycle::LifecycleState;

    fn resumed_lifecycle() -> LifecycleRegistry {
        let lifecycle = LifecycleRegistry::new();
        lifecycle.create().unwrap();
        lifecycle.start().unwrap();
        lifecycle.resume().unwrap();
        lifecycle
    }

    #[test]
    fn defaults_are_fresh_and_attached() {
        let lifecycle = resumed_lifecycle();
        let ctx = DefaultComponentContext::new(lifecycle.clone());
        assert_eq!(ctx.lifecycle().state(), LifecycleState::Resumed);
        // Fresh collaborators.
        assert_eq!(ctx.state_store().provider_count().unwrap(), 0);
        assert_eq!(ctx.state_store().pending_count().unwrap(), 0);
        assert!(!ctx.instance_store().is_destroyed().unwrap());
        assert_eq!(ctx.instance_store().len().unwrap(), 0);
        assert_eq!(ctx.back_handler().handler_count(), 0);
        // The fresh instance keeper is attached: root destroy ends it.
        lifecycle.destroy().unwrap();
        assert!(ctx.instance_store().is_destroyed().unwrap());
    }

    #[test]
    fn supplied_values_used_verbatim() {
        let lifecycle = resumed_lifecycle();
        let state = StateStore::from(StateKeeper::new());
        let instances = InstanceStore::from(InstanceKeeper::new());
        let back = SharedBack::new();
        let ctx = DefaultComponentContext::with_stores(
            lifecycle.clone(),
            state.clone(),
            instances.clone(),
            back.clone(),
        );
        assert!(Rc::ptr_eq(&ctx.state_store().inner, &state.inner));
        assert!(Rc::ptr_eq(&ctx.instance_store().inner, &instances.inner));
        // Back handles share one dispatcher: registering through either is
        // visible through the other.
        ctx.back_handler().register(0, true, |_, _| {}).unwrap();
        assert_eq!(back.handler_count(), 1);
    }

    #[test]
    fn external_keeper_gets_no_destroy_hook() {
        let lifecycle = resumed_lifecycle();
        let instances = InstanceStore::from(InstanceKeeper::new());
        let ctx =
            DefaultComponentContext::with_instance_store(lifecycle.clone(), instances.clone());
        lifecycle.destroy().unwrap();
        // Ownership stays with the caller: no hook installed.
        assert!(!instances.is_destroyed().unwrap());
        let _ = ctx;
    }

    #[test]
    fn dropping_context_alone_destroys_nothing() {
        let lifecycle = resumed_lifecycle();
        let ctx = DefaultComponentContext::new(lifecycle);
        let keeper = ctx.instance_store();
        drop(ctx);
        assert!(!keeper.is_destroyed().unwrap());
    }

    #[test]
    fn factory_produces_detached_contexts() {
        let parent = DefaultComponentContext::new(resumed_lifecycle());
        let factory = parent.context_factory();
        let other = DefaultComponentContext::new(resumed_lifecycle());
        let sibling = factory(ContextParts::new(
            other.lifecycle(),
            other.state_store().inner,
            other.instance_store().inner,
            other.back_handler(),
        ));
        // The factory assembled exactly the given parts (no parent link).
        assert!(Rc::ptr_eq(
            &sibling.state_store().inner,
            &other.state_store().inner
        ));
        assert!(Rc::ptr_eq(
            &sibling.instance_store().inner,
            &other.instance_store().inner
        ));
        drop(parent);
        assert_eq!(sibling.lifecycle().state(), LifecycleState::Resumed);
    }

    #[test]
    fn empty_key_is_legal_if_unique() {
        let parent = DefaultComponentContext::new(resumed_lifecycle());
        let child = parent.child_context("").unwrap();
        assert_eq!(child.lifecycle().state(), LifecycleState::Resumed);
        // ...but still unique within the parent.
        let err = parent.child_context("").unwrap_err();
        assert_eq!(err, DecomposeError::DuplicateKey { key: String::new() });
        assert!(err.to_string().contains("''")); // the key appears in the message
    }

    // -- Custom service-bearing contexts (no Clone bound) ------------------

    #[derive(Debug)]
    struct Logger {
        name: String,
    }

    /// Service-bearing custom context. `Clone` is NOT implemented and NOT
    /// required: shared ownership uses `Rc<AppContext>`, never `C: Clone`.
    #[derive(Debug)]
    struct AppContext {
        delegate: DefaultComponentContext,
        logger: Rc<Logger>,
        generation: u32,
    }

    /// Genuinely non-`Clone` service: `Box<dyn FnMut>` has no `Clone` impl,
    /// so `AuditLog` (and therefore `SessionContext`) can never be `Clone`.
    struct AuditLog {
        entries: Rc<RefCell<Vec<String>>>,
        #[allow(dead_code)]
        on_event: Box<dyn FnMut(u64)>,
    }

    impl AuditLog {
        fn new() -> Self {
            Self {
                entries: Rc::new(RefCell::new(Vec::new())),
                on_event: Box::new(|_| {}),
            }
        }
    }

    /// Third-generation custom context holding the non-`Clone` service.
    struct SessionContext {
        delegate: AppContext,
        audit: AuditLog,
    }

    impl ComponentContext for AppContext {
        fn lifecycle(&self) -> LifecycleRegistry {
            self.delegate.lifecycle()
        }
        fn state_store(&self) -> crate::StateStore {
            self.delegate.state_store()
        }
        fn instance_store(&self) -> crate::InstanceStore {
            self.delegate.instance_store()
        }
        fn back_handler(&self) -> SharedBack {
            self.delegate.back_handler()
        }
        fn context_factory(&self) -> ContextFactory<Self> {
            let base = self.delegate.context_factory();
            let logger = Rc::clone(&self.logger);
            let generation = self.generation + 1;
            Rc::new(move |parts| Self {
                delegate: base(parts),
                logger: Rc::clone(&logger),
                generation,
            })
        }
    }

    impl ComponentContext for SessionContext {
        fn lifecycle(&self) -> LifecycleRegistry {
            self.delegate.lifecycle()
        }
        fn state_store(&self) -> crate::StateStore {
            self.delegate.state_store()
        }
        fn instance_store(&self) -> crate::InstanceStore {
            self.delegate.instance_store()
        }
        fn back_handler(&self) -> SharedBack {
            self.delegate.back_handler()
        }
        fn context_factory(&self) -> ContextFactory<Self> {
            let base = self.delegate.context_factory();
            Rc::new(move |parts| Self {
                delegate: base(parts),
                audit: AuditLog::new(),
            })
        }
    }

    /// Navigation-style generic helper: proves the trait needs no `Clone`
    /// bound (this compiles for non-`Clone` `C`).
    fn generic_child<C: ComponentContext>(ctx: &C, key: &str) -> Result<C, DecomposeError> {
        ctx.child_context(key)
    }

    #[test]
    fn three_nested_custom_generations_preserve_services_without_clone() {
        let logger = Rc::new(Logger {
            name: "root".to_owned(),
        });
        let root = DefaultComponentContext::new(resumed_lifecycle());
        let app = AppContext {
            delegate: root,
            logger: Rc::clone(&logger),
            generation: 0,
        };

        // Generation 1 through the generic helper.
        let child = generic_child(&app, "child").unwrap();
        assert_eq!(child.generation, 1);
        assert!(Rc::ptr_eq(&child.logger, &logger));
        assert_eq!(child.logger.name, "root");

        // Generation 2, stored navigation-style behind Rc (no Clone needed).
        let child_ref = Rc::new(child);
        let grandchild = generic_child(&*child_ref, "grandchild").unwrap();
        assert_eq!(grandchild.generation, 2);
        assert!(Rc::ptr_eq(&grandchild.logger, &logger));

        // Generation 3 with a genuinely non-Clone service.
        let session = SessionContext {
            delegate: grandchild,
            audit: AuditLog::new(),
        };
        let leaf = generic_child(&session, "leaf").unwrap();
        assert_eq!(leaf.delegate.generation, 3);
        assert!(Rc::ptr_eq(&leaf.delegate.logger, &logger));
        leaf.audit.entries.borrow_mut().push("event".to_owned());
        assert_eq!(leaf.audit.entries.borrow().len(), 1);

        // Dropping all ancestors leaves descendants fully usable: no
        // stored-factory cycle exists (factories are computed per call).
        drop(session);
        drop(child_ref);
        drop(app);
        assert_eq!(leaf.lifecycle().state(), LifecycleState::Resumed);
        assert!(leaf.delegate.delegate.state_store().save().is_ok());
    }
}
