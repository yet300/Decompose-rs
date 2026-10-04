# ADR-0001: ComponentContext as trait + default struct

Date: 2026-10-04. Amended: 2026-10-04 (Phase 0.5: `Clone` bound removed).
Upstream: `arkivanov/Decompose@00840bc`. Status: **Accepted**.

## Context

Upstream models context as `GenericComponentContext<out T>` (F-bounded, covariant) with
`ComponentContext` as the default instantiation, so user-defined contexts with extra
services flow through navigation untouched. Upstream contexts are NOT clonable; a new
context means invoking the detached factory. The Rust port must preserve custom contexts
without Kotlin inheritance, and must not impose a `Clone` constraint upstream never had:
a custom context may legitimately hold non-`Clone` services/state.

## Decision

**Trait `ComponentContext: Sized` (no `Clone` bound) + concrete `DefaultComponentContext`.**
Navigation is generic (`fn navigate<C: ComponentContext>`) and uses `Rc<C>` wherever
shared ownership is needed; `ContextFactory<C>: Clone` (an `Rc` closure) is the clonable
thing, not `C`. `DefaultComponentContext` implements `Clone` itself as a shared-handle
convenience (clones share one scope); custom contexts implement `Clone` only if their own
design wants it.

## Analysis

- *Prototype* (`/tmp/dc-proto-a`, real `essenty-* 0.1.0` deps): generic `build_children<C>`
  with no `Clone` bound holds, constructs, and retains both a `Clone` default context and a
  custom context holding a genuinely non-`Clone` field (`Subscription` guard) via `Rc<C>`
  storage and an `Rc<dyn Fn(ContextParts) -> C>` factory. Negative probe confirms the
  custom context does NOT implement `Clone` while remaining fully usable.
- *Object safety*: `Self`-returning methods make the trait non-`dyn`-compatible — accepted,
  because no call site needs `dyn` (engine holds concrete `C` or `Rc<C>`).
- *Generic child factories*: associated `ContextFactory<C> = Rc<dyn Fn(ContextParts) -> C>`
  keeps factory types nameable and clonable per `C`.
- *Custom contexts*: delegation pattern verified against upstream docs recipe; the factory
  closes over shared services (`Rc<Logger>`-style) while per-scope state (guards, handles)
  stays per-instance and possibly non-`Clone`.
- *Lifetimes/mockability*: no lifetimes in the trait (owned `String` keys, `Rc` handles);
  test doubles implement the trait directly.
- Rejected (bare struct only): cannot carry user services without a global type or `dyn Any`
  maps. Rejected (F-bound generics): reproducing `out T` covariance via associated types
  adds complexity for zero additional expressiveness. Rejected (`C: Clone` bound): proven
  unnecessary by prototype; would forbid legitimate non-`Clone` services.

## Consequences

Phase 1 implements the trait (no `Clone` bound), the `Clone` default struct, and one
non-`Clone` custom-context example with tests. Future navigation generics bind on the trait
and store `Rc<C>`; no redesign expected.
