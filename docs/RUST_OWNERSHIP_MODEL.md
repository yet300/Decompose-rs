# Rust Ownership Model

## 1. Baseline

- Rust 1.96.0, Edition 2024 (matches Essenty-rs; no toolchain exception needed).
- Single-threaded core: **no `Send`/`Sync`/`Arc`/`Mutex`/Tokio bounds** on any component-tree
  type, mirroring Essenty-rs (`Rc`/`RefCell`, `!Send + !Sync`). Thread-safe variants, if ever
  wanted, are future work *outside* core (new types, not new bounds).
- Synchronous semantics stay synchronous. No async in core.

## 2. Ownership graph

| Object | Representation | Why |
|---|---|---|
| `LifecycleRegistry` (parent, child, merged) | `Rc<LifecycleRegistry>` shared handle (Essenty type is itself `Clone`-as-shared-handle) | Essenty registries are cheap shared handles; parent and merged lifecycles are observed from multiple scopes |
| `MergedLifecycle` struct | Owned value holding `merged: Rc<LifecycleRegistry>` + two `Subscription` guards + side-state `Rc<RefCell<…>>` | Dropping the struct unsubscribes both parents (RAII); no manual cleanup path |
| Parent `ComponentContext` | Borrowed (`&C`) at `child_context`; navigation engine holds `Rc<C>`-style shared ownership of scopes it creates | Creation is a borrow; long-lived trees are held by the engine/components, never by the factory |
| Child `ComponentContext` | Owned value `C` (contains `Rc` handles internally) | Cheap to move/clone; clone shares the same scope (documents that contexts are handles, not trees) |
| `StateKeeper` (parent side at construction) | `&mut StateKeeper` (exclusive borrow, short-lived) | `consume` + `register` require `&mut`; forces construction sequencing and makes duplicate-key errors `Result`s at the call site |
| Child `StateKeeper` dispatcher | `Rc<RefCell<StateKeeper>>` inside the child scope | `'static` save closures registered in the parent capture `Weak`/`Rc` to it; interior mutability is the documented cost of provider callbacks |
| `InstanceKeeper` (parent side) | `&mut InstanceKeeper` at construction; `Rc<RefCell<InstanceKeeper>>` for the live child scope | Same `&mut`-at-birth rationale; nested scope retained in parent under the child key for config-change identity |
| `BackDispatcher` (parent side) | `&mut BackDispatcher` at `register`; shared `Rc<RefCell<BackDispatcher>>` for live scopes | `register/unregister/set_enabled` take `&mut`; the child bridge holds the parent link to update the single parent callback |
| Factory | `Rc<dyn Fn(Lifecycle, StateKeeperSeed, InstanceScope, BackScope) -> C>` — i.e. `Rc`-shared closure ("factory trait" only if generics demand it; closures first) | Clonable, object-safe, zero new traits for the common case; custom contexts close over extra services (validated by prototype) |
| Child state namespace (parent key reservation) | Parent `StateKeeper` provider entry owning a `Weak`-upgraded child save closure | Parent never owns the child; child drop + lifecycle destroy both release the key |
| Child instance namespace | `ChildScope` value retained in parent keeper under the child key | Identity across recreation; removal on child-lifecycle destroy |
| Child back bridge | `ChildBackHandler` struct: child dispatcher + parent `BackHandle` id + gate flag + `Subscription` guards | Single parent callback; `stop()` unregisters; drop unregisters defensively |
| Subscriptions | Owned `Subscription` guards stored in the scope structs (`MergedLifecycle`, `ChildBackHandler`) | Drop = unsubscribe; leak = forgotten guard field, caught by `subscriber_count` assertions in tests |

## 3. Reference cycles and `Weak` usage

Cycles are possible wherever a `'static` closure registered in a parent captures a strong
handle to the child that owns the registration:

1. **State save closure**: parent provider `move \|\| child_keeper.borrow().save()` must capture
   `Weak<RefCell<StateKeeper>>`, upgrading per save; on upgrade failure return the last
   snapshot or `None` (via `register_optional`). Strong capture would keep a destroyed child
   alive inside the parent forever.
2. **Back enabled-listener**: `add_enabled_changed_listener` stores `Rc<dyn Fn(bool)>` in the
   child dispatcher; the listener updates the *parent* entry, so it must capture only the
   parent link (`Weak` or a raw handle id + `Rc<RefCell<BackDispatcher>>` to the parent),
   never the child scope itself.
3. **Lifecycle observers**: Essenty observers are `Rc<dyn Fn(LifecycleState)>` held by the
   registry; merged-side closures capture side-state cells and the merged registry `Rc`,
   never the parent registries. Guards (not closures) tie lifetimes: dropping the merged
   scope drops the guards, which removes the closures.
4. **Factory closures** capture extra services (`Rc<Logger>`-style) — a deliberate,
   bounded strong capture; services are expected to be long-lived and cycle-free.

Teardown paths (all RAII, no explicit close protocol except scope-destroy functions the
engine calls): child-lifecycle destroy → `unregister` state key + `remove` instance scope
+ back `stop()`; merged destroy → drop both `Subscription` guards; scope drop → defensive
`unregister`/`stop` if still registered (idempotent `bool` returns make double-teardown safe).

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
