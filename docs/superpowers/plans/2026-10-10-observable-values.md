# Observable Values Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox syntax.

**Goal:** Complete cached derived values, lifecycle bindings and Rc identity updates.
**Architecture:** Existing value API plus private mapping/lifecycle helpers; one crate.
**Tech Stack:** Rust 1.96/edition2024, Rc/RefCell, existing Essenty0.1.0.

Baseline d27d289: 360 all-target tests + 1 doctest. Normative contracts are in
docs/superpowers/specs/2026-10-10-observable-values-design.md.

## Task 1: Implementation, one source owner
Files: src/value.rs, private src/value/{mapped,lifecycle}.rs if useful,
tests/observable_values.rs. Controller owns docs/example/lib documentation.

- [x] Add and run RED identity/update tests before implementation.
  ```rust
  let source = MutableValue::new(3);
  let expected = source.get();
  let next = Rc::new(4);
  assert!(source.compare_and_set(&expected, next.clone()));
  assert!(Rc::ptr_eq(&source.get(), &next));
  assert!(!source.compare_and_set(&expected, Rc::new(5)));
  let result = source.update_and_get(|old| Rc::new(*old + 1));
  assert_eq!(*result, 5);
  ```
  Run rtk cargo test --test observable_values; expect missing methods, then GREEN.
- [x] Add RED cached mapping tests, implement minimal private derived backend.
  ```rust
  let source = MutableValue::new(2);
  let calls = Rc::new(Cell::new(0));
  let count = calls.clone();
  let mapped = source.value().map(move |value| {
      count.set(count.get() + 1); value * 10
  });
  let first = mapped.get();
  assert!(Rc::ptr_eq(&first, &mapped.get()));
  assert_eq!(calls.get(), 1);
  source.set(3);
  assert_eq!(*mapped.get(), 30);
  assert_eq!(calls.get(), 2);
  ```
  Extend tests with real subscriber identity/counts, chains, unsubscribe/detach,
  no-payload-bounds, mapper panic and newer-cache reentry ordering.
- [x] Add RED all-mode lifecycle tests and implement guarded lifecycle binding.
  ```rust
  let lifecycle = LifecycleRegistry::new();
  let source = MutableValue::new(1);
  let seen = Rc::new(RefCell::new(Vec::new()));
  let out = seen.clone();
  let _binding = source.value().subscribe_with_lifecycle(
      &lifecycle, ObserveLifecycleMode::StartStop,
      move |value| out.borrow_mut().push(*value));
  source.set(2);
  assert!(seen.borrow().is_empty());
  lifecycle.move_to(LifecycleState::Started).unwrap();
  assert_eq!(*seen.borrow(), vec![2]);
  ```
  Use valid Essenty transitions in final tests. Cover gate shutdown during replay,
  destroy/cancel/detach, captured-resource release, setup panic and reentrant save.
- [x] Run focused/full tests, fmt/strict Clippy; self-review contracts, report RED
  evidence and freeze source. Leave stage uncommitted for independent reviews.

## Task 2: Controller documentation and example
- [x] Add examples/observable_values.rs demonstrating cache identity, CAS, reentrant
  update retry and lifecycle gating on a real LifecycleRegistry.
- [x] Update README, ADR0011, roadmap and lib docs, including known panic limits.
- [x] Inspect implementation against design; no unrelated API or dependency changes.

## Task 3: Sequential reviews, verified commit and merge
- [x] Fresh independent spec review; original owner fixes findings and re-review.
- [x] Fresh whole-stage quality review after spec PASS; repeat both on source fixes.
- [x] Controller independently tests full suite/doc/fmt/strict Clippy/rustdoc,
  seven examples, installed targets and diff check; record actual results.
- [x] Commit completed observable values; merge accumulated branch into main
  after checking ancestry/clean state, preferably fast-forward, verify on main.
- [x] Create codex/router-extension-points from merged main and begin the separate
  authorized router-source/Navigator abstraction stage; no push/branch deletion.

## Verification record (2026-10-10, before independent review)

Controller independently ran: 384 unit/integration tests across nine test suites
and seven example test targets,
1 doctest, formatting, strict Clippy, strict rustdoc and diff whitespace checks.
All seven examples ran successfully. Library compilation passed all 24 installed
Rust targets (Apple platforms, Android, Linux, Windows and wasm32). These are
compile checks; cross-target runtime execution is not claimed.

Commands: `rtk cargo test --all-targets --all-features`,
`rtk cargo test --doc --all-features`, `rtk cargo fmt -- --check`,
`rtk cargo clippy --all-targets --all-features -- -D warnings`,
`rtk proxy env RUSTDOCFLAGS='-D warnings' cargo doc --all-features --no-deps`,
`rtk cargo run --example <each of seven examples>`,
`rtk cargo check --lib --all-features --target <each installed target>`,
`rtk git diff --check`. Independent reviews and commit/merge remain pending.

Initial spec review found stale mapped event delivery after a newer reentrant
replay, plus an idle upstream registration surviving a captured payload's Drop
panic during cancellation. The owner is adding regressions and correcting both;
the review and verification gates must be repeated before commit.

Spec re-review #3: PASS, high confidence. The original captured delivery and
cancellation regressions are fixed. A second review found chained upstream-read
reentry could overwrite a newer downstream cache; revision capture now includes
the upstream read boundary. Two regressions exercise two through four map layers.
All four independent scratch reproductions now have the intended behavior.

Current frozen implementation: 29 observable regression tests; controller and
spec reviewer independently confirm 389 unit/integration tests plus 1 doctest.
Controller repeated strict Clippy/rustdoc, formatting, diff whitespace, all seven
examples and all 24 installed target compile checks after the source fixes.
Quality review, commit and merge remain pending.

### Installed target matrix

All targets below passed library compilation for the final frozen source:

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

Final quality review: PASS, high confidence, no actionable findings. The reviewer
independently ran 389 tests plus 1 doctest, 29 focused tests, strict Clippy/rustdoc,
formatting, diff whitespace and the observable example. Additional independent
probes covered borrowed payload compatibility, reentrant cache payload Drop,
mapped lifecycle rollback with reentrant capture destruction, and active chains
with nested reads/updates. The stage is ready for the authorized local commit
and fast-forward merge into main.

## Delivery

Observable values committed as 9a70ae0. Main fast-forwarded from e96eaf1 to
9a70ae0, including ChildSlot, Pages, Panels, Items and Generic Navigation. The
merged main independently passed 389 tests and 1 doctest and had a clean tree.
Then codex/router-extension-points was created from that main for the authorized
next stage. Previous branches were preserved; no remote is configured.
