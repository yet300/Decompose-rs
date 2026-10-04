# Public API Proposal (design proposal, not committed API)

All signatures below are proposals to be tested against the ergonomics examples in §8.
Decisions reference ADRs in `docs/adr/`.

## 1. `ComponentContext` representation: trait + concrete struct (ADR-0001: DECIDED)

```rust
pub trait ComponentContext: Sized + Clone {
    fn lifecycle(&self) -> &LifecycleHandle;
    fn state_keeper(&self) -> &SharedKeeper<StateKeeper>;
    // ... instance_keeper(), back_handler(), factory() ...
    fn child_context(&self, key: &str) -> Result<Self, DecomposeError>;
    fn child_context_with_lifecycle(&self, key: &str, lifecycle: &LifecycleRegistry)
        -> Result<Self, DecomposeError>;
}
```

- Trait (not bare struct): preserves upstream's mandatory custom-context capability via
  generics (`fn navigate<C: ComponentContext>(ctx: &C)`), with zero casts and no global type.
- Concrete `DefaultComponentContext` struct ships in-core as the default `C`.
- Rejected: bare-struct-only (kills custom contexts), `dyn`-compatible trait (impossible
  with `Self`-returning factory; navigation is generic, so `dyn` is unnecessary — validated
  by prototype), Kotlin-style F-bound generics (`GenericComponentContext<T>` has no Rust
  equivalent worth the complexity; a single trait + associated factory carries the same power).
- Object safety is a non-goal; genericity is the mechanism. Mockability comes from
  implementing the trait on test doubles, not from `dyn`.

## 2. Factory representation: `Rc<dyn Fn(...) -> C>` (ADR-0002: DECIDED)

```rust
pub type ContextFactory<C> = Rc<dyn Fn(ContextParts) -> C>;

pub struct ContextParts {
    pub lifecycle: LifecycleRegistry,      // already merged / child-scoped by the caller
    pub state_keeper: SharedKeeper<StateKeeper>,
    pub instance_keeper: SharedKeeper<InstanceKeeper>,
    pub back_handler: SharedDispatcher,
}
```

Closures first; introduce a `factory trait` only if a future navigation generic demands
named-type factories (no evidence now). The factory is explicitly **detached** (same
contract as upstream): scoping inputs are prepared by `child_context`/engine, the factory
only assembles. Custom contexts close over extra services:

```rust
let factory: ContextFactory<AppContext> = Rc::new(move |parts| {
    AppContext {
        delegate: (base_factory)(parts.clone()),
        logger: logger.clone(),
    }
});
```

## 3. Key model (DECIDED)

```rust
pub struct ChildKey(String); // validated, non-empty; Display + AsRef<str> + Debug
```

- Public key type is an owned `String` wrapper (callers pass `&str`; storage is owned).
  `&str` at boundaries, owned in maps — no wrapper proliferation, no lifetimes in context types.
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
    ChildLifecycleDestroyed,          // supplied lifecycle already destroyed at child_context
    ManualChildLifecycleDestroy,       // supplied child lifecycle destroyed while attached
    ContextDestroyed,                  // operation on a destroyed scope
    Lifecycle(LifecycleError),         // transparent Essenty passthrough (#[from])
    StateKeeper(StateKeeperError),
    InstanceKeeper(InstanceKeeperError),
    TypeMismatch { key: String },      // retained-type conflicts
}
```

- `Result` + typed error everywhere a caller can recover (duplicate keys, destroyed inputs,
  consumed-state decode, type mismatches). `thiserror`-style, mirroring Essenty-rs errors.
- Panic **only** for programmer-contract violations that upstream signals with
  `error(...)`: manual destruction of an attached child lifecycle. Even there, prefer
  returning `Err(ManualChildLifecycleDestroy)` at the destroy *call site* when detectable,
  and panic only on the asynchronous path where no `Result` can propagate (exact behavior
  pinned by parity tests in Phase 1).
- No `Option`-swallowing: absent restored state is `Ok(None)` at consume sites, never silent default.

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

Custom context (extra service preserved through navigation):

```rust
#[derive(Clone)]
struct AppContext { delegate: DefaultComponentContext, logger: Rc<Logger> }

impl ComponentContext for AppContext { /* delegate + closed-over logger factory */ }
```

Fixed child:

```rust
let child = parent.child_context("details")?;
```

Manually-controlled lifecycle child:

```rust
let manual = LifecycleRegistry::new();
let child = parent.child_context_with_lifecycle("modal", &manual)?;
manual.resume().unwrap();   // merged lifecycle advances only with the parent
// manual.destroy() → Err/panic: destroying an attached child lifecycle is forbidden
```

## 9. API stability strategy (first release `0.1.x`)

- Public immediately: `ComponentContext` trait, `DefaultComponentContext`,
  `ContextFactory`/`ContextParts`, `ChildKey`, `DecomposeError`, `MergedLifecycle`,
  child-scope constructors, `DEFAULT_BACK_PRIORITY`.
- Crate-private: `ChildStateKeeper`/`ChildInstanceKeeper`/`ChildBackHandler` adapter
  internals (exposed only through `child_context` behavior + `pub(crate)` items),
  subscription-guard fields, enabled-listener tokens.
- Experimental (feature-gated or clearly marked): back-priority override overload,
  predictive-back forwarding surface.
- Waits for navigation: everything observable (`Value`), codecs, `children()` engine,
  `Child.{Created,Destroyed}`.
