# ADR-0002: Context factory as `Rc<dyn Fn(ContextParts) -> C>`

Date: 2026-10-04. Upstream: `arkivanov/Decompose@00840bc`. Status: **Accepted**.

## Context

Upstream `ComponentContextFactory<out T>` is a SAM interface taking four fully-formed
dependencies and returning a detached `T`. Rust candidates: generic `Fn` bounds, `Rc<dyn Fn>`,
factory trait, associated factory type, constructor abstraction.

## Decision

**`pub type ContextFactory<C> = Rc<dyn Fn(ContextParts) -> C>`** with `ContextParts`
(lifecycle, state keeper, instance keeper, back handler) prepared by the caller.
Closures first; a factory trait only if a future generic demands named factory types.

## Analysis

- *Object safety/allocation*: `Rc<dyn Fn>` is object-safe and clonable (one allocation per
  factory, shared across all children of a scope) — prototype-verified.
- *Cloneability*: required (factories live in clonable contexts and navigators); `Rc`
  gives it for free, unlike `Box<dyn Fn>` or bare generics at struct fields.
- *Custom types*: closures capture extra services (`logger.clone()`), exactly mirroring the
  upstream decorator recipe; no per-type boilerplate.
- *Nested creation*: engine calls the same `Rc` per child with different `ContextParts`;
  ownership is explicit (parts are owned values, no lifetimes).
- *Testability*: tests substitute `Rc::new(|parts| TestContext::new(parts))` trivially.
- Rejected (factory trait / associated type): more machinery with no Phase-1 consumer;
  rejected (generic `F: Fn` bounds on context structs): infects every holder with a type
  parameter for no benefit.

## Consequences

`ContextParts` fields are owned `Rc`-style handles. The factory contract (detached
assembly; caller prepares scoping) is documented on the type and enforced by tests.
