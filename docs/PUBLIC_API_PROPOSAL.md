# Public API Proposal (design proposal, not committed API)

All signatures below are proposals to be tested against the ergonomics examples in §8.
Decisions reference ADRs in `docs/adr/`.

## 1. `ComponentContext` representation: trait + concrete struct (ADR-0001: DECIDED)

```rust
pub trait ComponentContext: Sized {
    fn lifecycle(&self) -> LifecycleRegistry;
    fn state_keeper(&self) -> SharedKeeper;
    fn instance_keeper(&self) -> SharedInstances;
    fn back_handler(&self) -> SharedBack;
    /// Upstream `componentContextFactory`, computed from &self per call.
    fn context_factory(&self) -> ContextFactory<Self>;

    /// Provided: prepare the child scope, then construct through the factory.
    fn child_context(&self, key: &str) -> Result<Self, DecomposeError>;
    fn child_context_with_lifecycle(
        &self,
        key: &str,
        lifecycle: &LifecycleRegistry,
    ) -> Result<Self, DecomposeError>;
}
```

- Trait (not bare struct): preserves upstream's mandatory custom-context capability via
  generics (`fn navigate<C: ComponentContext>(ctx: &C)`), with zero casts and no global type.
- **No `Clone` bound**: a custom context may hold non-`Clone` services/state. Shared
  ownership uses `Rc<C>`; the clonable factory (`ContextFactory<C>`, an `Rc` closure) is
  what gets cloned, not `C`. (Three nested service-bearing generations executed in
  `/tmp/dc-proto-c` without any `Clone` bound.)
- **No `from_parts` on the trait** (F-01): a generic constructor cannot synthesize custom
  services. `from_parts` exists only as an inherent constructor on concrete types
  (`DefaultComponentContext::from_parts`). The trait's fifth member is the factory —
  mirroring upstream's `componentContextFactory`, not a constructor.
- Concrete `DefaultComponentContext` struct ships in-core and implements `Clone` itself as
  a shared-handle convenience (clones share one scope).
- Rejected: bare-struct-only (kills custom contexts), `dyn`-compatible trait (impossible
  with `Self`-returning factory; navigation is generic, so `dyn` is unnecessary),
  Kotlin-style F-bound generics (no Rust equivalent worth the complexity), `C: Clone`
  bound (proven unnecessary; would forbid legitimate services), stored factory (invites
  self-referential cycles; computing per call costs one `Rc` allocation).

## 2. Factory representation: `Rc<dyn Fn(...) -> C>` (ADR-0002: DECIDED)

```rust
pub type ContextFactory<C> = Rc<dyn Fn(ContextParts) -> C>;

pub struct ContextParts {
    lifecycle: LifecycleRegistry,
    state: SharedKeeper,
    instances: SharedInstances,
    back: SharedBack,
}

impl ContextParts {
    // No public constructor and no public fields. Downstream code can
    // inspect/clone the four usable handles, but cannot construct
    // ContextParts manually. The canonical construction path consumes the
    // original ContextParts through a base factory call or a concrete
    // inherent from_parts(parts). (No ContextScope struct: REDUNDANT per
    // ADR-0002 — the four handles are the scope; lifetime is parent-held.)
    pub fn lifecycle(&self) -> &LifecycleRegistry;
    pub fn state_keeper(&self) -> SharedKeeper;
    pub fn instance_keeper(&self) -> SharedInstances;
    pub fn back_handler(&self) -> SharedBack;
}
```

