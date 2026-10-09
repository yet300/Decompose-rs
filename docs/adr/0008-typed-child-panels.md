# ADR-0008: Typed ChildPanels

Date: 2026-10-09. Status: accepted and implemented.
Reference: Decompose 00840bcfed6d55710c8fbb1a72503f2041eda490, router/panels.

## Context

Panels adds three independently typed roles and a lifecycle mode. Main is
required; Details and Extra are optional independently of each other. Several
roles may be Resumed at once. The existing engine already owns transactions,
lifecycle and queues, so a second engine would duplicate resource invariants.

## Decision

Keep one crate and public typed configuration/component parameters per role.
Private role-tagged configuration and component enums adapt them to the shared
engine. Same underlying values in different roles are distinct identities.
Collect configurations as owned Rc vectors so Panels needs no hidden cache or
mutable side channel. No runtime downcasts or Clone bounds are necessary.

Three independent Slots would make a compound replacement publish partially
committed states and complicate rollback. A caller-supplied common enum would
discard the typed role API of upstream. Both alternatives are rejected.

Single resumes the last present role (Extra, then Details, then Main). Dual
resumes Main and the last optional role; Details is Created if Extra exists.
Triple resumes every present role. Mode changes preserve component identity.
All lifecycles are bounded by the parent; only eligible live child Back bridges
are enabled. Automatic Back pops an optional role only in Single. Explicit pop
is available in any mode. Dismissing Details leaves Extra unchanged.

Use Infallible for an unsupported Extra role, mapping Kotlin Nothing without
inventing a dummy component. Supply a named constructor accepting two factories
and a matching two-role codec adapter. Keep the existing Child struct in public
snapshots; present panels are always live.

## State notification and persistence

Add a private model state-changed hook with a default no-op. Panels exposes an
optional callback receiving new state and optional old state. Initial notification
runs with Back/state registration complete while queued work is held. Later
notifications follow snapshot publication and Back enablement, preceding event
completion. No-op events and automatic Back also notify. Completed effects are
not reversed if a user callback panics; queues recover and provisional ownership
guards still clean up constructor failures.

DPN1 saves mode and optional-role presence, typed configuration payload, stable
keys, nested child state and the allocation counter. Whole-membership user codec
callbacks encode/decode three typed configurations; the library owns mode and
framing. Structural validation precedes codec and component callbacks, and decoded
presence must match the envelope. Existing DST1, DSL1 and DPG1 bytes stay unchanged.

## Consequences

Panels reuses the ownership, retained instances, queues and deferred Back paths of
the other models. No serde/runtime/UI dependency or Essenty changes are required.
Typed factories and snapshots express heterogeneous components directly. The
common engine remains private; its extension does not freeze a new generic API.

Independent specification and quality reviews passed without findings. Verification
passed 262 unit/integration tests plus 1 doctest, strict formatting/Clippy/rustdoc,
four runnable examples and compilation for all 24 installed targets.

Design and verification record:
../superpowers/specs/2026-10-09-child-panels-design.md and
../superpowers/plans/2026-10-09-child-panels.md.
