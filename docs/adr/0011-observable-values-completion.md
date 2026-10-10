# ADR-0011: Complete observable values using Rc identity

> Naming superseded by [ADR-0019](0019-explicit-rust-api-surface.md):
> `compare_and_set`/`update`/`update_and_get`/`get_and_update` are deleted in
> favor of `compare_exchange`/`update_rc`/`update_rc_and_get`/`get_and_update_rc`
> plus `modify`. The identity/retry semantics below are unchanged.

Date: 2026-10-10. Status: implemented and independently reviewed.
Reference: Decompose 00840bc, value and value/operator.

## Decision

Keep Value and MutableValue in the existing crate and synchronous single-threaded
API. Complete cached map, lifecycle-bound subscription and compare/update helpers
without imposing Clone/Eq or introducing a runtime. Payload snapshots are Rc;
compare_and_set compares Rc::ptr_eq, and update transformers return Rc snapshots.
Reentrant mutation can make an update's expected snapshot stale, so it retries
the transformer. Return the successful CAS's predecessor or installed snapshot,
even if an observer immediately performs another mutation.

Map is a derived Value with a cached input/output pair keyed by input identity.
It computes the initial output immediately and refreshes reads without requiring
permanent observation. Multiple readers and observers reuse the mapped snapshot.
Mapper callbacks and old cache payload destruction run outside obstructing cell
borrows. Recursive reads of the same in-flight uncached input have a documented
panic guard; unwinding leaves the cache usable for retry.
Revision protection includes upstream reads, since a chained derived read can
execute user code before the downstream mapper begins. Captured older results
may be returned to their caller without replacing a newer reentrant cache or
publishing them after a subscriber's fresher replay.

subscribe_with_lifecycle gates source observation at Created, Started or Resumed.
ObserveLifecycleMode defaults to StartStop. A returned Subscription explicitly
owns both registrations; this RAII API replaces Kotlin's void lifecycle-owned
extension. Dropping/cancelling ends the binding; detach leaves it active until
terminal lifecycle destruction. A reopened gate replays the latest source value.
Ordinary RAII ownership avoids a registry/listener cycle. Essenty has no weak
registry handle, so explicit detach retains a registry witness until Destroy;
dropping external registry handles alone does not release detached bindings.
Setup guards handle a replay that stops/destroys its lifecycle or panics before
the source token has been returned.

## Scope and consequences

No new crate, dependencies, Send/Sync bounds, platform integrations or Essenty
changes. Rc identity supplies coherent synchronous compare/update semantics,
not multithreaded atomicity. Transformations may execute more than once and
successful user callback effects are not rolled back after a panic. Existing
Essenty lifecycle observer delivery limits still apply when callbacks panic.

The public Value/MutableValue names and direct observation semantics remain
compatible. Private mapping/lifecycle helpers keep their responsibilities
separate from the direct value notification pump.

[Design](../superpowers/specs/2026-10-10-observable-values-design.md) and
[implementation plan](../superpowers/plans/2026-10-10-observable-values.md).
