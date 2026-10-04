# ADR-0004: Back-handler reentrancy (`SharedBack`)

Date: 2026-10-04 (Phase 0.6/F-02). Upstream: `arkivanov/Decompose@00840bc`.
Essenty-rs: `0.1.0`. Status: **Accepted**.

## Context

Essenty-rs `BackDispatcher::dispatch_to` holds `entries.get_mut(&id)` across the synchronous
callback invocation (`dispatcher.rs:378-381`), and aggregate listeners fire from inside
`register`/`unregister`/`set_enabled` while `&mut self` is borrowed. Behind a shared
`Rc<RefCell<BackDispatcher>>`, any second borrow from inside a callback — e.g. the child
bridge's `child.borrow_mut().back()` completing and its enabled-listener calling
`parent.borrow_mut().set_enabled(...)` while the parent's `borrow_mut().back()` is still
active — panics with `RefCell already borrowed`. Executed before the fix: the F-02 flow
panics. The public API must therefore never expose raw `RefCell` borrowing downstream.

## Decision

Decompose owns a **`SharedBack`** abstraction above Essenty-rs (no Essenty changes):

```rust
#[derive(Clone)] // Rc-backed shared handle, !Send + !Sync, no Arc/Mutex
pub struct SharedBack { /* dispatcher, busy depth, FIFO pending queue, cached aggregate */ }

impl SharedBack {
    pub fn register(&self, priority: i32, enabled: bool,
        cb: impl FnMut(BackEvent, &SharedBack) + 'static)
        -> Result<BackHandle, DispatchInProgress>;
    pub fn unregister(&self, id: u64);            // immediate when idle, queued when busy
    pub fn set_enabled(&self, id: u64, enabled: bool);   // same
    pub fn set_priority(&self, id: u64, priority: i32);   // same
    pub fn back(&self) -> bool;                   // same-dispatcher reentry queues, returns false
    pub fn predictive_start(&self) -> bool;       // + progress / cancel / invoke
    pub fn can_handle(&self) -> bool;             // fresh read when idle, cached aggregate when busy
    pub fn add_enabled_changed_listener(&self, cb: impl Fn(bool) + 'static) -> u64;
}
```

Algorithm (executed in `/tmp/dc-proto-c`, as amended by the Phase 1.1
re-audit resolutions below — the amendment section is normative; the two
struck paragraphs are retained as explicitly SUPERSEDED history):

- **Uniform busy window.** Every entry point increments a `busy` depth counter; the
  *outermost* frame runs its operation then drains the FIFO pending queue; nested frames
  only enqueue. ~~A panic guard resets `busy` on unwind without draining (leftovers
  drain at the next entry).~~ SUPERSEDED by R-02: on unwind the busy flag resets and
  the entire pending queue is discarded — nothing replays later. No `RefCell` borrow
  is ever held across a nested entry: each queued op borrows separately.
- **Queued ops**: `SetEnabled`, `SetPriority`, `Unregister`, `AddListener`,
  `RemoveListener`~~, and deferred same-dispatcher `Invoke` (back / predictive
  variants)~~. SUPERSEDED by R-01: there is no deferred same-dispatcher `Invoke`;
  reentrant back/predictive calls return the false-equivalent and queue nothing.
  FIFO order; each applied after the outer dispatch releases dispatcher access.
- **Registration during dispatch is rejected** with `Err(DispatchInProgress)` — a precise,
  testable contract. Essenty exposes no id reservation for queued registration, and a
  wrapper-level id-indirection map was rejected as permanent complexity for a path the
  engine never needs (navigation events enter through the queued `Relay`, never through
  back dispatch; user child-creation from inside a back callback gets the explicit error).
- **Bridge rules (normative).** The child bridge parent callback is a plain
  `SharedBack::register` callback (NOT `register_reentrant`): it captures a strong child
  `SharedBack` and forwards each `BackEvent.phase` to the matching child entry point.
  Gate aggregation (`lifecycle-gate && child-aggregate`) flows through the child's
  enabled-listener, which holds a `Weak` parent link and calls parent `set_enabled` —
  safe in every window because the wrapper queues when busy.
- **Stale-bridge safety.** Queued gate updates drain FIFO before the next outer entry
  completes, so a bridge can never swallow a future event on a stale aggregate; `can_handle`
  during dispatch returns the maintained cached aggregate.

