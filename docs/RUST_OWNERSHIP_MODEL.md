# Rust Ownership Model

> Historical Phase 1 research. Implemented state-adapter and retention-lifetime
> corrections in [ADR-0005](adr/0005-stack-and-retention-corrections.md) supersede conflicting statements here.

## 1. Baseline

- Rust 1.96.0, Edition 2024 (matches Essenty-rs; no toolchain exception needed).
- Single-threaded core: **no `Send`/`Sync`/`Arc`/`Mutex`/Tokio bounds** on any component-tree
  type, mirroring Essenty-rs (`Rc`/`RefCell`, `!Send + !Sync`). Thread-safe variants, if ever
  wanted, are future work *outside* core (new types, not new bounds).
- Synchronous semantics stay synchronous. No async in core.

## 2. Ownership graph (Phase 0.5 revision)

Lifetime comes from **parent-held strong captures** (mirroring upstream's GC graph), not
from RAII guards in context structs. Context structs are views: dropping one detaches
nothing. All teardown is lifecycle-driven. See ADR-0003.

| Object | Representation | Why |
|---|---|---|
| `LifecycleRegistry` (parent, child, merged) | Essenty shared handle (`Clone` shares state; pass by value/`&`) | Cheap shared handles; observed from multiple scopes |
| `MergedLifecycle` | Owned struct holding the merged `LifecycleRegistry`; its parent observers live **in the parent registries** (self-sustaining), removed by id from the teardown hook (or self-removed after driving merged to `Destroyed`) | Dropping the struct must NOT unsubscribe parents (upstream-faithful: observers outlive the view) |
| Parent `ComponentContext` | Borrowed (`&C`) at `child_context`; engine holds `Rc<C>` where shared ownership is needed | `child_context(&self)`: keepers/dispatchers behind `Rc<RefCell<…>>`, mutated via short `borrow_mut()`; `C: Clone` never required |
| Child `ComponentContext` | Owned value `C` containing the four `Rc`-shared child handles | Move semantics; `Clone` only if the context's own design wants it. No separate scope struct (REDUNDANT — the handles are the scope; lifetime is parent-held) |
| Back access | `SharedBack` handle only (ADR-0004); raw `RefCell<BackDispatcher>` borrowing is crate-private | Uniform busy window + FIFO queue; registration during dispatch rejected |
| `StateKeeper` (parent side) | `Rc<RefCell<StateKeeper>>`; `consume` + `register` via short `borrow_mut()` inside `child_context(&self)` | `&mut`-at-birth rejected: `RefCell` gives the same sequencing with upstream-like `&self` ergonomics |
| Child `StateKeeper` dispatcher | `Rc<RefCell<StateKeeper>>`; parent provider captures a **strong** `Rc` to it | Provider must save child state for the full registration lifetime with zero external refs (upstream `stateKeeper::save` is a strong bound reference); graph `parent → provider → child` has no back edge, so no cycle |
| `InstanceKeeper` (parent side) | `Rc<RefCell<InstanceKeeper>>`; child scope retained in parent under the child key (strong by construction) | Identity across recreation; removal on child-lifecycle destroy |
| `BackDispatcher` (parent side) | `SharedBack` handle; single parent callback captures a **strong** child `SharedBack` | Bridge must route for the full registration lifetime; all access through the busy-window API (ADR-0004) |
| Factory | `ContextFactory<C> = Rc<dyn Fn(ContextParts) -> C>`; `ContextParts` consumed by value with no public fields; whole-parts forwarding via base-factory delegation | Clonable, object-safe; custom contexts close over extra services; public boundary `&str` keys, owned `String` captures (`'static` providers are compiler-enforced) |
| Child state namespace | Parent provider entry owning the strong child-save closure (DCS1-framed bytes) | Released by lifecycle-destroy `unregister`, never by context drop; fixed children persist to parent-scope end |
| Child back bridge | Child `SharedBack` + parent handle id + gate cell + detached-subscription ids (teardown data, not RAII guards) | `stop()` = unregister parent callback; driven by the lifecycle hook |
| Subscriptions | Detached (`Subscription::detach`) + explicit removal by id in teardown hooks; destroy-contract observer is fire-and-forget | Parent registries own observer lifetime (like upstream); guards are never the lifetime mechanism |

Borrow discipline (executed in `/tmp/dc-proto-b` and `/tmp/dc-proto-c`: nested
three-level plus observer-driven creation complete with no borrow panics): never hold a
`RefCell` borrow across lifecycle observer delivery, back dispatch, user callbacks, or
factory invocation. Snapshot ids/keys into locals first; re-borrow inside each closure
invocation. Back-handler access additionally goes exclusively through `SharedBack`
(ADR-0004), which makes nested borrowing structurally impossible downstream.

## 3. Reference cycles and `Weak` usage

Exactly one genuine cycle exists: parent-callback→child→enabled-listener→parent.
It is broken by capturing a **`Weak<SharedBack>`** parent link in the child's
enabled-listener (upgrade per call; no-op when gone). Everything else is acyclic by
construction:

1. **State save closure**: STRONG `Rc` to the child keeper (Issue C). No back edge exists
   (child providers point strictly downward), so this is not a cycle. `Weak` here would be a
   semantic regression (verified against upstream's strong bound reference + prototype).
2. **Merged observers**: held by parent registries; capture merged registry + side-state.
   The transient parent→merged→parent retention is broken deterministically when merged
   reaches `Destroyed` (observers remove themselves/hook by id afterwards).
3. **Teardown-hook closures**: held by the manual lifecycle; capture parent handles strongly.
   No path leads back to the manual lifecycle, so no cycle.
4. **Factory closures** capture extra services (`Rc<Logger>`-style) — deliberate, bounded,
   cycle-free.

Teardown (lifecycle-driven, each step idempotent): manual-child-lifecycle destroy →
`unregister` state key → `destroy` instance scope entry → back-bridge `stop` (+ unregister
parent callback) → unsubscribe merged observers → merged registry destroys (cascading to
child observers). Destroy delivery in Essenty-rs runs in reverse subscription order, so the
first-subscribed destroy-contract guard fires *last*: functional cleanup hooks run first,
then the manual-destroy panic (F-03). A panic during observer delivery aborts the loop and
may skip observer clearing — panic tests use fresh fixtures and assert nothing about
post-panic cleanup. Context drop performs no parent-visible mutation. No Kotlin-Essenty
ordering parity is claimed (unverified).

## 4. Threading model

Default core is `!Send + !Sync`, exactly like Essenty-rs. Rationale: component trees live on
one UI thread on every target (Android main, iOS main, WASM single-thread); `Rc` keeps
WebAssembly viable and avoids `Arc<Mutex<…>>` taxation on every callback dispatch. No
public API takes or returns `Arc`, `Mutex`, futures, or Tokio types. If a host needs to move
state across threads it confines the tree to one thread (or wraps at its own layer) — core
never helps nor hinders. Optional `Send`-safe mirrors are documented future work and must
be new types, never retrofitted bounds (which would break every component constructor).

## 5. `no_std` decision: NOT A GOAL

All four Essenty-rs crates currently require `std` (only `alloc`/`core` containers used, so
`no_std + alloc` is "realistic" per their docs, but not implemented). Decompose-rs core will
use `std` types (`String`, `Vec`, `BTreeMap`, `Rc`) without apology. Classify as NOT A GOAL:
revisit only if Essenty-rs itself goes `no_std` first; never complicate the component-context
API for hypothetical bare-metal UI trees.
