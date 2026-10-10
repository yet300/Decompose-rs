# Lazy Initial State and Compatibility Matrix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox syntax.

**Goal:** Add six additive lazy standard-router factories and publish an evidence-based compatibility matrix.
**Architecture:** Eager wrappers delegate to FnOnce factories; the existing engine chooses restored state before initialization. Preserve transport, policies and wire formats. The controller owns documentation and example; one implementer owns all production code and new regression tests.
**Tech Stack:** Rust 1.96/edition 2024, synchronous Rc ownership, existing Essenty 0.1.0 dependencies.

Baseline/main e129ab1: 410 unit/integration tests + 1 doctest. Current branch
codex/lazy-initial-state-compatibility. Reuse the suitable current checkout;
no additional worktree is needed. Normative user-approved spec:
docs/superpowers/specs/2026-10-10-lazy-initial-state-compatibility-design.md.

## Task 1: Six factories and regression suite (one source owner)

Modify src/router/stack/mod.rs and engine.rs, slot/mod.rs, pages/mod.rs,
panels/mod.rs and engine.rs, items/mod.rs. Create tests/lazy_initial_state.rs.
Do not alter codecs, transports, dependencies or engine policies unless a concrete
regression requires a narrowly justified fix. Controller owns src/lib.rs docs.

- [x] Write public API tests before production edits. Include all six functions,
  non-Clone owned captures, borrowed mutable counters and dyn sources.
  ```rust
  let mut calls = 0;
  let nav = StackNavigation::new();
  let source: &dyn NavigationSource<StackEvent<u8>> = &nav;
  let routes = vec![1_u8];
  let stack = child_stack_with_initial::<_, _, _>(
      &context, source,
      || { calls += 1; routes },
      StackOptions::default(), |c, _| *c,
  ).unwrap();
  assert_eq!(calls, 1);
  assert_eq!(*stack.get().active.configuration, 1);
  ```
  Run `rtk cargo test --test lazy_initial_state`; verify missing new APIs are the
  RED cause and record it. Test fixtures use real contexts/keepers and transport.
- [x] Add six `_with_initial` factories with argument-position impl FnOnce,
  existing named generic counts/bounds/source/returns. Move existing build bodies
  to lazy functions and delegate eager functions with `|| initial`.
  Stack builder passes `|| initial().into_iter().map(Rc::new).collect()`;
  Slot uses the same Option iteration; Pages/Items/Panels pass initial directly.
  Two-role Panels delegates to the new three-role lazy factory with
  `|never: &Infallible, _| match *never {}`. No initializer 'static bound.
- [x] Run focused tests GREEN; add all-family no-state evaluation and restored
  suppression tests. Save real family state, recreate the hosting context with
  that state, and supply `|| panic!("restored initializer must be skipped")`.
  Include valid empty Slot/Pages/Items, inactive Items, Panels mode/membership,
  two-role Panels and transient no-codec behavior.
- [x] Verify destroyed/duplicate/corrupt/decoder-error paths never evaluate;
  validate key reuse, exact input preservation and cancellation. Invalid initial
  Stack/Pages/Items must clean registrations and allow same-key retry.
- [x] Verify subscribe/initializer/factory nested emissions share FIFO and that
  completions observe published state. Include Panels initial hook ordering.
  Test one-shot initializer captures are released on evaluation and suppression,
  callback Drop can reenter source/keeper, parent destruction aborts construction,
  initializer panic preserves original payload under custom cancellation panic.
  Combined-panic regressions use subprocess isolation as existing tests do.
- [x] Run focused/full tests, doctest, fmt and strict Clippy. Self-review borrow,
  Drop and ownership boundaries; report actual counts/RED cause and freeze source.
  No commit, branch switch or matrix/example/docs edits by the source owner.

## Task 2: Compatibility evidence and executable example (controller)

Create docs/COMPATIBILITY.md, docs/adr/0013-lazy-initial-state-and-compatibility.md
and examples/lazy_initial_state.rs. Update README.md, docs/PORTING_ROADMAP.md,
src/lib.rs and stage design/plan statuses.

- [x] Compare pinned upstream public APIs with Rust source and existing test
  evidence. Cover contexts/scopes/retention/Back, five families and helper
  operations, sources/traits, Generic Navigation, values and codecs.
- [x] Add matrix rows with pinned source URLs, Rust API/tests and exact statuses.
  Document adaptations and gaps separately: class grouping, bounds/ownership,
  completion/error policy, duplicates, decoder decline, memory-only saver/global
  settings/test-utils, platform APIs. Do not equate Rust wire formats with Kotlin.
