# Essenty-rs Compatibility Audit (`essenty-* 0.1.0`)

Audited against the published crates.io sources (registry copies), not design notes.
Essenty release: tag `v0.1.0` = `2bf0e9108d8ae2dcd67ac3ab7ea7ca54cae16092` (2026-09-24).
All four crates: Edition 2024, `rust-version 1.96.0`, Apache-2.0, `!Send + !Sync` (`Rc`-based),
`std` with only `alloc`/`core` containers (future `no_std + alloc` realistic).

Verdict: **no genuine Essenty gap. No Essenty changes required.** Every child-scope
semantic composes in Decompose from public Essenty-rs APIs. One intentional semantic
difference (retention cleanup timing) is documented below.

## 1. Decision table

| Capability (upstream requirement) | Essenty-rs support | Decision |
|---|---|---|
| Lifecycle states + validated transitions (`create/start/resume/pause/stop/destroy`, `move_to`) | `LifecycleRegistry` + `LifecycleState` — same five states, same step methods, `move_to` multi-step, `destroy` walks back through `Started/Created` (or direct from `Initialized`) | DIRECT MATCH |
| Subscription replay (new subscribers catch up to current state) | `subscribe` replays `Created/Started/Resumed` for the current state | DIRECT MATCH |
| Reverse-delivery order | `apply` notifies in reverse for `Started/Created/Destroyed` retreat transitions | DIRECT MATCH |
| Per-event hooks (`doOnCreate/doOnStart/doOnResume/doOnPause/doOnStop/doOnDestroy`) incl. direction tracking (`Started` from pause vs start) | `do_on_create/do_on_start(_once)/do_on_resume(_once)/do_on_pause(_once)/do_on_stop(_once)/do_on_destroy`; pause/stop track consecutive deliveries; `do_on_destroy` on destroyed registry runs immediately | DIRECT MATCH |
| RAII unsubscription for merged-lifecycle observers | `Subscription` guard: drop unsubscribes, `detach() -> u64`, `unsubscribe(id)`, `is_active()` | DIRECT MATCH (observers parent-held + `detach()`ed; explicit removal by id in teardown hooks — ADR-0003) |
| State providers under unique keys; duplicate rejection | `register` / `register_value` / `register_optional(_value)` → `Err(DuplicateKey)` | DIRECT MATCH (map to `DecomposeError::DuplicateKey`) |
| Single-shot consume of restored state | `consume_bytes` / `consume_value` (remove-on-read, `None` when absent/consumed) | DIRECT MATCH |
| Deterministic save chaining (parent → child → grandchildren) | `save() -> BTreeMap<String, Vec<u8>>`; providers replace restored values; `None` providers preserve restored | DIRECT MATCH (child registers `move \|\| child.save()` closure) |
| Unregistration on child destroy | `unregister(key) -> bool` | DIRECT MATCH |
| Byte-oriented state (no format lock-in) | Opaque `Vec<u8>` + caller-supplied `encode`/`decode` fns; `serde` only as bounds, no format hard-coded | SEMANTICALLY EQUIVALENT and better: do not reintroduce `SerializableContainer`; boundary is codec-supplied bytes |
| Instance retention by key with identity (`getOrCreate` same object) | `get_or_create` returns clones of the same `Rc<T>` (factory runs ≤ once) | DIRECT MATCH |
| `put` prebuilt retention / `remove` / typed `get` with mismatch error | `put`, `remove<T>`, `get<T>` → `TypeMismatch` instead of panics | DIRECT MATCH (stricter and better than Kotlin casts) |
| Scope end (`destroy`/`destroy_all`, `is_destroyed`, `Destroyed` error) | `destroy(key)`, `clear`, `destroy_all`, `is_destroyed`, `Destroyed` error | DIRECT MATCH |
| Back dispatcher: priority + enabled winner, LIFO ties | `register(priority, enabled, cb)`; winner = max `(priority, registration id)` among enabled | DIRECT MATCH (`PRIORITY_DEFAULT = 0` must be re-declared in Decompose; Essenty-rs has no such constant) |
| Back enabled aggregation + listener | `can_handle()` + `add_enabled_changed_listener` (mirrors upstream by name) | DIRECT MATCH |
| Regular + predictive invocation (`back`, start/progress/cancel/invoke with gesture ownership + fallback) | `back`, `predictive_start(_with)`, `predictive_progress(_with)`, `predictive_cancel`, `predictive_invoke`; claimed-owner routing; removal-cancels-owner; `NoGestureInProgress` error | DIRECT MATCH (map the four `DefaultChildBackHandler` forwards 1:1; `BackEvent { phase, progress, position }` covers upstream `BackEvent`) |
| Priority mutation / per-callback enabled toggle | `set_priority`, `set_enabled`, `is_enabled`, `priority`, `unregister -> bool` | DIRECT MATCH |
| Reentrant registration changes during dispatch | `register_reentrant` + `BackCommands` queue (applies after the callback returns) | SEMANTICALLY EQUIVALENT (use for callbacks that register/unregister from inside handlers) |
| `MergedLifecycle` (min of two lifecycles) | Not in Essenty (same as upstream: lives in Decompose) | MUST LIVE IN DECOMPOSE-RS (build on `subscribe` + step methods; see §2) |
| `child(...)` scoping helpers | Not in Essenty (same as upstream: Decompose extensions) | MUST LIVE IN DECOMPOSE-RS (see §2) |
| Observable `Value` / `Relay` / `Cancellation` | Not in Essenty (upstream Decompose owns them) | MUST LIVE IN DECOMPOSE-RS (Phase 2; core stays runtime-agnostic) |
| `KSerializer` plumbing / `expect-actual` / `Uuid.randomKey` | Kotlin-specific | KOTLIN-SPECIFIC / NOT APPLICABLE (keys: caller-supplied `String`; engine key minting: deterministic counter, never random, in core) |
| Thread-safe (`Send+Sync`) registries / Tokio integration | `essenty-lifecycle-tokio` exists but is optional-only | OPTIONAL INTEGRATION, never a core dependency |
| `Instance.onDestroy()` deterministic cleanup callback | **Not present**: retention follows Rust ownership — `Drop` runs at last-`Rc` release, not at scope end | ADAPTER IN DECOMPOSE with documented semantic difference (see §3); NOT a blocker |

