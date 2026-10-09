# ADR-0006: ChildSlot and shared private navigation machinery

Date: 2026-10-09. Status: accepted and implemented; final checks passed.
Reference: Decompose 00840bcfed6d55710c8fbb1a72503f2041eda490, router/slot.

## Context

ChildStack already implements scope creation, lifecycle transitions, retained
instances, constructor rollback and repeated callback entry. ChildSlot adds
zero-or-one membership and exercises paths a nonempty stack cannot express.
Duplicating the stack engine would make future fixes apply to two owners of the
same lifecycle and resource invariants. Publishing a generic children engine
would commit to an API broader than the current tested models.

## Decision

Keep one crate. Share the private event/scope/transaction machinery; small model
policies define valid membership, snapshot shape, automatic Back operation and
persistence. Public router::slot exposes ChildSlot with Option<Child>,
SlotNavigation and explicit codecs. Preserve router::stack public imports and
DST1 persistence compatibility. Common child and error types have compatibility
reexports; public configuration and component snapshots continue using Rc.

Empty slots are valid and invoke no factory. Replacing a configuration prepares
the incoming scope before removing old membership, then destroys the outgoing
child before the incoming child resumes. Equal configurations preserve child
identity without lifecycle churn. Dismiss ends its retained scope even if old
snapshots keep the component instance reachable. Source ownership, parent
lifetime, repeated callback queues and deferred Back work follow Stack rules.

SlotCodec serializes one configuration. DSL1 frames explicit empty/populated
membership, stable key/counter and nested child state. Empty state bypasses
configuration callbacks and overrides a populated initial configuration on
restore. The empty envelope must be saved, so an old active configuration cannot
reappear after dismissal. Corrupt input returns a typed error and remains
available for retry. Saved membership controls retained-key reconciliation.

Convenience operations propagate rejected transitions as panics. navigate_with,
activate_with and dismiss_with expose typed completion, with dismiss reporting
whether a child existed. User panics propagate; abandoned work and provisional
resources are cleaned up. Successful user callback side effects remain outside
rollback.

## Consequences

The public API has no Clone/Send/Sync bounds on contexts/components, and no
UI/runtime/serde dependency. State keeper and Essenty primitives remain the
existing dependencies. Slot and Stack can coexist with different hosting keys;
Back uses the established child bridge and SharedBack priority/LIFO rules.
The shared engine remains internal, leaving future Pages behavior free to
introduce additional scope states without freezing public generic machinery.

## Lifecycle-panic recovery

Membership is committed after lifecycle transitions. A user callback panic can
therefore leave the previous snapshot referencing an ended local scope, even
though event pumps recover. Equality matching must consider only live local
lifecycles and retained keepers; a subsequent equal-configuration request
constructs a fresh scope/key instead of reporting success with a destroyed
component. Completed callback side effects are not reversed. Regression cases
cover both outgoing destroy and incoming resume panics in Slot and Stack.

## Verification

The implementation passed independent spec and ownership/correctness reviews.
181 tests passed (115 unit, 34 Slot integration, 27 Stack integration, 4 context
integration, 1 doctest), as did strict Clippy, rustdoc, formatting and both examples.
Library compilation passed all 24 installed targets. The full command/target
record is in ../superpowers/plans/2026-10-09-child-slot.md.