## Analysis

- *Evaluated `register_reentrant` + `BackCommands`*: rejected as the Decompose mechanism.
  `BackCommand` has no `SetPriority` variant, covers no predictive gating, and does nothing
  for cross-dispatcher listener re-borrows — the actual F-02 panic. The wrapper subsumes it
  uniformly; raw Essenty reentrancy remains available for non-Decompose uses.
- *Evaluated wrapper-level id indirection* (to support register-during-dispatch): rejected;
  explicit rejection is clearer and engine-sufficient (case 9 allows either).
- *Evaluated same-dispatcher reentrant `back()`*: queued with `false` return (documented),
  never executed reentrantly (borrow would be held), never panicking on supported paths.
- Gesture semantics preserved: claimed-owner routing, mid-gesture priority immunity, and
  removal-cancels-owner are inner-Essenty behaviors the wrapper delegates untouched.

## Consequences

`ComponentContext::back_handler()` returns `SharedBack`, never a raw
`Rc<RefCell<BackDispatcher>>`. In-handler mutation contract (F-11): ordinary mutations
outside dispatch apply synchronously and are immediately observable; same-dispatcher
mutation from inside a callback MUST go through `SharedBack` methods (which queue) —
raw nested borrowing is unsupported by API construction. Parity tests execute all nine
F-02 cases plus the F-11 contract on fresh fixtures.

## Phase 1 implementation amendments (2026-10-04)

R-01 through R-03 (independent re-audit) are resolved as follows; the
implementation (`src/shared_back.rs`) is authoritative:

- **R-01 — no deferred self-`Invoke`.** Same-dispatcher reentrant
  `back()` returns `false` immediately and queues nothing; reentrant
  predictive calls are non-recursive no-ops (`predictive_start` → `false`,
  progress/cancel/invoke → `Ok(false)`). There is deliberately no pending
  `Invoke` variant: the `callback → back → drain → callback → …` infinite
  synchronous drain loop cannot occur. Cross-dispatcher forwarding stays
  synchronous, and any parent→child→parent ping-pong terminates because the
  already-busy dispatcher in the cycle refuses reentry. Regression tests:
  `r01_same_dispatcher_reentrant_back_returns_false_without_requeue`,
  `r01_reentrant_predictive_calls_are_non_recursive_noops`,
  `cross_dispatcher_forwarding_stays_synchronous_and_terminates`.
- **R-02 — unwind discards the aborted dispatch.** On panic the busy flag
  resets and the *entire* pending mutation queue is discarded (not drained
  later); the dispatcher keeps its last completed state and stays reusable.
  Post-panic callers re-apply intended mutations. Regression test:
  `r02_panic_discards_queued_mutations_without_stale_replay`.
- **R-03 — evented aggregate only.** Enabled listeners receive the newly
  computed `enabled: bool`; bridge gate logic is
  `parent_enabled = lifecycle_gate && NEW evented child aggregate` and must
  never call `can_handle()` inside the listener (busy-window reads return
  the cached pre-drain aggregate by design). Positive test:
  `r03_bridge_uses_evented_bool_for_gate`; negative test proving the old
  `can_handle()`-in-listener pattern stale:
  `r03_can_handle_in_listener_pattern_is_stale_negative`.
- **Callback peer is weak.** The `&SharedBack` handed to registered
  callbacks is upgraded per call from a `Weak` link: a strong capture would
  close an `Rc` cycle (dispatcher → entry → peer → dispatcher) and leak
  every dispatcher with live callbacks. Proven by
  `bridge_lifetime_strong_child_weak_parent_releases` (strong counts).
- **Start/stop vocabulary mapping.** `SharedBack` exposes no public
  `start()`/`stop()`: *start bridge* = register the parent callback and
  enable per gate; *gate change* = `set_enabled`; *stop bridge* =
  `unregister` the parent callback. The upstream `start`/`stop` wording
  survives only in this mapping and in `ChildBackHandler` history.
- **Error layering (R-04).** `DispatchInProgress` is a `SharedBack`-level
  error, converted into `DecomposeError::BackDispatchInProgress` only at
  `ComponentContext` construction boundaries (with full rollback).
