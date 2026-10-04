# ADR-0001: ComponentContext as trait + default struct

Date: 2026-10-04. Upstream: `arkivanov/Decompose@00840bc`. Status: **Accepted**.

## Context

Upstream models context as `GenericComponentContext<out T>` (F-bounded, covariant) with
`ComponentContext` as the default instantiation, so user-defined contexts with extra
services flow through navigation untouched. The Rust port must preserve that capability
without Kotlin inheritance. Candidates: (A) trait-based context, (B) single concrete struct,
(C) generic/associated-context abstraction mirroring the F-bound.

## Decision

**Trait `ComponentContext: Sized + Clone` + concrete `DefaultComponentContext`.**
Navigation is generic (`fn navigate<C: ComponentContext>`); custom contexts implement the
trait by delegation and close extra services over the factory. No F-bound generics, no `dyn`.

## Analysis

- *Object safety*: `Self`-returning methods make the trait non-`dyn`-compatible — accepted,
  because no call site needs `dyn` (engine holds concrete `C` or `Rc<C>`).
- *Generic child factories*: associated `ContextFactory<C> = Rc<dyn Fn(ContextParts) -> C>`
  keeps factory types nameable and clonable per `C`.
- *Custom contexts*: delegation pattern verified against upstream docs recipe; prototype
  (`/tmp/decompose-proto`, kept out of the repo) confirms `Rc<dyn Fn> + Clone` compiles
  and `Weak` breaks parent/child cycles.
- *Lifetimes/mockability*: no lifetimes in the trait (owned `String` keys, `Rc` handles);
  test doubles implement the trait directly.
- Rejected (B): a single struct cannot carry user services without a global type or
  `dyn Any` maps — both worse than a trait. Rejected (C): reproducing `out T` covariance
  via associated types adds complexity for zero additional expressiveness in Rust.

## Consequences

Phase 1 implements the trait, the default struct, and one custom-context example with tests.
Future navigation generics bind on the trait; no redesign expected.
