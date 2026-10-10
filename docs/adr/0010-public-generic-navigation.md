# ADR-0010: Public Generic Navigation over the shared engine

Date: 2026-10-09. Status: accepted and implemented; independent spec and quality reviews passed, 2026-10-10.
Reference: Decompose 00840bcfed6d55710c8fbb1a72503f2041eda490, router/children.

## Context

All five predefined navigation families are implemented. Applications also need
to describe their own whole navigation state, events and observable result.
Upstream exposes NavState, per-child lifecycle descriptors, a source of events
and a children factory with mapper, hooks, persistence and conditional Back.

Publishing the private Model trait directly would expose internal saved-child
representations and family-specific reconciliation hooks. A second public engine
would duplicate lifecycle, queue, scope teardown, rollback and retention rules.

## Decision

Add router::children in the same crate as a custom-model adapter to the existing
private engine. NavState<C> is a Clone whole-state with deterministic immutable
children() projection; ChildNavState<C> is a concrete Rc configuration/status
record. This replaces the Kotlin ChildNavState/SimpleChildNavState pair without
requiring another user trait. Configuration identity uses Eq alone.
Reuse live components by equality while refreshing managed configuration for
future factories. This matches upstream resurrection using the current descriptor
even when payload fields ignored by Eq have changed. Inactive snapshots expose
that current handle, using the same private opt-in policy introduced for Items.

Move generic lifecycle statuses and live/destroyed child records to neutral
ChildStatus and ChildItem types. Pages preserves its public PageStatus/PageChild
names through re-export aliases. The engine no longer depends on public Pages
types to represent arbitrary children.

Public NavigationSource<E> is an object-safe subscription trait. SimpleNavigation
broadcasts Rc<E> handles synchronously with FIFO reentry and cancellation guards.
Rc event sharing removes a Clone bound on event payloads. The adapter subscribes
before initialization, buffers setup events, and feeds receiver-specific typed
transformations into the existing engine queue. Its callbacks use Weak ownership;
the parent state provider retains the engine and subscription until destruction.

The factory maps N plus ordered ChildItem records to an arbitrary S. Optional
hooks observe committed state and explicit-event completion, including typed
errors. Back uses a cached optional action derived once per successful transition.
If no event completion hook is installed, rejected explicit events panic rather
than silently discarding errors, matching the predefined convenience operations.
It shares the existing priority and deferred-dispatch rules. User callback
captures and component snapshots remain user-controlled lifetime references.

Store whole-state snapshots internally as Rc<N>. Clone the handle while borrowed,
then run user N::clone after release; replace the handle and commit scopes before
dropping old N. Reentrant saves see coherent state, events remain queued, and a
Drop-triggered parent destruction stops publication. This addresses arbitrary
user-defined Clone/Drop implementations introduced by custom whole-state types.

Teardown takes the source token and queued events out of their cells, finishes
Back, child, provider and subscription cleanup, then propagates the first cleanup
panic. During existing unwinding it preserves the original panic. A custom
cancellation callback cannot strand live child scopes. Construction rollback
similarly guards each step: restore prior retention, remove a fresh keeper and
return consumed input before propagating cleanup panics. A private setup token
guards cancellation immediately after subscribe returns, covering initial-state,
decoder and pre-engine Clone panics before ownership transfers to the engine.
Existing unwinds retain their original panic instead of aborting on cancellation.

These guarantees cover navigation-owned cleanup and construction rollback.
Essenty lifecycle dispatch may stop delivering independent listeners when a
callback panics, including default-root attached keeper cleanup. The engine does
not destroy a parent keeper: a custom context may intentionally retain it across
lifecycle recreation. Catching a lifecycle panic therefore leaves the hosting
owner responsible for any retained cleanup whose listener was skipped. This
preserves the existing retention and lifecycle panic boundary.

## Persistence

NavStateCodec persists the entire custom N, including metadata and statuses.
Optional encode skips a save and child providers, following upstream's
save-state-before-save-children order; optional decode explicitly declines a valid frame
and starts from the lazy initial factory. Decoder errors remain errors.
DCH1 separates caller payload from stable keys, allocation counter and nested
DCS1 states for both live and destroyed entries. Structural validation precedes
user decode; restored count and duplicate policy precede component factories.
A codec must preserve configuration identity and order. Restored requested
statuses may change, as in the upstream API.

No codec means transient navigation, skipping child providers at parent save.
Existing family envelopes and behavior remain unchanged. Construction guards
restore consumed input and release reservations/subscriptions on failure.
Destructive lifecycle panics keep the existing limitation: ended scopes cannot
be revived by retaining an old snapshot; explicit later navigation reconciles
fresh scopes.

## Consequences

Consumers can build empty stacks, trees represented as ordered children or
application-specific activation policies without copying engine internals.
The core stays synchronous, single-threaded and independent of UI, platform,
serialization runtimes and extra dependencies. Linear Eq reconciliation and
immutable callback inputs match established Rust contracts in this port.

[Design](../superpowers/specs/2026-10-09-generic-navigation-design.md) and
[implementation/verification plan](../superpowers/plans/2026-10-09-generic-navigation.md).
