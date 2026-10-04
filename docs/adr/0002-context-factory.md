# ADR-0002: Context factory as `Rc<dyn Fn(ContextParts) -> C>`

Date: 2026-10-04. Amended: 2026-10-04 (Phase 0.5: scope-lifetime rule added).
Upstream: `arkivanov/Decompose@00840bc`. Status: **Accepted**.

## Context

Upstream `ComponentContextFactory<out T>` is a SAM interface taking four fully-formed
dependencies and returning a detached `T`. Rust candidates: generic `Fn` bounds, `Rc<dyn Fn>`,
factory trait, associated factory type, constructor abstraction. Phase 0.5 additionally
requires: a factory must not be able to *accidentally* discard the hidden scope-lifetime
state (`ContextScope`) that keeps a child scope attached (see ADR-0003).

## Decision

**`pub type ContextFactory<C> = Rc<dyn Fn(ContextParts) -> C>`**, where `ContextParts` is
consumed by value and exposes **no public fields**: four private handle fields plus a
private `scope: ContextScope` field. Downstream code can inspect/clone the four usable
handles through read/clone accessors only; it cannot construct `ContextParts` manually and
cannot access `ContextScope`. Closures first; a factory trait only if a future generic
demands named factory types.

Lifetime-safety rules (prototype-validated in `/tmp/dc-proto-b`):

1. `ContextParts` has no public constructor, no public fields, and no `scope` accessor;
   the only way to obtain handles is through read/clone accessors
   (`lifecycle()` / `state_keeper()` / `instance_keeper()` / `back_handler()`), and the
   only sanctioned sink is `C::from_parts(parts)` / a base-context factory that retains
   `parts.scope`.
2. `DefaultComponentContext::from_parts` stores `parts.scope`; it cannot be constructed from
   bare handles without going through `ContextParts`.
3. Canonical custom-context pattern — delegate, never destructure:
   ```rust
   let base = base_factory.clone();
   let factory: ContextFactory<AppContext> = Rc::new(move |parts| {
       AppContext { delegate: base(parts), logger: logger.clone() }
   });
   ```
   Because `scope` is private, the custom factory cannot strip lifetime state without
   deliberately bypassing `from_parts`; the ergonomic path always forwards it.
4. Deliberately wrong code (`|_parts| Custom::new(...)`) can still drop scope — accepted:
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

`ContextParts` fields for handles are owned `Rc`-style clones; the private `scope` field
travels by value into the constructed context. Documented on the type; enforced by tests.
