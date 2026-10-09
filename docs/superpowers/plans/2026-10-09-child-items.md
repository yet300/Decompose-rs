# ChildItems Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development with one
> src/tests owner, followed by fresh independent specification and quality reviewers.
> Use TDD. Controller owns docs, README, Cargo description and example.

**Goal:** Add Eq-only lazy ChildItems with flexible activity and durable tombstones.
**Architecture:** An Items Model reuses the private engine; a weak receiver control
provides local navigation and checked synchronous activation without broadcasts.
A small Eq-key ItemMap preserves configuration identity without Hash bounds.
**Tech Stack:** Rust 1.96/edition 2024, four registry Essenty 0.1 primitives, thiserror.
**Design:** ../specs/2026-10-09-child-items-design.md. Base: 0336a19.

## 1. Membership, equality map and lifecycle policy

Create src/router/items/{mod,map,navigation,state}.rs and tests/child_items.rs;
modify src/router/mod.rs to export items and add lazy lookup error variants.

- [x] Add the initial failing zero-factory lazy/status test and establish RED with
  `rtk cargo test --test child_items`; no production feature before RED.

```rust
use decompose_rs::router::items::{child_items, Items, ItemsNavigation, ItemsOptions};
use decompose_rs::{ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState};
use std::{cell::Cell, rc::Rc};

let lifecycle = LifecycleRegistry::new();
lifecycle.move_to(LifecycleState::Resumed).unwrap();
let root = DefaultComponentContext::new(lifecycle.clone());
let counter = Rc::new(Cell::new(0));
let calls = counter.clone();
let nav = ItemsNavigation::new();
let items = child_items(&root, &nav, Items::new(vec![1u8, 2]).unwrap(),
    ItemsOptions::default(), move |_, ctx| { calls.set(calls.get() + 1); ctx }).unwrap();
assert!(items.get().active_items.is_empty());
assert_eq!(counter.get(), 0);
let first = items.get_or_create(&1).unwrap();
assert_eq!(first.lifecycle().state(), LifecycleState::Created);
assert!(Rc::ptr_eq(&first, &items.get_or_create(&1).unwrap()));
lifecycle.destroy().unwrap();
```

- [x] Implement ItemMap equality semantics, owned/Rc construction, manual Clone
  and map operations; test Eq-only configs, equal replacement and lookup/removal.
- [x] Implement Items/ChildItems/ActiveChild/ActiveLifecycleState/ItemsOptions,
  duplicate validation, normalization and model status/snapshot mapping.
- [x] Implement ItemsNavigation and ItemsNavigator navigate/set_items/
  set_active_items with completions; all trait methods also have inherent access.
- [x] Test three requested lifecycle targets/parent bounding, equality/reordering,
  helpers/completions/no-op publication, duplicate rejection and active pruning.

## 2. Receiver-local lazy control, Back and ownership

Modify private src/router/engine.rs minimally; keep existing build return through
an adapter wrapper. Add a build_with_control path or equivalent weak receiver
control, dispatching Events into the same queue with an optional synchronous gate.

- [x] Test receiver-local isolation and lazy navigation after source drop; RED
  before implementing control/handle methods.
- [x] Implement weak control, regular local enqueue, idle/Back/initialization check
  for synchronous activation, destroyed checks, get/value/subscribe and lazy lookup.
- [x] Test unknown/busy/self-removing lazy fetch and active reads; local recursion
  from observers/factories/lifecycle/completion stays FIFO and error-recoverable.
- [x] Test inactive state-preserving fresh instance/keeper creation, removal/readd
  state loss, external retention, dropped view/source, teardown and all user panic
  stages. Navigation control must not create parent/value/engine Rc cycles.
- [x] Test Back only through Started/Resumed live children, Created gating,
  parent bounding, predictive cancellation and deferred scope mutations.

## 3. DPI1 codec and adversarial persistence

- [x] Add roundtrip/inactive restore failing tests before implementing codec.
- [x] Implement typed list codec and exact DPI1 frame from the specification.
  Validate every structural child field before calling user decode; validate
  decoded count/unique configs before factories. Reuse DCS1 nested parser.
- [x] Add immutable DPI1 byte fixture and malformed-field/pre-callback tests,
  including status/UTF8/key duplicates/counter/truncation/nested/trailing and
  codec count/duplicates/failure. Keep all existing frozen fixture bytes.
- [x] Test authoritative empty/all-inactive restore, child states, repeated
  tombstone recreation/save, external retention/transient reset, input rollback,
  provider reservation/type conflict and coherent save during provider navigation.
- [x] Test non-Clone custom context propagation and mixed/nested models.
- [x] Run all baseline plus new tests, fmt and strict Clippy; self-review and freeze
  src/tests. Report concrete evidence and any concerns; do not commit code.

## 4. Controller docs and independent review

- [x] Add runnable examples/child_items.rs: non-Clone config, lazy fetch, active
  viewport/lifecycle changes, deactivation/recreation with saved state, restored
  membership/active targets and no automatic Back.
- [x] Update README/Cargo description/roadmap/lib scope and ADR-0009; previous
  Panels plan records actual commit 0336a19.
- [x] Fresh read-only spec reviewer checks actual code vs complete design and
  pinned upstream contracts. Sole implementer fixes any concrete gaps; re-review.
