# Decompose-rs

Independent Rust port of [Ark Ivanov's Decompose](https://github.com/arkivanov/Decompose)
— component-context foundation only.

> **Status: Phase 1 / early development.**
> `ComponentContext` (lifecycle, state, retention, back handling, child
> scoping) is implemented with upstream semantic parity.
> **ChildStack/navigation is NOT implemented yet.**

## Relationship to upstream

Decompose-rs preserves *observable behavioral semantics* of upstream
Decompose (research baseline `master@00840bc`), reimplemented in idiomatic
Rust on top of [Essenty-rs](https://github.com/yet300/Essenty)
(`essenty-lifecycle`, `essenty-state-keeper`, `essenty-instance-keeper`,
`essenty-back-handler` — registry-only `0.1.0` dependencies, no path/git
overrides). It is not a copy of the upstream implementation and makes no
claim of official affiliation; see `NOTICE`.

Research and architecture documents live in `docs/` (including ADRs and the
upstream behavior contract).

## Supported principles

- Synchronous, runtime-agnostic core: no async, no Tokio, no `serde` in core.
- Single-threaded UI-tree model: `!Send + !Sync`, `Rc`-based like Essenty-rs.
- Parent-held scope lifetime; lifecycle-driven teardown (never drop-driven).
- Custom service-bearing contexts flow through navigation untouched, with no
  `Clone` bound and no casts.
- Minimal producing-error surface (`DecomposeError`): every variant has a
  real producing path, a test, and documentation.

## Quick sketch

```rust
use decompose_rs::{ComponentContext, DefaultComponentContext, LifecycleRegistry};

let lifecycle = LifecycleRegistry::new();
lifecycle.create().unwrap();
let root = DefaultComponentContext::new(lifecycle);

let child = root.child_context("details").unwrap();

let manual = LifecycleRegistry::new();
let modal = root.child_context_with_lifecycle("modal", &manual).unwrap();
// manual.destroy() after attachment panics by contract:
// "The lifecycle of a child ComponentContext must never be destroyed manually."
```

## Quality gates (Rust 1.96.0)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo test --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

## License

Apache-2.0 — see `LICENSE-APACHE` and `NOTICE`.
