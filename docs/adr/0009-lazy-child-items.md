# ADR-0009: Lazy ChildItems with receiver-local navigation

Date: 2026-10-09. Status: accepted and implemented.
Reference: Decompose 00840bcfed6d55710c8fbb1a72503f2041eda490, router/items.

## Context

ChildItems manages ordered, unique configurations and a separate map of active
lifecycle targets. Several children can be Started/Resumed simultaneously; other
members have no component until requested. Upstream returns a lazy observable
that also navigates its own receiver. It does not define an automatic Back pop.

Rust configurations in this port need only Eq and 'static, and never Clone.
Requiring a HashMap or BTreeMap would add Hash/Ord to the public contract; mapping
activity by list index would lose its association with configurations on reorder.

## Decision

Keep one crate and reuse the existing private engine, including its inactive
entries with stable keys and saved state. A small ItemMap<C,V> uses equality
lookup over unique Rc configuration handles. It has linear lookup and introduces
no Hash/Ord bound or dependency. Snapshots expose ordered configuration handles
and a map of active Child instances with requested lifecycle targets.

The lazy handle exposes normal observable get/value/subscribe and an explicit
get_or_create(&C) returning Result<Rc<T>, NavigationError>. Already active lookup
preserves identity and target; inactive lookup activates Created. Unknown members
are errors. A synchronous inactive lookup during navigation/Back/construction is
rejected before it queues work. A child removed by queued construction effects
cannot be returned and yields ChildUnavailable. Parent destruction yields Destroyed. Active snapshot entries are checked against
the live scope: after a destructive lifecycle panic the old snapshot can retain
an ended component, which lazy lookup rejects with ChildUnavailable.

A default identity normalization hook lets Items prune/canonicalize activity
before validation; existing models retain their previous behavior. A private
control with Weak engine ownership dispatches receiver-local events to
the same FIFO queue. It makes neither a second transaction engine nor a broadcast
through the external source. Existing builders continue returning Value. Dropping
a source does not disable local navigation; dropping a view does not stop its
parent-owned router. Back is delegated to eligible live children only.

Normalize active-map entries to canonical membership configurations and prune
missing keys consistently. This follows upstream pruning on list changes and
gives a deterministic safe result where other upstream invalid-map behavior is
undefined. Duplicate member configurations are always rejected.

## Persistence and ownership

DPI1 saves order, active targets, counter, stable keys and nested state for every
member, including inactive entries. User codecs handle the ordered configuration
list only; library metadata owns activity. Validate all structural fields and
nested DCS1 before decoding configurations, then validate count/uniqueness before
factories. Restored empty/all-inactive state overrides initial input.

Deactivation saves state and ends the old component scope/keeper; reactivation
creates fresh instances with the same key/state. Removing a configuration discards
its state and key. Equal member replacement preserves live instances, but a later
fresh activation receives current configuration data, including fields not used
by Eq. An Items-specific private reuse policy keeps previous model behavior intact. External retained services survive parent recreation for
restored live children. Transient routers reset and discard previous retention.
Existing reservations, rollback and coherent saving remain engine responsibilities.
Old DST1/DSL1/DPG1/DPN1 envelopes stay unchanged.

## Consequences

The API supports Eq-only, non-Clone configurations and service-bearing custom
contexts. Lazy reads have explicit fallible semantics suited to Rust ownership.
The public core remains single-threaded and synchronous, with no serde/runtime
or platform/UI dependency. Linear equality lookup is a deliberate tradeoff; this
stage does not promise hash-indexed reconciliation performance.

Independent specification and quality reviews passed after correcting a stale
active-snapshot getter found by quality review. Verification passed 325 unit/
integration tests plus 1 doctest, strict formatting/Clippy/rustdoc, five examples
and compilation for all 24 installed targets.

[Design](../superpowers/specs/2026-10-09-child-items-design.md) and
[verification plan](../superpowers/plans/2026-10-09-child-items.md).