- [x] After SPEC PASS fresh read-only quality reviewer checks all code/borrows/
  control lifetimes/persistence and regressions. Fix/re-review concrete findings.

## 5. Final gates and delivery

- [x] `rtk cargo test --all-targets --all-features` and doc/all-feature tests pass.
- [x] `rtk cargo fmt --all -- --check` and
  `rtk cargo clippy --all-targets --all-features -- -D warnings` pass.
- [x] `rtk proxy env RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features`.
- [x] Run child_stack, child_slot, child_pages, child_panels and child_items examples.
- [x] `rtk cargo check --lib --target <target>` for all 24 installed targets.
- [x] Final diff/status; record counts/platforms/review evidence and deliver local
  ChildItems changes on codex/child-items, preserving the previous-stage commit.

Progress: complete. Source/tests and controller artifacts are finalized.
Independent specification and quality reviews passed, including re-review of the
corrected stale-snapshot lazy lookup. Controller gates passed after the correction. No ChildItems commit, merge or push was performed.
ChildPanels was committed as 0336a19 at the user's request before this stage.

## Verification record (2026-10-09)

Controller independently verified source/test changes and ran:

- `rtk cargo test --all-targets --all-features`: 325 passed (121 unit, 63 Items,
  40 Pages, 35 Panels, 35 Slot, 27 Stack and 4 context integration tests).
- `rtk cargo test --doc --all-features`: 1 passed; total: 326 tests. The complete
  263-test baseline remains green; 63 meaningful Items tests were added.
- Format check, strict all-target/all-feature Clippy and strict rustdoc: passed.
- All five router examples: passed. The Items example verified same-key fresh
  component/state recreation, retained counter 17, configuration-based reorder,
  three requested targets bounded by parent, no automatic Back, and restored
  order/active targets plus inactive child state.
- `rtk cargo check --lib --target <target>` passed all 24 installed targets below.
  These are compile checks, not platform runtime tests.

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

## Implementation evidence and correction

Initial RED: the missing router::items API failed the new zero-factory/lazy test.
Persistence RED: three tests failed against deliberate unimplemented codec
stubs; GREEN added exact DPI1 framing, restoration and frozen-byte fixture.

Self-review identified equal configurations whose Eq ignores payload fields.
A regression test reproduced fresh factories receiving old instead of new data.
Pinned ChildController.activate supplies the current configuration when a
Destroyed child is recreated. The fix makes Items opt into refreshing the reused
Managed.configuration handle; live component/key identity remains unchanged.
Previous models use the default false policy. Both initially inactive and
previously deactivated entries now receive current data on fresh activation.
Tests verify RED/GREEN, canonical map/Child handles and old-model regressions.

## Independent reviews

Specification review: SPEC PASS, high confidence, no actionable compliance gaps.
The fresh reviewer read the complete design, all new production code, full engine
transaction/retention/teardown paths, DCS1 and all 59 Items tests, then compared
pinned upstream Items/ChildController. Its independent focused run passed 59 tests.
Canonical handles/fresh payload input, lazy ownership/error ordering, Back/queues,
strict pre-codec validation, rollback/persistence and old model preservation match
the design. Old model source/state/acceptance files have no diff against 0336a19.

Quality review: changes requested (one Important/P2 finding, high confidence).
The separate reviewer independently reproduced get_or_create returning an ended
component after a caught destructive lifecycle panic; the published old active
snapshot remained while the scope and keeper had already ended. Controller
reproduced the same failure using the reviewer's temporary path-dependent probe.
The fix checks weak-control scope availability before both active and post-sync
lazy returns, yielding ChildUnavailable for ended/unavailable entries while
preserving valid active reads. Four new regressions first reproduced the failure, then passed with the fix:
immediate fetch after caught outgoing/incoming lifecycle panic; caught manual
child lifecycle destruction; destroyed or exclusively borrowed keeper, with valid
same-instance fetch restored after releasing the borrow; and observer destruction
between synchronous activation and final lazy lookup. Active snapshot checks and
post-sync checks both verify actual local/merged lifecycle and keeper liveness.
No activation or publication occurs on an unavailable active fast path.

Specification re-review: SPEC PASS, high confidence. The reviewer independently
checked updated error semantics and weak-control ownership, then passed all 63
Items tests. Previous specification review remains valid for the other contracts.
Controller repeated all-target/doc tests, fmt, strict Clippy/rustdoc, all five
examples and all 24 target compile checks after the source fix; all passed.

Quality re-review: APPROVED, high confidence, no remaining actionable findings.
The reviewer independently inspected actual scope/keeper checks, safe borrowing,
Weak control ownership and both getter paths. Its adapted original probe now
passes: ended scope returns ChildUnavailable, explicit repair creates a fresh
live Resumed child, keeper borrow conflict returns ChildUnavailable and releasing
it restores same-instance lookup, manual child destruction is rejected and parent
destruction returns Destroyed. Independent runs passed 63 Items tests, 325 all-target
tests, 1 doctest and formatting. No repository changes were made by reviewers.

Final controller `git diff --check` passed. Dependencies remain unchanged. Changes
stay local and uncommitted on codex/child-items; the previous ChildPanels stage is
committed as 0336a19. No merge or push was performed.
