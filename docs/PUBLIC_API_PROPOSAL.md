# Public API Proposal (design proposal, not committed API)

All signatures below are proposals to be tested against the ergonomics examples in §8.
Decisions reference ADRs in `docs/adr/`.

## 1. `ComponentContext` representation: trait + concrete struct (ADR-0001: DECIDED)

```rust
pub trait ComponentContext: Sized {
    fn from_parts(parts: ContextParts) -> Self;
    fn child_context(&self, key: &str) -> Result<Self, DecomposeError>;
    fn child_context_with_lifecycle(
        &self,
        key: &str,
        lifecycle: &LifecycleRegistry,
    ) -> Result<Self, DecomposeError>;
    // ... lifecycle()/state_keeper()/instance_keeper()/back_handler() accessors ...
}
```

- Trait (not bare struct): preserves upstream's mandatory custom-context capability via
  generics (`fn navigate<C: ComponentContext>(ctx: &C)`), with zero casts and no global type.
- **No `Clone` bound** (Phase 0.5, prototype-validated): a custom context may hold
  non-`Clone` services/state. Shared ownership uses `Rc<C>`; the clonable factory
  (`ContextFactory<C>`, an `Rc` closure) is what gets cloned, not `C`.
- Concrete `DefaultComponentContext` struct ships in-core and implements `Clone` itself as
  a shared-handle convenience (clones share one scope).
- Rejected: bare-struct-only (kills custom contexts), `dyn`-compatible trait (impossible
  with `Self`-returning factory; navigation is generic, so `dyn` is unnecessary),
  Kotlin-style F-bound generics (no Rust equivalent worth the complexity), `C: Clone`
  bound (proven unnecessary; would forbid legitimate services).

## 2. Factory representation: `Rc<dyn Fn(...) -> C>` (ADR-0002: DECIDED)

```rust
pub type ContextFactory<C> = Rc<dyn Fn(ContextParts) -> C>;

pub struct ContextParts {
    lifecycle: LifecycleRegistry,
    state: SharedKeeper,
    instances: SharedInstances,
    back: SharedBack,
    scope: ContextScope,
}

impl ContextParts {
    // No public constructor and no public fields. Downstream code can
    // inspect/clone the four usable handles, but cannot construct
    // ContextParts manually and cannot access ContextScope. The canonical
    // construction path consumes the original ContextParts through
    // C::from_parts(parts) or a delegating base factory.
    pub fn lifecycle(&self) -> &LifecycleRegistry;
    pub fn state_keeper(&self) -> SharedKeeper;
    pub fn instance_keeper(&self) -> SharedInstances;
    pub fn back_handler(&self) -> SharedBack;
}
```

Closures first; introduce a `factory trait` only if a future navigation generic demands
named-type factories (no evidence now). The factory is explicitly **detached** (same
contract as upstream): scoping inputs are prepared by `child_context`/engine, the factory
only assembles — and it must forward the private `scope` (by constructing through
`from_parts` or a delegating base factory) so the child cannot silently detach. Because
retained provider closures must be `'static`, the public boundary takes `&str` keys but
everything captured is an owned `String` (compiler-enforced).

```rust
let base = base_factory.clone();

let factory: ContextFactory<AppContext> = Rc::new(move |parts| {
    AppContext {
        delegate: base(parts),
        logger: logger.clone(),
    }
});
```

## 3. Key model (DECIDED)

```rust
pub struct ChildKey(String); // owned; unique-within-parent only — "" is allowed
```

- Public key type is an owned `String` wrapper (callers pass `&str`; storage is owned).
  Upstream requires only *uniqueness within the parent scope* (verified: `ChildStateKeeper`
  checks `isRegistered` only; no length/content validation), so Rust adds no non-empty or
  format restriction. `""` is legal if unique.
- Nested namespaces are *positional composition*, not string concatenation: each scope holds
  its own keeper/dispatcher; the child key indexes the parent map. Sibling scopes may reuse
  identical internal keys. Deterministic: keys come from callers or engine counters —
  **never UUIDs/random** in core (upstream `randomKey()` is an engine detail deferred to Phase 2,
  where it becomes a deterministic per-collection counter + caller key).
- Duplicate parent-level key → `Err(DecomposeError::DuplicateKey { key })`. No panics.

## 4. Error model (DECIDED)

```rust
pub enum DecomposeError {
    DuplicateKey { key: String },
    ChildLifecycleDestroyed,          // supplied lifecycle already destroyed at creation
    ContextDestroyed,                  // operation on a destroyed scope
    Lifecycle(LifecycleError),         // transparent Essenty passthrough (#[from])
    StateKeeper(StateKeeperError),
    InstanceKeeper(InstanceKeeperError),
    TypeMismatch { key: String },      // retained-type conflicts
}
```

- `Result` + typed error everywhere a caller can recover (duplicate keys, destroyed inputs
  detectable *before* side effects, consumed-state decode, type mismatches).
  `thiserror`-style, mirroring Essenty-rs errors.