## 2. Adapter shapes (all implementable with public APIs only; Phase 0.5 revision)

Parent keepers/dispatchers are shared as `Rc<RefCell<…>>`, so `child_context(&self)` uses
short `borrow_mut()` calls that never cross observer delivery, back dispatch, user
callbacks, or factory invocation (prototype-validated, incl. nested + observer-driven
creation). Because `register` providers are `'static`, the public boundary takes `&str`
keys but everything captured is an owned `String`.

- **MergedLifecycle**: struct holding the merged `LifecycleRegistry` + side-state cells.
  Parent registries own the observer closures (self-sustaining; removed by detached id
  from the teardown hook, or self-removed after driving merged to `Destroyed`). On each
  delivery compute `target = min` with explicit destroyed checks — do NOT use derived
  `Ord`, on which `Destroyed` is maximal — and drive the merged registry toward the
  target. Destroyed input at construction → immediately `destroy()` the merged registry
  and subscribe to nothing.
- **Child StateKeeper**: `consume_bytes` + `register` via short borrows. The save closure
  captures a **strong** `Rc<RefCell<StateKeeper>>` to the child dispatcher (upstream's
  `stateKeeper::save` bound reference is strong; no back edge exists, so no cycle).
  Teardown link = lifecycle-destroy `unregister`; never context drop.
- **Child InstanceKeeper**: `put(key, …)` the child scope (retained, strong by
  construction; `get_or_create` identity for config-change retention); wire the child
  lifecycle's destroy hook to `destroy(key)` (idempotent `bool`).
- **Child InstanceKeeper**: `get_or_create(key, \|\| ChildScope::new())` on the parent
  keeper where `ChildScope` owns a nested `InstanceKeeper`; wire the child lifecycle's
  `do_on_destroy` to `parent.remove::<ChildScope>(key)` (exactly-once via `bool` return).
  Already-destroyed lifecycle → fresh detached `InstanceKeeper::new()`.
- **Child BackHandler**: struct holding the child `BackDispatcher` + parent handle id +
  gate cell + detached-subscription ids. Register one parent callback (capturing a strong
  `Rc` to the child dispatcher) forwarding `BackEvent.phase` to it
  (`Started→predictive_start`, `Progressed→predictive_progress`, `Cancelled→predictive_cancel`,
  `Invoked→back`); aggregate `parent.set_enabled(handle, gate && child.can_handle())` on
  every gate change and via `child.add_enabled_changed_listener` (which captures a
  **`Weak`** parent link — the one genuine cycle, broken by upgrade-per-call).
  `stop()` = `parent.unregister(handle)`. Teardown hook drives the full sequence.

## 3. Intentional semantic difference: retention cleanup timing

Upstream `InstanceKeeper` calls `Instance.onDestroy()` deterministically at scope end even
while external references exist. Essenty-rs (and therefore Decompose-rs) runs `Drop` when
the last `Rc` is released. Consequences, all acceptable and documented as contract:

1. Values that need scope-end side effects must implement them in `Drop` (idiomatic Rust);
   `Drop` may run *later* than scope end if the component tree still holds clones.
2. Scope-end *bookkeeping* (marking the nested scope destroyed so post-destroy
   `get_or_create` returns `Destroyed`) is explicit and immediate — only value dropping is deferred.
3. Navigation teardown still explicitly releases scopes in reverse order (as upstream does);
   determinism of *ordering* is preserved, only value-drop timing is ownership-driven.
4. No Essenty change is proposed: adding an `onDestroy` callback protocol to Essenty-rs
   would fight Rust ownership for zero navigation benefit, since the engine already owns
   explicit teardown paths.

## 4. Dependency style

Depend on the four primitive crates directly
(`essenty-lifecycle/state-keeper/instance-keeper/back-handler = "0.1.0"`), never on the
umbrella `essenty` crate (which pulls platform adapters) and never on
`essenty-lifecycle-tokio` in core. This mirrors upstream's `api()`-on-four-libraries
boundary exactly.
