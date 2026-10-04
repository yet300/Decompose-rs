# ADR-0002: Context factory as `Rc<dyn Fn(ContextParts) -> C>`

Date: 2026-10-04. Amended: 2026-10-04 (Phase 0.5: scope-lifetime rule added;
Phase 0.6: `ContextScope` removed as REDUNDANT; factory consumes whole `ContextParts`).
Upstream: `arkivanov/Decompose@00840bc`. Status: **Accepted**.

## Context

Upstream `ComponentContextFactory<out T>` is a SAM interface taking four fully-formed
dependencies and returning a detached `T`. Rust candidates: generic `Fn` bounds, `Rc<dyn Fn>`,
factory trait, associated factory type, constructor abstraction. Phase 0.5 additionally
required accident-proof forwarding of the prepared child scope; Phase 0.6 achieves it by
making `ContextParts` non-constructible and non-destructurable downstream, with delegation
(not destructuring) as the canonical factory pattern (see ADR-0003 for lifetime).

## Decision

**`pub type ContextFactory<C> = Rc<dyn Fn(ContextParts) -> C>`**, where `ContextParts` is
consumed by value and exposes **no public fields**: four private handle fields
(`lifecycle`, `state`, `instances`, `back`) with read/clone accessors only, and no public
constructor. There is deliberately **no `ContextScope` struct** (Phase 0.6/F-01
classification: REDUNDANT — the four handles *are* the scope; true lifetime is
parent-held per ADR-0003, and the "scope enforcement" argument for a separate struct was
overstated). Closures first; a factory trait only if a future generic demands named
factory types.

Factory-safety rules (executed in `/tmp/dc-proto-b` and `/tmp/dc-proto-c`):

1. `ContextParts` has no public constructor and no public fields; handles are obtained
   through read/clone accessors
   (`lifecycle()` / `state_keeper()` / `instance_keeper()` / `back_handler()`). The
   sanctioned sink for the whole `parts` value is a base factory call (`base(parts)`) or a
   concrete inherent `from_parts(parts)` — never destructuring.
2. `DefaultComponentContext::from_parts` is an inherent (non-trait) constructor; custom
   factories delegate to a base factory rather than rebuilding from handles.
3. Canonical custom-context pattern — delegate, never destructure:
   ```rust
   let base = base_factory.clone();
   let factory: ContextFactory<AppContext> = Rc::new(move |parts| {
       AppContext { delegate: base(parts), logger: logger.clone() }
   });
   ```
   Since `ContextParts` cannot be constructed or destructured downstream, the ergonomic
   path always forwards the prepared scope.
4. Deliberately wrong code (`|_parts| Custom::new(...)`) can still ignore scope — accepted:
   the bar is *accidental* misuse, which rules 1–3 eliminate. Our own factories are covered
   by the scope-survival parity test (parent save/bridge/merged-subscription live after the
   factory returns with all temporaries dropped).

## Analysis

- *Object safety/allocation*: `Rc<dyn Fn>` is object-safe and clonable (one allocation per
  factory, shared across all children of a scope).
- *Custom types*: closures capture extra services (`logger.clone()`), mirroring the upstream
  decorator recipe; no per-type boilerplate. No `Clone` on `C` required (ADR-0001).
- *Nested creation*: engine calls the same `Rc` per child with different `ContextParts`;
  ownership is explicit (parts are owned values, no lifetimes).
- *Testability*: tests substitute `Rc::new(|parts| TestContext::from_parts(parts))` trivially.
- *`'static` providers*: closures retained in Essenty keepers must be `'static`; therefore
  the public boundary takes `&str` keys but everything captured is an owned `String`
  (compiler-enforced; hit once in prototyping and codified here).
- Rejected (factory trait / associated type): more machinery with no consumer. Rejected
  (generic `F: Fn` bounds on context structs): infects every holder with a type parameter.

## Consequences

`ContextParts` is consumed by value with no public fields; handles travel as owned
`Rc`-style clones via accessors. Documented on the type; enforced by tests.
