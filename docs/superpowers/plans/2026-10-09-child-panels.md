# ChildPanels implementation plan

> For agentic workers: use superpowers:subagent-driven-development with one
> source/tests owner, then independent spec and quality reviewers. Use TDD and
> verification-before-completion. Controller owns docs/examples/README/Cargo metadata.

Goal: implement the authorized typed Main/Details/Extra router in one crate.
Architecture: private tagged enums adapt heterogeneous roles to the common
engine; model policy supplies modes, Back, callback and persistence. Existing
router contracts and byte formats remain compatible.
Tech stack: Rust 1.96/2024, four registry Essenty 0.1.0 primitives and thiserror.
Design: docs/superpowers/specs/2026-10-09-child-panels-design.md.

## 1. Typed Panels state, policy and navigation

Files: create src/router/panels/{mod,engine,navigation,state}.rs,
tests/child_panels.rs; modify src/router/{mod,engine}.rs and existing model policy
configurations() signatures in pages, stack and slot.

- [x] Write a failing mode/identity test using child_panels, Panels::new,
  ChildPanelsMode::Single/Dual/Triple and distinct typed factories; run
  `rtk cargo test --test child_panels` and establish RED before implementation.
- [x] Implement the public structs/enums, manual Clone and no-extra helper.
- [x] Adapt configuration collection to owned Rc vectors and private role-tagged
  config/component enums. Keep scope creation/transactions in the shared engine.
- [x] Implement twelve mode/presence cases, same-role identity matching, factory
  forward/destruction reverse order and parent-bounded statuses.
- [x] Implement navigate/_with and all named helper/_with operations, preserving
  independently absent optional roles and numeric/mode state per receiver.
- [x] Add tests for heterogeneous non-Clone roles, identical values across roles,
  helpers, equality identity and shared-source callbacks; run focused and all
  baseline tests after the policy signature change.

Example initial behavioral assertion:

```rust
let nav = PanelsNavigation::new();
let mut initial = Panels::new(1u8);
initial.details = Some(Rc::new("details".to_owned()));
initial.extra = Some(Rc::new(true));
let value = child_panels(&root, &nav, initial, PanelsOptions::default(),
    |_, ctx| ctx, |_, ctx| ctx, |_, ctx| ctx).unwrap();
assert_eq!(value.get().main.instance.lifecycle().state(), LifecycleState::Created);
let old = value.get();
nav.set_mode(ChildPanelsMode::Triple);
assert!(Rc::ptr_eq(&old.main.instance, &value.get().main.instance));
assert_eq!(value.get().main.instance.lifecycle().state(), LifecycleState::Resumed);
```

## 2. State hook, Back and persistence

- [x] Add default no-op on_state_changed model hook; notify initial state with
  None after provider/Back installation while queue remains initializing, and
  notify committed events after Value publication, before completion. Test
  old/new state, no-op events, automatic Back and all reentrant hook stages.
- [x] Implement Single-only automatic Back; test child precedence, parent-bound
  gating, Dual/Triple fallback, predictive cancel, dispatch deferral and retries.
- [x] Implement typed PanelsCodec and no-extra adapter, DPN1 framing and strict
  structural decode before callbacks. Add failures for every structural field,
  decoded role mismatch, trailing/nested data and exhausted counters.
- [x] Test persisted mode/membership/nested state, retained service identity,
  transient reset, coherent provider reentry and codec save failures.
- [x] Test reservation before user callbacks, restored-input retry on failures,
  keeper type conflict, provisional-scope cleanup and post-panic source usability.
- [x] Cover mixed/nested routers, custom context services, views/source drops and
  old DST1/DSL1/DPG1 fixtures. Do not edit controller-owned docs or commit code.
- [x] Run `rtk cargo test --all-features`, fmt and strict all-target Clippy; report
  actual counts and freeze src/tests for independent review.

## 3. Documentation, example and independent reviews

- [x] Controller adds examples/child_panels.rs with distinct config/component
  types, mode changes preserving identity, Single-only Back and mode/state restore.
- [x] Update README, Cargo description, roadmap and ADR-0008; record exact Rust
  API, callback ordering and original pinned reference.
- [x] Read-only fresh spec reviewer verifies design against actual code/tests.
- [x] Sole implementer fixes concrete spec gaps and reviewer rechecks.
- [x] Read-only fresh quality reviewer examines transactions/borrows/lifetimes,
  typed projection, Back and parser; substantiate important findings with probes.
- [x] Fix and re-review findings before completion.

## 4. Final verification

- [x] `rtk cargo test --all-targets --all-features` and `rtk cargo test --doc --all-features`.
- [x] `rtk cargo fmt --all -- --check` and
  `rtk cargo clippy --all-targets --all-features -- -D warnings`.
- [x] `rtk proxy env RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features`.
- [x] Run child_stack, child_slot, child_pages and child_panels examples.
- [x] `rtk cargo check --lib --target <target>` for all 24 installed targets.
- [x] Check final diff; record actual results/reviews and deliver local changes.

Progress: complete. Independent specification and quality reviews passed without
findings; controller verification and documentation are complete. The previous
ChildPages stage was committed as a1f675b. ChildPanels was subsequently committed
as 0336a19 (`feat: implement typed ChildPanels with lifecycle modes`) at the
user's request before starting ChildItems. No merge or push was performed.

## Verification record (2026-10-09)

Controller independently verified the implementation, rather than relying on the
implementer's report:

- All-target/all-feature tests: 262 passed (121 unit, 40 Pages integration,
  35 Panels integration, 35 Slot integration, 27 Stack integration and 4 context
  integration). Doc tests: 1 passed. Total: 263.
- Existing 221 tests remain green. Added 35 Panels integration tests, 6 DPN1
  unit/fixture tests and a literal pre-Panels DPG1 fixture test: 42 new tests.
- Format check, all-target/all-feature Clippy with warnings denied and rustdoc
  with warnings denied passed.
- All four router examples ran successfully, including Panels mode/identity,
  automatic Back, and restoration of Dual mode with child state.
- Library compilation passed every installed target listed below. These checks
  establish compilation, not execution on each platform.

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

## Review record

Specification review: PASS, high confidence, no concrete gaps. The independent
reviewer inspected the design, all four Panels modules, shared engine changes,
pinned upstream contracts and all Panels tests. Its own test runs reproduced
262 all-target tests plus 1 doctest, with 35 Panels integration and 6 DPN1 tests.
Old codec/navigation/source implementations are unchanged; the Pages test change
adds only the literal compatibility fixture.

Quality review: APPROVED, high confidence, no actionable Critical, Important or
Minor findings. The separate reviewer inspected every production Panels module,
shared engine switch/build/cleanup/save logic, old-model changes, all Panels tests
and the DPG1 fixture. Its fresh runs independently reproduced 262 all-target tests
and 1 doctest, plus focused Panels and DPN1 checks. No code fixes were required.

Controller repeated the all-target/all-feature and doc tests, formatting,
strict Clippy, strict rustdoc and all four examples after source freeze. Final
`git diff --check` passed. Dependencies remain unchanged.
