# ADR-0013: Add explicit lazy standard-router factories

Date: 2026-10-10. Status: implemented and verified; independent spec and quality
reviews passed without findings.
Baseline/main: e129ab1. Branch: codex/lazy-initial-state-compatibility.
Reference: Decompose 00840bcfed6d55710c8fbb1a72503f2041eda490.

## Decision

Add six `_with_initial` functions for Stack, Slot, Pages, Panels (three-role and
no-extra) and Items. Each accepts FnOnce returning the same state as the eager
API, with no initializer Clone/'static bound. Existing eager functions delegate
through closures and keep their named generic counts and source contracts.
Rust cannot overload free functions, and a conversion trait accepting arbitrary
states and closures would add avoidable public wrappers/coherence complexity.

Use the existing shared engine's restoration-before-initialization path. A valid
restored state suppresses initialization, including empty/inactive membership.
Without restored state, evaluation is synchronous during construction. FnOnce
allows borrowed and owned non-Clone captures without retaining them in a router.
This does not change lazy Items component creation, model validation, codecs,
Back or event completion. Initializer events use the existing initialization FIFO.

Existing rollback guards handle validation/panic/Destroy and cancellation. Codec
errors do not fall back to initialization. Caller callback side effects remain
outside rollback. Custom-source pre-token registration cleanup and Essenty panic
dispatch limitations retain their documented boundaries.

A regression exposed parent destruction during initializer evaluation or capture
Drop (including a skipped initializer). The shared builder accessed the already
destroyed instance keeper before checking lifecycle again, yielding
Context(InstanceKeeper(Destroyed)). Rechecking the parent immediately after the
restoration/initializer selection returns NavigationError::Destroyed before
retention access and keeps construction rollback in charge of registrations.

## Compatibility accounting

Publish [COMPATIBILITY.md](../COMPATIBILITY.md) with pinned upstream source links,
Rust source/test evidence and explicit implemented/adapted/gap/excluded statuses.
It separates semantic coverage from Kotlin overloads/wire formats and compile
targets from runtime tests. Standard per-save/per-restore decline, memory-only
saving, global settings/error hooks and public test utilities remain documented
gaps or excluded delivery. Web/UI/platform integrations remain outside scope.

No extra crate, dependency, runtime or mandatory configuration Clone/Hash/Send/
Sync bound. Validation passed 428 unit/integration tests and 1 doctest, including
18 new lazy initialization tests. Formatting, strict Clippy/rustdoc, all nine
examples and library compilation for all 24 installed targets passed. Confidence
is high for this stage's bounded Rust contract. Implementation is delivered on
its feature branch; local commit was subsequently requested by the user.

[Design](../superpowers/specs/2026-10-10-lazy-initial-state-compatibility-design.md)
and [verification plan](../superpowers/plans/2026-10-10-lazy-initial-state-compatibility.md).
