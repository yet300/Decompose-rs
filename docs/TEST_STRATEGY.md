# Test Strategy (upstream → Rust parity matrix)

Principle: tests prove **semantics**, not implementation. Every suite below derives from
an upstream `commonTest` suite; each item states the observable behavior and the
classification for Phase 1.

Upstream suites inventoried: `MergedLifecycleTest` (40+ cases), `DefaultChildBackHandlerTest`,
`ChildContextWithLifecycleTest`, `ChildContextWithoutLifecycleTest`,
`ChildContextWithBackHandlerPriorityTest`, `MutableValueTest`, `ValueMapTest`, `RelayTest`,
`ChildControllerTest`, `Children*Test` (basic/keys/lifecycle/retained/saved/back),
`TransientNavStateSaverTest`, per-family stack/slot/pages/panels/items tests.

## Phase 1 suites (MUST IMPLEMENT IN PHASE 1)

1. **DefaultComponentContext**: defaults (fresh state keeper; instance keeper attached to
   lifecycle; fresh back dispatcher); supplied values used verbatim; factory produces detached
   contexts.
2. **MergedLifecycle**: non-destroyed inputs → `Initialized`; destroyed input → `Destroyed`
   with zero subscriptions; forward table (`C×C→Created`, `S×S→Created+Started`,
   `R×R→+Resumed`); reverse (`pause→Started` iff was `Resumed`; `stop→Pause+Stop`;
   `destroy→Pause+Stop+Destroy`); second destroy silent; destroy unsubscribes all
   (`subscriber_count == 0`); post-destroy parent moves have no effect; non-replaying manual
   drivers (reentrancy: transitions from inside observers converge).
3. **Child context without supplied lifecycle**: child shares parent lifecycle object
   (resume/destroy propagate); state restores after recreation; parent shows
   `is_registered(key)` / `contains(key)` / `back()`-gated behavior; duplicate key errors
   with the key in the message.
4. **Child context with supplied lifecycle**: min-table (either side `INITIALIZED` pins the
   child; both `RESUMED` advances); manual destroy of the supplied lifecycle errors;
   already-destroyed supplied lifecycle → detached scope (no registration, never saved).
5. **StateKeeper namespace**: duplicate-key `Err(DuplicateKey)`; consume-restore-save
   round-trip (parent save → child bytes → new parent → child consume); `unregister` on
   child-lifecycle destroy exactly once; destroyed-lifecycle child never saved; nested
   grandchildren chain; sibling internal-key reuse.
6. **InstanceKeeper namespace**: same-key `get_or_create` identity (`Rc::ptr_eq`) across
   config-change recreation; fresh scope on non-config recreate; parent `destroy_all`
   cascades to nested scopes (marked destroyed; value `Drop` deferred per documented
   difference); child-lifecycle destroy removes exactly once (second destroy safe);
   destroyed-lifecycle → detached keeper; type-mismatch `Err` leaves retained value intact.
7. **BackHandler hierarchy**: `start`/`stop` idempotence + `handler_count`;
   `enabled == lifecycle_gate && child.can_handle()` full 2×2 matrix; destroyed-lifecycle
   detached (`back() == false`); lifecycle `STARTED` gate table (`CREATED+enabled→false`).
8. **BackHandler priority**: explicit priorities dispatch highest-first; default ordering is
   reverse creation order (LIFO ties); live priority mutation affects subsequent presses.
9. **Predictive back forwarding**: start/progress/cancel/invoke each route to the child's
   current winner exactly once; gesture ownership survives mid-gesture priority changes;
   `progress/cancel/invoke` with no gesture → `Err(NoGestureInProgress)`.
10. **Recreation / restoration**: `TestContext::recreate(config_change: bool)` harness —
    state round-trips through real bytes; config change retains instance identity;
    process death replaces it.
11. **Cleanup / leak**: after full teardown, parent `subscriber_count == 0`,
    `handler_count == 0`, no providers registered; scope drop defensively unregisters
    (double-teardown safe).
12. **Reentrancy**: `child_context` and navigation-style transitions issued from inside
    lifecycle/back observers converge without deadlock (`RefCell` discipline: never hold a
    borrow across observer dispatch).

## Later / excluded

- `Children*` engine suites, per-family stack/slot/pages/panels/items suites →
  LATER NAVIGATION TEST (Phases 2–5, in dependency order).
- Web-history, browser binding, predictive-gesture animation suites → PLATFORM TEST (Phases 7–9).
- `expect/actual`, Parcel/Bundle serializer, Robolectric suites → NOT APPLICABLE.
- `Value`/`Relay` unit suites → Phase 2 with the observable-state implementation
  (needed by the engine, not by context).

## Harness

- `TestContext` (in-crate `#[cfg(test)]`): `LifecycleRegistry` default `Resumed`,
  empty keepers/dispatchers, `DefaultComponentContext` factory, `recreate(bool)` mirroring
  upstream `TestComponentContext.recreate`.
- Assertions on counts (`subscriber_count`, `handler_count`, `provider_count`,
  `pending_count`) — Essenty-rs exposes all of them; leak tests use them, not internals.
- Quality gates (also Phase 1 acceptance): `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test`, `cargo doc` clean on current stable-plus-Essenty toolchain (1.96.0).
