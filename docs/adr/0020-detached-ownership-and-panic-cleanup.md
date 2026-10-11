# ADR-0020: Detached ownership and independent panic cleanup

Date: 2026-10-11. Status: Accepted. Supersedes conflicting cleanup and borrowing claims in ADR-0003 and ADR-0005.

## Defects and decision

The audited `StateStore::unregister` replaced the entire shared keeper temporarily. Removed captures saw an empty keeper; successful reentrant mutations disappeared, and a destructor panic lost unrelated state. Restoration guards cannot make that design correct.

Decompose's `StateKeeper` now owns user providers separately from Essenty's inert key reservations. Registration methods, including value and optional-value providers, populate that ownership layer. Unregistration detaches only the matching callback and snapshot provider, removes its reservation, and releases the detached callbacks after the shared store borrow ends. Pending bytes/memory and unrelated registrations remain in place. Same-key registration from a destructor is an ordinary new registration and persists.

Essenty's instance registry remains responsible for typed retrieval and duplicate/terminal errors. An internal instance scope holds additional ownership references for Decompose-managed payloads. Whole-store release replaces the registry under a short borrow, preserving terminal state, then releases the old registry and each additional reference independently. Destructors see the committed empty/destroyed state. Individual retained-scope removal also detaches before releasing ownership.

`cleanup::Cleanup` supplies one protocol for preparation, scope ending, retention, construction rollback, source cancellation, history cancellation and teardown:

1. Commit idempotent bookkeeping before callbacks.
2. Catch each independent cleanup action, allowing the remaining actions to finish.
3. Preserve an unwind already active when cleanup starts.
4. Otherwise resume the first cleanup panic after independent actions finish.
5. Dispose of secondary panic payloads under a catch boundary. If their destructor itself panics, forget that replacement payload to prevent recursive panic-payload destruction from aborting cleanup.

This does not turn callback failures into successful navigation. Factory/predicate/observer panics still propagate; rollback preserves consumed input and prior retention for retry. A scope already ended by a callback failure is reconstructed on retry rather than reused.

## Primitive and ownership boundaries

Published Essenty 0.1.0 provides neither a provider-detach operation nor instance enumeration/drain. Arbitrary providers registered directly in an `EssentyStateKeeper` before conversion (or through explicit primitive dereferencing) retain primitive destructor/reentry behavior: their captures cannot be extracted by Decompose. Register through Decompose's `StateKeeper` or `StateStore` for the detach guarantee. Conversion preserves imported registrations and pending bytes; it does not execute providers eagerly or copy their state into a mock keeper.

Likewise, opaque values already present in an imported raw `InstanceKeeper` cannot be enumerated for per-value panic isolation. Whole-store release drops that primitive outside the store borrow, so reentrant reads work, but multiple panicking destructors hidden in that primitive can still double-panic. Values inserted through Decompose services and all library-owned retained scopes use independent release. A supplied external instance store remains caller-owned and gets no default lifecycle destruction hook.

Essenty lifecycle dispatch itself may stop delivering other listeners after one listener panics. Decompose finishes the independent steps inside its owned cleanup boundaries; it cannot force arbitrary primitive lifecycle listeners to run. Explicit lifecycle Destroy is still required for parent-owned subscriptions. External Rc snapshots keep component objects alive after scope bookkeeping ends, so their final object destructor belongs to the final external owner.

No unsafe field extraction, dependency source changes, path override, alternate runtime, Arc or mandatory Clone bound is introduced. Durable Rust persistence envelopes remain unchanged.

## Evidence

`tests/store_unregister.rs` reproduces reentrant reads, replacement/same-key registration, nested removal, restored consumption, panic preservation, provider families and independent callback-map release. `tests/panic_cleanup.rs` exercises actual root/child lifecycle paths, deactivation, retained-scope release, restored retries, survivor identity, first-panic propagation and subprocess double-panic cases. Final executed results are recorded in the remediation report, separately from Kotlin source inspection.
