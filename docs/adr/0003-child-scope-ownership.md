# ADR-0003: Child-scope ownership (parent-held lifetime, lifecycle-driven teardown)

Date: 2026-10-04. Rewritten: 2026-10-04 (Phase 0.5).
Upstream: `arkivanov/Decompose@00840bc`. Status: **Accepted**.

## Context

Upstream attaches child scopes via `StateKeeper.child` / `InstanceKeeper.child` /
`BackHandler.child` backed by GC + lifecycle hooks. The context object itself is a *view*:
dropping it detaches nothing — parent-held closures (`stateKeeper::save` bound reference,
merged observers, the single parent back callback) keep every dispatcher alive, and
lifecycle-destroy hooks perform all teardown. A first-draft Rust design (RAII guards owned
by the context struct + `Weak` provider links) got this backwards: dropping the child
struct would silently detach the scope, and `Weak` providers would lose child state while
still registered. Both were disproven by source re-audit and prototype.

## Decision

**Lifetime ownership mirrors the upstream GC graph with parent-held strong
captures; teardown is lifecycle-driven, never drop-driven.**

- `ContextScope` (private fields, stored in every context, travels inside `ContextParts`)
  is a plain handle bundle: merged-or-shared lifecycle + child keeper/dispatcher `Rc`s +
  keys + parent links. It owns **no RAII teardown guards** and implements **no `Drop`
  teardown**: dropping a context detaches nothing (upstream-faithful).
- True lifetime ownership (each edge kept alive by its parent-side holder):
  1. **State provider** (in parent keeper) captures a **strong** `Rc` to the child keeper.
     Graph `parent keeper → provider → child keeper` has no back edge (child providers
     point strictly downward to grandchildren), so no cycle. Keeper stays saveable for the
     full registration lifetime, with zero external handles alive — verified by re-audit
     (`supplier = stateKeeper::save` is a strong bound reference) and prototype.
  2. **Merged observers** live in the parent registries and capture the merged registry
     strongly; they unsubscribe by id from the teardown hook (or self-remove after driving
     merged to `Destroyed`, breaking the transient parent→merged→parent retention loop
     deterministically on the destroy path).
  3. **Back bridge**: the single parent callback captures a **strong** `Rc` to the child
     dispatcher; the child's enabled-listener captures a **`Weak`** parent link (the one
     genuine cycle: parent-callback→child→listener→parent), upgrading per call.
  4. **Instance scope** is retained in the parent keeper under the child key (strong by
     construction).
  5. **Destroy-contract + cleanup subscriptions** on manual lifecycles are `detach()`ed
     (fire-and-forget, like upstream's ignored `Cancellable`); the cleanup hook carries
     detached ids and removes itself plus all of the above when it fires.
- Teardown order on manual-child-lifecycle destroy (each step idempotent):
  `unregister` state provider → `remove` instance scope → back-bridge `stop` (+unregister
  parent callback) → unsubscribe merged observers → merged registry destroys (cascading to
  child observers). Fixed children (shared parent lifecycle) keep the provider until the
  parent scope ends — matching upstream. `ContextScope` drop does none of this.
- `child_context(&self)` (not `&mut self`): parent keepers/dispatchers are shared as
  `Rc<RefCell<…>>`; construction uses short-lived `borrow_mut()` calls that never cross
  observer delivery, back dispatch, user callbacks, or factory invocation (prototype
  covers nested three-level + observer-driven creation without borrow panics).

## Analysis

- Rejected (guards-in-context + `Weak` provider): early context drop detaches merged
  following and leaks registrations; `Weak` loses saveable state while registered. Both
  failure modes demonstrated before the rewrite.
- Rejected (`Arc<Mutex>` / `Send + Sync`): taxes every callback, kills WASM, contradicts
  Essenty-rs and upstream's single-threaded UI-tree assumption.
- Accepted difference: retained-value `Drop` runs at last-`Rc` release, not
  deterministically at scope end (upstream `onDestroy`); scope *bookkeeping* (destroyed
  flags, unregistration) stays immediate — only value dropping is ownership-driven.
- `try_borrow_mut` is not needed on hot paths given the discipline above; `Drop` impls
  perform no parent-visible mutation, so drop-time reentrancy cannot occur.

## Consequences

Phase 1 structs follow this graph exactly. Mandatory parity tests: scope-survival after
factory return; strong-provider save with zero external child refs (test specified below); teardown
ordering; reentrant creation; `subscriber_count`/`handler_count`/`provider_count` leak
assertions. No Essenty changes required.

Mandatory Phase 1 ownership test: create child state scope; drop all ordinary external
child handles not semantically required; save parent while the namespace is registered;
assert child state is present (and round-trips through restore).