- [x] Add runnable example with an owned non-Clone initial route, real save/restore
  via StackCodec and an initializer that would panic if restoration invoked it.
  Assert initial evaluation count and restored child identity/configuration.
- [x] Describe all six factory names, FnOnce borrowed/move captures, zero/one
  evaluations and constructor buffering/rollback. Link matrix and example.

## Task 3: Sequential independent reviews and verification

- [x] Fresh spec review of actual source/tests/matrix/docs against approved design;
  original owner fixes any source finding and reviewer rechecks.
- [x] Fresh whole-stage quality review after spec PASS; repeat after source fixes.
- [x] Controller independently confirms full tests/doctest, fmt, strict Clippy/
  rustdoc, all nine examples, installed library target matrix and diff check.
  Commands: `rtk cargo test --all-targets --all-features`,
  `rtk cargo test --doc --all-features`, `rtk cargo fmt -- --check`,
  `rtk cargo clippy --all-targets --all-features -- -D warnings`,
  `rtk proxy env RUSTDOCFLAGS='-D warnings' cargo doc --all-features --no-deps`,
  `rtk cargo run --example <example>`,
  `rtk cargo check --lib --all-features --target <installed target>`,
  `rtk git diff --check`.
- [x] Record actual final counts, target names, review results and scope limits.
  Local commit was subsequently requested by the user. Keep the feature branch;
  no further merge, remote push or branch deletion was requested.

## Verification before independent reviews (2026-10-10)

Initial RED: the focused test exited 101 with exactly six E0425 missing lazy
factory names. After thin adapters, a new regression reproduced
Context(InstanceKeeper(Destroyed)): initializer evaluation/capture Drop destroyed
the parent before the builder accessed retention. A five-line lifecycle guard in
src/router/engine.rs now returns NavigationError::Destroyed before retention
access. Both evaluated/skipped capture paths are exercised for all six factories.
No state-machine, codec, transport, dependency or model-policy change was needed.

Source owner froze the implementation after 18 focused tests, 428 full tests,
1 doctest, fmt, strict Clippy and diff checks passed. Controller independently
confirmed focused/full tests, doctest, fmt, strict Clippy/rustdoc and diff checks.
All nine examples executed successfully, including real restoration without
initial evaluation in lazy_initial_state. Matrix audit found no missing local
links, undefined references or missing pinned upstream source files (38 rows,
33 upstream reference links). Independent spec and quality reviews passed.

`cargo check --lib --all-features --target <target>` passed every installed target
below. These checks do not claim runtime execution on other hosts.

```text
aarch64-apple-darwin
aarch64-apple-ios
aarch64-apple-ios-macabi
aarch64-apple-ios-sim
aarch64-apple-tvos
aarch64-apple-tvos-sim
aarch64-apple-visionos
aarch64-apple-visionos-sim
aarch64-apple-watchos
aarch64-apple-watchos-sim
aarch64-linux-android
aarch64-pc-windows-msvc
aarch64-unknown-linux-gnu
aarch64-unknown-linux-musl
armv7-linux-androideabi
i686-linux-android
wasm32-unknown-unknown
x86_64-apple-darwin
x86_64-apple-ios
x86_64-apple-ios-macabi
x86_64-linux-android
x86_64-pc-windows-msvc
x86_64-unknown-linux-gnu
x86_64-unknown-linux-musl
```

## Independent specification review (2026-10-10)

PASS, high confidence, no missing requirement or unapproved scope. The reviewer
read actual APIs/adapters/engine/test suite/example/documentation and validated
all 38 compatibility rows against primary pinned sources and Rust evidence.
All links resolve and remaining capabilities are explicitly qualified. It
independently passed 18 focused tests, all 428 tests, 1 doctest and the lazy
example. No implementation repair was required after this review.

## Independent quality review and readiness (2026-10-10)

PASS, high confidence, no Critical, Important or Minor findings. The fresh quality
reviewer read every production change, complete new test suite, example, matrix,
ADR/spec/plan and README/roadmap. It checked capture-drop timing, lifecycle-guard
placement, construction rollback, cancellation, retry bytes, eager compatibility
and pinned upstream persistence/settings semantics. It independently passed 18
focused tests, all 428 tests, 1 doctest, the restoration example and diff check.
No source repair was required after either independent review.

The stage is ready on codex/lazy-initial-state-compatibility. The user subsequently
requested a local commit. Main remains e129ab1. No new dependency, codec/transport change,
remote push, further merge or branch deletion was performed.
