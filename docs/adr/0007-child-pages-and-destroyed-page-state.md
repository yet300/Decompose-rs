# ADR-0007: ChildPages and destroyed page state

Date: 2026-10-09. Status: accepted and implemented; final checks passed.
Reference: Decompose 00840bcfed6d55710c8fbb1a72503f2041eda490, router/pages.

## Context

Stack and Slot always have a live instance for every configuration in their
membership. Pages keeps configurations even when their components are destroyed.
Its selected index also affects lifecycle without changing membership. Reusing
the vector-only engine unchanged cannot represent these behaviors correctly.

## Decision

Keep one crate and the private shared engine. Navigation events carry each
model's actual state, including Pages selection. Models determine validation,
target lifecycle statuses, snapshots, Back and persistence. A managed page is
either a live scope or configuration/key/saved-state data without an instance.
Do not publish generic engine machinery or replace the existing Child struct.
Pages exposes a separate PageChild enum with optional instance access.

Use Option<usize> for selection: None denotes empty membership. Nonempty Pages
requires an in-range index. This replaces Kotlin's negative empty sentinel with
a Rust representation that does not mix valid positions and sentinel numbers.
set_items clamps the old numeric index; it does not follow configuration identity.

By default, selection targets Resumed, immediate neighbors target Created, and
other pages are Destroyed. A caller policy may choose Created, Started, Resumed
or Destroyed independently. The parent lifecycle bounds every live page.
Configuration equality and occurrence order preserve stable identities.

Child Back bridges require both a Started/Resumed target and a Started/Resumed
merged lifecycle. A merged-lifecycle listener updates the bridge when the parent
advances or stops, even if the child's own enabled callback does not change.
The same rule applies to the shared Stack/Slot scopes.

Destroying a page saves its child state and ends its scope and retained keeper.
Reactivating it uses the same key and saved state with a new component and keeper.
An old observable snapshot may retain an ended component. Preparing transitions
and state saves before destructive steps preserves the current state on typed
errors. Panic cleanup releases provisional scopes and restores queue usability;
it cannot undo arbitrary completed user callback side effects.

Pages uses its own DPG1 persistence envelope. It saves configuration order,
selection, child keys and nested states, including destroyed pages, plus the
monotonic key counter. Restored policy is recomputed rather than serialized.
Old DST1/DSL1 bytes and public Stack/Slot contracts remain compatible.

## Consequences

No Clone/Send/Sync bounds, runtime, UI, serde or extra Essenty changes are needed.
ChildPages makes destruction observable and requires callers to handle the
absence of a page instance. This is necessary for lazy lifetime management.
External parent retention preserves only pages that remain live; destroyed
pages preserve serializable state rather than retained services.

The design and verification record are in
../superpowers/specs/2026-10-09-child-pages-design.md and
../superpowers/plans/2026-10-09-child-pages.md.

## Verification

Independent specification and quality reviews approved the implementation.
221 tests passed, including all 181 baseline cases; strict Clippy, rustdoc,
formatting, three runnable examples and all 24 installed compilation targets
passed. Separate quality probes confirmed cleanup and usable retries after
predictive cancellation panic and failed tombstone resurrection.