Closures first; introduce a `factory trait` only if a future navigation generic demands
named-type factories (no evidence now). The factory is explicitly **detached** (same
contract as upstream): scoping inputs are prepared by `child_context`/engine, the factory
only assembles. Because retained provider closures must be `'static`, the public boundary
takes `&str` keys but everything captured is an owned `String` (compiler-enforced).

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
pub(crate) struct ChildKey(String); // owned; unique-within-parent only — "" is allowed
```

- Phase-1 child APIs accept `&str`; no Phase-1 external API consumes `ChildKey`, so it
  stays **crate-private** (F-10 — no unused public vocabulary; revisit in Phase 2 when
  engine-generated keys exist). Internally it is an owned `String` wrapper.
- Upstream requires only *uniqueness within the parent scope* (verified: `ChildStateKeeper`
  checks `isRegistered` only; no length/content validation), so Rust adds no non-empty or
  format restriction. `""` is legal if unique.
- Nested namespaces are *positional composition*, not string concatenation: each scope holds
  its own keeper/dispatcher; the child key indexes the parent map. Sibling scopes may reuse
  identical internal keys. Deterministic: keys come from callers or engine counters —
  **never UUIDs/random** in core (upstream `randomKey()` is an engine detail deferred to Phase 2,
  where it becomes a deterministic per-collection counter + caller key).
- Duplicate parent-level key → `Err(DecomposeError::DuplicateKey { key })`. No panics.

## 4. Error model (DECIDED; minimized per F-09)

```rust
pub enum DecomposeError {
    DuplicateKey { key: String },
    ChildLifecycleDestroyed,          // supplied lifecycle already destroyed at creation
    BackDispatchInProgress,           // register requested while that dispatcher is dispatching
    Lifecycle(LifecycleError),         // transparent Essenty passthrough (#[from])
    StateKeeper(StateKeeperError),
    InstanceKeeper(InstanceKeeperError),
}
```

Only variants with a real Phase-1 producing API exist. Removed as speculative:
`ContextDestroyed` (child creation on a destroyed parent is *allowed* — the merged scope
is simply born destroyed with no subscriptions, mirroring upstream; no error path
produces it) and `TypeMismatch` (child-scope construction uses fixed internal types with
Decompose-namespaced keys, so no caller-observable mismatch path exists in Phase 1;
genuine keeper mismatches still surface through the transparent
`InstanceKeeper(InstanceKeeperError)` passthrough). Framing decode failures surface as
`StateKeeper(StateKeeperError::Decode { .. })` — no new variant.
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
executed for nested and observer-driven creation in `/tmp/dc-proto-b` and
`/tmp/dc-proto-c` with no borrow panics).

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
  `ContextFactory`/`ContextParts` (no public fields), `SharedBack`, `DecomposeError`,
  `MergedLifecycle`, child-scope constructors, `DEFAULT_BACK_PRIORITY`.
- Crate-private: `ChildKey`, adapter internals (exposed only through `child_context`
  behavior + `pub(crate)` items), detached-subscription ids, enabled-listener tokens,
  framing codec.
- Experimental (feature-gated or clearly marked): back-priority override overload,
  predictive-back forwarding surface.
- Waits for navigation: everything observable (`Value`), codecs, `children()` engine,
  `Child.{Created,Destroyed}`.

## 10. Shared back-handler access (ADR-0004: DECIDED)

`ComponentContext::back_handler()` returns `SharedBack` — an `Rc`-backed (`Clone`,
`!Send + !Sync`, no `Arc`/`Mutex`) reentrancy-safe handle. Raw `Rc<RefCell<BackDispatcher>>`
never appears in public API: Essenty holds a mutable borrow across synchronous callback
dispatch, so any nested borrow from inside a callback panics.

- Uniform busy window: outermost entry runs its operation then drains a FIFO pending
  queue; nested entries only enqueue. Panic guard resets depth on unwind without draining.
- Queued when busy: `set_enabled`, `set_priority`, `unregister`, listener add/remove,
  same-dispatcher reentrant `back`/predictive (returns `false`, applied in order).
- Rejected with `Err(DispatchInProgress)`: new-callback `register` during dispatch
  (no id reservation exists for queued registration; engine never needs it).
- `can_handle()`: fresh read when idle, maintained cached aggregate when busy.
- Bridge use (normative): plain `register` (not `register_reentrant` — `BackCommand` has
  no priority op and covers no cross-dispatcher listener hazard); strong child capture;
  `Weak` parent link in the enabled-listener; gate updates through wrapper methods.
- In-handler rule (F-11): idle mutations apply synchronously and are immediately
  observable; in-callback same-dispatcher mutation MUST use `SharedBack` methods. All
  nine F-02 cases plus the contract executed in `/tmp/dc-proto-c`.

## 11. Internal child-state framing (F-05: DECIDED, no ADR — versioned, cheap to evolve)

`child StateKeeper::save()` yields `BTreeMap<String, Vec<u8>>`; the parent provider needs
one `Vec<u8>`. The internal `DCS1` container (not a public serializer; navigation codecs
layer above it in Phase 6):

```text
magic "DCS1" (4 bytes) | entry_count u32 LE
per entry: key_len u32 LE | key UTF-8 bytes | value_len u32 LE | value bytes
stable BTreeMap order; no serde dependency
```

Decode failures: bad magic / truncation (all prefix lengths) / trailing bytes / invalid
UTF-8 keys / duplicate encoded keys → typed decode error surfacing as
`StateKeeperError::Decode`; `u32` lengths with checked cursor arithmetic (huge prefixes
error, never panic). Empty map, empty keys, and arbitrary binary payloads are legal;
nesting works by framing a frame. Eight coverage cases (incl. overflow) executed in
`/tmp/dc-proto-c`.

## 12. Construction order, rollback, root attachment, merged driver (DECIDED)

Normative `child_context` sequence (F-06):

```text
1. validate supplied lifecycle state (destroyed → Err(ChildLifecycleDestroyed), no effects)
2. duplicate state-key check BEFORE consuming restored bytes
3. consume restored state
4. create/register child state keeper (DCS1 provider)
5. obtain retained child InstanceKeeper via get_or_create (record pre-existence)
6. create/register back bridge
7. merged lifecycle + subscriptions in dependency order
8. build ContextParts
9. invoke custom context factory LAST
```

- Recoverable failure (`DuplicateKey` at step 2): nothing consumed, created, registered,
  or subscribed — explicit atomicity test.
- Factory panic (step 9): option A, temporary rollback guard (executed in `/tmp/dc-proto-c`).
  The guard owns exactly: state-key registration, back-callback registration, merged-sub
  ids, and the instance entry *iff created by this attempt* (pre-existence recorded at
  step 5). A pre-existing retained scope is never removed. Guard `Drop` is infallible
  (`try_borrow_mut`, no unwraps); success defuses it.
- Root `attachTo` (F-07): fresh default `InstanceKeeper` + one detached destroy
  subscription calling `destroy_all()` — lifetime independent of any temporary guard, no
  context-drop teardown. Supplied external keepers get no hook. Tests: root destroy ends
  the default keeper; external keeper untouched.
- Merged driver (F-08): `Initialized → Destroyed` emits `Created` then `Destroyed`
  (via `create(); destroy()`, matching the upstream observable sequence — bare Essenty
  `destroy()` would skip `Created`); `AlreadyDestroyed` from race/reentrant paths is
  swallowed as idempotent success.
