# ADR-0012: Reuse typed navigation sources for standard routers

Date: 2026-10-10. Status: implemented; spec and quality reviews passed.
Baseline/main: 9a70ae0. Branch: codex/router-extension-points.

## Decision

Standard child factories accept the existing object-safe NavigationSource<E>
trait. Public immutable NavigationEvent<S,R> and family event aliases describe
transformers and per-receiver completion with shared Rc callbacks. The concrete
navigation constructors remain convenient defaults and implement the same source
protocol using SimpleNavigation, sharing its synchronous FIFO broadcast loop.

StackNavigator, SlotNavigator, PagesNavigator and PanelsNavigator provide generic
navigate_with plus family helper methods, following the existing ItemsNavigator
pattern. Inherent methods delegate to the traits; custom wrappers inherit helpers
without reproducing the standard model's lifecycle or state policies. Navigator
traits use static dispatch because their methods accept generic callbacks. Source
registration is object-safe and child factories also accept dyn NavigationSource.

Family adapters translate events and completion results into the existing private
engine. Slot's optional configuration becomes the private Vec representation at
that boundary. Panels retain typed roles and Items retain receiver-local lazy
control. Existing construction buffering, rollback, Back and lifecycle teardown
guards apply to standard custom sources.

## Compatibility and consequences

No new crates, dependencies, runtime or mandatory Clone/Hash/Send/Sync bounds.
Factories preserve their named generic parameter count through argument-position
impl Trait. Source implementations need not be Clone or 'static themselves; their
returned subscription owns registration. Events default to no-op completion,
while navigator convenience methods retain panic-on-error behavior and explicit
navigate_with reports typed errors. Each receiver independently completes its
transition after publication. Back operates locally without calling the source.

A custom subscribe implementation owns cleanup before it returns its token.
If it panics without returning the token, the router cannot recover a registration
that the source failed to guard. Once a token exists, the shared engine owns
construction rollback and destruction cancellation. Existing Essenty panic
dispatch limitations remain applicable.

Lazy initial-state closures, Web history and UI/platform integrations are separate
scope. Local integration into main was authorized by the user after verification.

Validation: 410 unit/integration tests and 1 doctest passed, including 21 new
extension regressions. Formatting, strict Clippy/rustdoc, all eight examples and
library compilation for 24 installed targets passed. Independent spec and quality
reviews both passed without findings; confidence is high.

[Design](../superpowers/specs/2026-10-10-router-extension-points-design.md) and
[plan](../superpowers/plans/2026-10-10-router-extension-points.md).
