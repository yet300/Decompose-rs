# ADR-0001: ComponentContext as trait + default struct

Date: 2026-10-04. Amended: 2026-10-04 (Phase 0.5: `Clone` bound removed).
Rewritten: 2026-10-04 (Phase 0.6/F-01: `from_parts` removed from trait; fifth
`context_factory` member added; `child_context` provided via the factory).
Upstream: `arkivanov/Decompose@00840bc`. Status: **Accepted**.

## Context

Upstream models context as `GenericComponentContext<out T>` with `ComponentContext` as the
default instantiation, so user-defined contexts flow through navigation untouched. Its
load-bearing fifth member is `componentContextFactory` — not a generic constructor. A
first-draft Rust trait with `fn from_parts(parts: ContextParts) -> Self` was incoherent:
`ContextParts` cannot synthesize a service-bearing custom context's extra state (e.g.
`logger: Rc<Logger>`), so no custom type could implement it without already owning its
services — the exact thing the factory closure exists to capture.

## Decision

```rust
pub trait ComponentContext: Sized {
    fn lifecycle(&self) -> LifecycleRegistry;
    fn state_keeper(&self) -> SharedKeeper;
    fn instance_keeper(&self) -> SharedInstances;
    fn back_handler(&self) -> SharedBack;
    /// Upstream `componentContextFactory`: builds the SAME custom type from
    /// prepared child scope parts, closing over any extra services.
    fn context_factory(&self) -> ContextFactory<Self>;

    /// Provided: prepare the child scope, then construct through the factory.
    fn child_context(&self, key: &str) -> Result<Self, DecomposeError> { /* ... */ }
    fn child_context_with_lifecycle(&self, key: &str, lifecycle: &LifecycleRegistry)
        -> Result<Self, DecomposeError> { /* ... */ }
}
```

- `from_parts` remains only as an inherent/private constructor on concrete types
  (`DefaultComponentContext::from_parts`, `TestContext::from_parts`) — never on the trait.
- `child_context` is a provided trait method: scope preparation (steps F-06/1–8) runs once
  in the default impl; construction (step 9) goes through `self.context_factory()(parts)`,
  so every custom type preserves itself with zero casts and no `Clone` bound.
- The factory is computed from `&self` per call (fresh `Rc` closure capturing current
  services + base factory). It is never stored on the context, so no self-referential
  `Rc` cycle is possible by construction.

## Analysis (executed in `/tmp/dc-proto-c`, module `trait_nesting`)

- Three nested `AppContext` generations (`generation` counter + shared `logger`) built
  through the same generic `child_context`; services propagate, types preserved.
- Dropping all ancestors leaves descendants fully usable (no factory cycle).
- `Rc<AppContext>` navigation-style storage with no `C: Clone` bound.
- Custom context holds a genuinely non-`Clone` field and still implements the trait.
- Rejected (trait `from_parts`): incoherent for service-bearing types (the F-01 finding).
- Rejected (stored factory): invites self-referential cycles for zero benefit; computing
  per call costs one `Rc` allocation per `child_context`, at UI-tree rates.
- Rejected (`C: Clone` bound, again): still unnecessary.

## Consequences

Phase 1 implements the five-method trait with provided `child_context` methods, the
`Clone` default struct, and a non-`Clone` service-bearing custom-context example with
tests. Navigation binds on the trait and constructs exclusively via `context_factory`.