- Programmer-contract violations that surface *through Essenty-owned calls* are panics, not
  `Result`s — Decompose cannot change `LifecycleRegistry::destroy()`'s signature:
  - **Manual destruction of an attached child lifecycle panics.** `child_context` registers
    a destroy observer (mirroring upstream `doOnDestroy { error(...) }`, verified to fire
    synchronously inside `destroy()`); destroying the supplied manual registry afterwards
    panics with `"The lifecycle of a child ComponentContext must never be destroyed
    manually."` Documented with a `# Panics` section on `child_context_with_lifecycle`;
    covered by a `catch_unwind` parity test. (Essenty-rs sets state to `Destroyed` before
    notifying, so the registry still reports destroyed after the panic; post-panic cleanup
    of sibling hooks is deliberately unspecified — programmer-error path only.)
  - **Creation-time detection stays recoverable:** if the supplied lifecycle is *already*
    destroyed when `child_context_with_lifecycle` is called, return
    `Err(ChildLifecycleDestroyed)` *before* any registration side effect. (Deliberate,
    documented divergence: upstream throws at creation too, as an artifact of immediate
    `doOnDestroy`; Rust recovers because nothing has happened yet. Post-creation destroy
    still panics.)
- No `Option`-swallowing: absent restored state is `Ok(None)` at consume sites, never silent default.
- Do NOT invent a wrapper lifecycle type just to `Result`-ify the manual-destroy path.

## 5. Observable state direction (DEFERRED WITH REASON)

Navigation will need `Value`-equivalent observation. Decision deferred to Phase 2 *by design*:
`ComponentContext` itself does not need it, and the choice (callback/subscription observable
vs `Rc`-backed `Value<T>` vs external abstraction) must be validated against the generic
`children()` engine, not guessed now. Constraints recorded for Phase 2:

- Core remains runtime-agnostic: **no Tokio `watch`, no futures, no `Send` bounds**.
- Required semantics: current-value read + subscribe-with-synchronous-replay + RAII
  cancellation + lifecycle-aware subscribe + reentrancy-safe emission (upstream `MutableValue`
  contract). A `map`/derived combinator is required (cached by identity).
- Leading candidate: `Rc`-backed `Value<T>`/`MutableValue<T>` port (single-threaded,
  `RefCell`-guarded, CAS-by-identity via `Rc::ptr_eq`), because navigation diffing and tests
  (`by fooComponent::stack`-style ergonomics) assume exactly these semantics.

## 6. Serialization boundary (DEFERRED WITH REASON)

```text
Decompose navigation  →  caller-supplied codec (encode/decode fns)  →  Essenty opaque bytes
```

- Decompose **must not** depend on `serde` in core (not even as optional default): Essenty-rs
  already takes explicit `encode`/`decode` closures, so navigation persistence is expressed as
  `save: Fn() -> Vec<u8>` / `restore: Fn(&[u8]) -> Option<T>` pairs (plus a `NavStateSaver`
  equivalent). `serde_json`/`postcard` appear only in tests and downstream crates.
- Configurations need only `Clone + Eq` for transient navigation; persistence is opt-in per
  `children()` call (`None` codec = transient only, mirroring upstream `null` serializer).
- Revisit in Phase 6 (state-restoration refinement) only if evidence shows callers need more.

## 7. Crate structure (DECIDED)

```toml
# Phase 1: exactly one crate
[package]
name = "decompose"   # see REPOSITORY_STRATEGY.md: blocked by crates.io collision → use `decompose-rs`
```

- One `decompose-rs` crate for all of Phases 1–5 (context + engine + stack/slot/pages/panels/items).
  Crate explosion is rejected: navigation families share the engine and splitting them buys
  nothing but version-drift.
- Later candidates, each requiring fresh justification: `decompose-test-utils` (only when
  out-of-crate reuse is proven), `decompose-web` (browser-history binding),
  `decompose-<ui>` per integration. None created in Phase 1.
- Deps: the four `essenty-* 0.1.0` primitives only. Dev-deps: `serde_json` (test codecs) or similar.

## 8. Ergonomics examples (proposal sketches)

Root component:

```rust
struct RootComponent<C: ComponentContext> { _ctx: C }

impl<C: ComponentContext> RootComponent<C> {
    fn new(ctx: C) -> Self {
        let _child: C = ctx.child_context("child").expect("unique key");
        Self { _ctx: ctx }
    }
}
```

Custom context (extra service preserved through navigation; `Clone` NOT required —
a context may hold non-`Clone` per-scope state such as subscription guards):

```rust
struct AppContext { delegate: DefaultComponentContext, logger: Rc<Logger> }

impl ComponentContext for AppContext { /* delegate + closed-over logger factory */ }
```

`child_context` takes `&self` (shared handles + short `RefCell` borrows; never held
across observer delivery, back dispatch, user callbacks, or factory invocation —
prototype-validated for nested and observer-driven creation).

Fixed child:

```rust
let child = parent.child_context("details")?;
```

Manually-controlled lifecycle child:

```rust
let manual = LifecycleRegistry::new();
let child = parent.child_context_with_lifecycle("modal", &manual)?;
manual.resume().unwrap();   // merged lifecycle advances only with the parent
// manual.destroy() → PANICS: destroying an attached child lifecycle is forbidden
// (see §4 error model for the # Panics contract).
```

## 9. API stability strategy (first release `0.1.x`)

- Public immediately: `ComponentContext` trait, `DefaultComponentContext`,
  `ContextFactory`/`ContextParts`, `ChildKey`, `DecomposeError`, `MergedLifecycle`,
  child-scope constructors, `DEFAULT_BACK_PRIORITY`.
- Crate-private: `ContextScope` (private fields, travels inside `ContextParts`),
  `ChildStateKeeper`/`ChildInstanceKeeper`/`ChildBackHandler` adapter
  internals (exposed only through `child_context` behavior + `pub(crate)` items),
  detached-subscription ids, enabled-listener tokens.
- Experimental (feature-gated or clearly marked): back-priority override overload,
  predictive-back forwarding surface.
- Waits for navigation: everything observable (`Value`), codecs, `children()` engine,
  `Child.{Created,Destroyed}`.
