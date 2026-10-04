# ADR-0003: Child-scope ownership (parent-held lifetime, lifecycle-driven teardown)

Date: 2026-10-04. Rewritten: 2026-10-04 (Phase 0.5).
Amended: 2026-10-04 (Phase 0.6: `SharedBack` replaces raw `Rc<RefCell<BackDispatcher>>`;
`get_or_create` normative; destroy-ordering corrected; root `attachTo` specified;
merged-driver rules; no `ContextScope` struct).
Upstream: `arkivanov/Decompose@00840bc`. Status: **Accepted**.

## Context

Upstream attaches child scopes via `StateKeeper.child` / `InstanceKeeper.child` /
`BackHandler.child` backed by GC + lifecycle hooks. The context object itself is a *view*:
dropping it detaches nothing — parent-held closures (`stateKeeper::save` bound reference,
merged observers, the single parent back callback) keep every dispatcher alive, and
lifecycle-destroy hooks perform all teardown.

## Decision

**Lifetime ownership mirrors the upstream GC graph with parent-held strong
captures; teardown is lifecycle-driven, never drop-driven. There is no `ContextScope`
struct (Phase 0.6/F-01 classification: REDUNDANT — the four child handles, held by the
context struct itself, *are* the scope).**

True lifetime ownership (each edge kept alive by its parent-side holder):

1. **State provider** (in parent keeper) captures a **strong** `Rc` to the child keeper.
   Graph `parent keeper → provider → child keeper` has no back edge (child providers
   point strictly downward to grandchildren), so no cycle. Keeper stays saveable for the
   full registration lifetime, with zero external handles alive. Provider bytes are the
   F-05 `DCS1` frame encoding the child's whole `BTreeMap`.
2. **Merged observers** live in the parent registries and capture the merged registry
   strongly; they unsubscribe by id from the teardown hook (or self-remove after driving
   merged to `Destroyed`, breaking the transient parent→merged→parent retention loop
   deterministically on the destroy path). Merged driver rules (F-08): `Initialized →
   Destroyed` goes through `create(); destroy()` (upstream-compatible observable
   `Created, Destroyed`, not a bare destroy); `AlreadyDestroyed` from a race/reentrant
   path is swallowed as idempotent success.
3. **Back bridge** (`SharedBack`, see ADR-0004): the single parent callback captures a
   **strong** handle to the child `SharedBack`; the child's enabled-listener captures a
   **`Weak`** parent link (the one genuine cycle), upgrading per call. All same-dispatcher
   mutations from inside dispatch queue FIFO; registration during dispatch is rejected.
4. **Instance scope** is obtained via **`get_or_create`** (F-04 normative — never `put`
   for same-key construction, so recreation preserves `Rc::ptr_eq` identity) and retained
   in the parent keeper under the child key.
5. **Destroy-contract + cleanup subscriptions** on manual lifecycles are `detach()`ed
   (fire-and-forget, like upstream's ignored `Cancellable`); the cleanup hook carries
   detached ids and removes itself plus all of the above when it fires. On destroy,
   Essenty-rs delivers observers in **reverse** subscription order (F-03): the
   destroy-contract guard is subscribed first so it fires *last*, after functional
   cleanup hooks have run — then it panics per the contract. A panic during observer
   delivery aborts the loop and may skip observer clearing; panic tests use fresh
   fixtures and assert nothing about post-panic cleanup.
6. **Root `attachTo`** (F-07): a fresh default `InstanceKeeper` gets exactly one detached
   destroy subscription on the root lifecycle calling `destroy_all()`. A *supplied*
   external keeper gets no hook — ownership stays with the caller, documented.

Teardown order on manual-child-lifecycle destroy (each step idempotent):
`unregister` state provider → `destroy` instance scope entry → back-bridge `stop`
(+ unregister parent callback) → unsubscribe merged observers → merged registry destroys
(cascading to child observers). Fixed children (shared parent lifecycle) keep the
provider until the parent scope ends — matching upstream.

`child_context(&self)`: parent keepers are shared as `Rc<RefCell<…>>` with short-lived
`borrow_mut()` calls that never cross observer delivery, back dispatch, user callbacks,
or factory invocation; back access goes exclusively through `SharedBack` (no raw
`RefCell` borrowing downstream).

## Analysis

- Rejected (guards-in-context + `Weak` provider): early context drop detaches merged
  following and leaks registrations; `Weak` loses saveable state while registered.
- Rejected (`Arc<Mutex>` / `Send + Sync`): taxes every callback, kills WASM, contradicts
  Essenty-rs and upstream's single-threaded UI-tree assumption.
- Rejected (raw `Rc<RefCell<BackDispatcher>>` in public API): F-02 reentrancy panic,
  executed before the fix and resolved by `SharedBack` (ADR-0004).
- Accepted difference: retained-value `Drop` runs at last-`Rc` release, not
  deterministically at scope end (upstream `onDestroy`); scope *bookkeeping* (destroyed
  flags, unregistration) stays immediate — only value dropping is ownership-driven.

## Consequences

Phase 1 structs follow this graph exactly. Mandatory parity tests: scope-survival after
factory return; strong-provider save with zero external child refs (test specified below);
teardown ordering; reentrant creation; `subscriber_count`/`handler_count`/`provider_count`
leak assertions; `get_or_create` `ptr_eq` identity; reverse-order destroy delivery. No
Essenty changes required.

Mandatory Phase 1 ownership test: create child state scope; drop all ordinary external
child handles not semantically required; save parent while the namespace is registered;
assert child state is present (and round-trips through restore).
