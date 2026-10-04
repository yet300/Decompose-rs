# ADR-0003: Child-scope ownership (`Rc` handles, `&mut`-at-birth, RAII guards)

Date: 2026-10-04. Upstream: `arkivanov/Decompose@00840bc`. Status: **Accepted**.

## Context

Upstream attaches child scopes via interface extensions (`StateKeeper.child`,
`InstanceKeeper.child`, `BackHandler.child`) backed by GC + `Cancellation` tokens.
Rust has no GC and Essenty-rs keepers take `&mut` for registration; the port must define
where `Rc`/`Weak`/`RefCell`/borrows go, where cycles break, and how teardown runs —
without `Arc<Mutex<…>>` and without requiring `Send + Sync`.

## Decision

- Live scopes are `Rc`-shared handles (`LifecycleRegistry` already is; keepers/dispatchers
  ride in `Rc<RefCell<…>>`); construction takes short-lived `&mut` borrows of the parent
  keeper/dispatcher (`consume` + `register` sequencing, duplicate keys as `Result`).
- Parent-registered closures (state save providers, back enabled-listeners) capture `Weak`
  handles and upgrade per call — never strong child handles.
- Lifetimes are tied by owned RAII guards: `MergedLifecycle` stores two `Subscription`s,
  `ChildBackHandler` stores its guards + parent handle id; drop = unsubscribe/unregister.
- Teardown is explicit + defensive: child-lifecycle destroy → `unregister` + `remove` +
  back `stop()` (all idempotent `bool` returns); scope drop repeats defensively.

## Analysis

- `&mut`-at-birth is a deliberate divergence from Kotlin's extension style, forced by
  Essenty-rs signatures; it improves the API (construction sequencing is checked by the
  borrow checker, duplicates surface as errors at the call site).
- `Weak`-in-parent-closures is the single cycle-breaking rule; the prototype confirms
  parent drop is observable via failed upgrade.
- Rejected (`Arc<Mutex>` everywhere): would tax every callback, kill WASM viability, and
  "solve" ownership by erasing the single-threaded UI-tree reality both Essenty-rs and
  upstream assume. Rejected (global singletons / mandatory runtime): banned by anti-goals.
- Documented difference: retained-value `Drop` runs at last-`Rc` release rather than
  deterministically at scope end (upstream `onDestroy`); scope bookkeeping stays immediate.

## Consequences

Phase 1 structs (`MergedLifecycle`, child adapters, `TestContext`) follow this graph
exactly; leak tests assert on Essenty count APIs (`subscriber_count`, `handler_count`,
`provider_count`) rather than internals.
