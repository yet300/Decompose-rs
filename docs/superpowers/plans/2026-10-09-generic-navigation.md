# Generic Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Allow Rust consumers to create custom navigation models with typed events/state and generic children.
**Architecture:** Public router::children adapter over the shared private engine; neutral child/status types with Pages aliases. Parent-owned runtime and weak custom-source subscribers share the existing queue and lifetime.
**Tech Stack:** Rust 1.96/edition2024, Rc/RefCell, existing essenty-*0.1.0; no new dependencies.

Baseline b1a7d56 on codex/generic-navigation: 325 all-target tests + 1 doctest passed.
Full contracts are in docs/superpowers/specs/2026-10-09-generic-navigation-design.md.

## Task 1: Public generic navigation (one source owner)
Files:
- Create src/router/children/mod.rs (options, model adapter, children factory).
- Create src/router/children/types.rs (neutral types, NavState and descriptors).
- Create src/router/children/navigation.rs (object-safe source, event broadcast, results).
- Create src/router/children/state.rs (optional whole-state codec and DCH1 frame).
- Modify src/router/{mod.rs,engine.rs,pages/mod.rs} and src/lib.rs.
- Create tests/generic_navigation.rs.
Implementer owns src and tests exclusively; controller owns docs/example/Cargo/README.

- [x] Write a public API RED test before implementation: a non-Clone event adds a descriptor to
  a custom Clone whole-state; mapper returns its metadata and live/destroyed children.
  ```rust
  // The complete test fixture defines a parent context, NavState and SimpleNavigation<Event>.
  let view = children(&ctx, &source, || MyState::empty(), ChildrenOptions::default(),
      |mut state, event| { state.apply(event); state },
      |state, items| (state.revision, items),
      |configuration, child_ctx| MyComponent::new(configuration, child_ctx)).unwrap();
  source.navigate(Event::Add(7));
  assert_eq!(view.get().0, 1);
  assert_eq!(view.get().1.len(), 1);
  ```
  Run `rtk cargo test --test generic_navigation`; expect missing public API.
- [x] Add neutral child types and Pages aliases, object-safe custom source and FIFO SimpleNavigation.
  Test Rc event identity, no subscribers, multi-receiver order, drop/cancel/detach and panic recovery
  before completing source implementation.
- [x] Add minimal engine subscriber/lazy initial seams and custom-model adapter. Source subscription
  precedes initial factory; prepare subscription guards before callbacks. Use pending events for
  synchronous subscription emissions and initialization; hold no callback-preventing borrows.
- [x] Add RED/GREEN runtime tests for each specified lifecycle/state/key/duplicate/ownership/custom
  context/reentrancy case. Use real Essenty and assert instance identity/lifecycle/state, not just
  callback counts. Verify old Pages/Items aliases and families with full baseline suite.
- [x] Add RED/GREEN hook and conditional Back cases. Cache optional back action once per transition,
  clone it before invoking. Test publication -> hook -> completion order and event Rc identity,
  explicit error completions and no Back completion; predictive cancel and child-priority behavior.
- [x] Add optional whole-state codec and DCH1 tests first, then encoder/parser. Frame guards precede
  decode; restored count/duplicates precede factory. Test optional encode/decode, initial factory
  skipped on authoritative empty restore, status-changing restore and retained identities. Exercise
  malformed magic/truncation/count/UTF8/key/counter/DCS1/trailing input and callback panics/rollback.
- [x] Self-review all contracts and run `rtk cargo test --all-targets --all-features`,
  `rtk cargo test --doc --all-features`, `rtk cargo fmt --all -- --check` and
  `rtk cargo clippy --all-targets --all-features -- -D warnings`. Report RED evidence, exact totals,
  files/remaining concerns to controller. Leave changes uncommitted for review.

## Task 2: User-facing example and docs (controller)
- [x] Create examples/generic_navigation.rs: custom empty-capable navigation/state metadata, typed
  non-Clone events, lifecycle policies, Back, optional whole-state byte codec and restoration.
  Run `rtk cargo run --example generic_navigation`; expect state assertions and readable output.
- [x] Write ADR0010 explaining public adapter, neutral types, event sharing, DCH1 and adaptations.
  Update README/lib docs/Cargo description/roadmap/current spec; record ChildItems commit b1a7d56.
- [x] Self-review API consistency and documented boundaries.

## Task 3: Sequential independent reviews and root verification
- [x] Fresh spec reviewer compares actual src/tests/docs with design and pinned Kotlin. Fix gaps
  through original source owner; re-review until PASS.
- [x] Fresh quality reviewer only after spec PASS. Inspect rollback, synchronous custom source
  initialization, callbacks/RefCell, ownership/persistence/cached Back and regression risk. Use
  independent path-dependent /tmp probes for suspected bugs. Fix then repeat reviews as needed.
- [x] Root independently runs full tests/doctests/fmt/Clippy, warning-free rustdoc, all six examples,
  all installed targets and `rtk git diff --check`; record exact successful checks.
- [x] Index current code graph when available; report verified delivery and branch, leaving new
  stage reviewable and uncommitted. No merge, push or unrelated platform integrations.

## Implementation and controller verification (2026-10-09)

Initial implementation completed. Specification review passed; quality review
found three reentrant cleanup defects (recorded below); final corrections and
verification are recorded in the later sections.
New stage remains uncommitted on
codex/generic-navigation, based on ChildItems commit b1a7d56. No merge or push.

Controller independently ran:
- `rtk cargo test --all-targets --all-features`: 350 passed (14 suites).
  Breakdown: 121 unit, 63 Items, 40 Pages, 35 Panels, 35 Slot, 27 Stack,
  4 context integration and 25 Generic Navigation tests.
- `rtk cargo test --doc --all-features`: 1 passed; total 351.
- Format check, strict all-target/all-feature Clippy and warning-free rustdoc: passed.
- All six runnable examples: passed. The new custom gallery verified arbitrary
  metadata, non-Clone event/config/component/result, empty membership, destroyed
  child state (edits17), fresh resurrection with stable key, parent lifecycle bounds,
  restored inactive membership skipping initial factory and conditional Back to empty.
- All 24 installed targets passed `rtk cargo check --lib --all-features --target <target>`:
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
Target checks confirm compilation, not platform runtime behavior.
- `rtk git diff --check`: passed. No dependency changes.

## TDD and parity corrections

Observed RED before implementation: missing router::children API, stale equal
configuration payload on resurrection, encodeNone wrongly querying a panicking
child provider, nested events emitted without receivers reaching late subscribers,
unobserved explicit errors silently discarded, rejected generic Back silently consumed.
Each is GREEN with a meaningful regression.

Upstream inspection refined the original design:
- Resurrection uses current state.configuration; reuse canonicalization opt-in
  shares the Items hook while keeping a live instance's original handle.
- Upstream saveState(None) precedes saveChildState: encode once before lazy children,
  keep payload local and freeze whole-state/items/counter before either callback.
- Explicit error hook omission panics; arbitrary rejected Back propagates through
  a private model hook without fabricating E or changing old families' error policy.
- Public Subscription::from_cancel supports custom sources; cancellation is one-shot.

## Quality findings and resumed work (2026-10-10)

Initial independent specification review passed with high confidence, after actual
source inspection and fresh 25-focused/350-all-target/1-doc tests, formatting,
Clippy and rustdoc. Graph refreshed to 1,758 nodes and 9,387 edges.

Quality review and controller probes reproduced three Important defects:

1. A custom navigation state's Drop ran during mutable state replacement and
   could not reenter a parent save: RefCell already mutably borrowed. Controller
   probe: /tmp/decompose-generic-drop-probe, path-dependent public API, exit101.
2. A custom state's Clone ran under a state read borrow during save; sending a
   navigation event from Clone tried to replace the borrowed state and panicked.
3. A custom source's cancellation panic interrupted parent teardown before Back,
   child scopes and provider were removed; parent was Destroyed but child remained
   Created with two handlers and a registered DefaultChildren provider.

Independent reviewer probe: /tmp/decompose-quality-probes; controller reran it
and confirmed both Clone and cancellation failures. Three newly appended Drop
regressions were run RED on resumption (0 passed, 3 failed): coherent save,
parent-destroy reporting/publication, and preserving committed scopes on Drop panic.

The run was interrupted by agent usage limits; the user explicitly requested
continuation. The original implementation owner corrected all findings and froze
source for repeated specification/quality reviews.

### Corrected runtime and fresh controller gates

- Navigation state is Rc<N> internally, so user Clone and Drop run outside cell
  borrows. Items, state, counter and Prepared ownership commit before replaced
  state is dropped; destruction from Drop stops publication.
- Teardown captures its first cleanup panic, completes every remaining cleanup,
  and preserves an existing unwinding panic instead of causing a second unwind.
- Seven new regressions: three Drop cases (save, FIFO/parent destroy, panic and
  committed scope), Clone during external save, cancellation cleanup, combined
  child/cancellation panic with reverse teardown, and initial hook/cancellation
  panic with rollback/key release. The original defects were reproduced before
  corrections; the added unwind checks also passed after the correction.
- Original controller Drop probe now exits0: reentrant drop save panicked=false.
  Original reviewer probe, independently rerun by controller, exits0: Clone
  reentrant save panics=false; cancellation panic still propagates, but parent/
  child are Destroyed, handler_count=0, provider=false.
- Controller independently repeated full all-target/all-feature tests: 357 passed
  (121 unit + 63 Items + 40 Pages + 35 Panels + 35 Slot + 27 Stack + 4 context +
  32 Generic Navigation). Doc test: 1 passed; total358.
- Formatting, strict Clippy, warning-free rustdoc and git diff check passed.
- Repeated all six examples and all 24 target checks listed above after the fixes;
  each exited0. No platform runtime claim follows from cross-target compilation.

### Repeat specification finding: constructor rollback

Repeat specification review found one additional P1 defect, independently
reproduced by the controller with /tmp/decompose-spec-rollback-probe. Constructing
restored navigation during a parent Back callback fails registration. A custom
source cancellation panic during that ordinary error cleanup escaped
Construction::drop before it removed new retention and returned consumed bytes.
The parent stayed alive, but retention_count was 1 and restored_bytes_preserved
was false (expected 0 and true). The initial-hook unwind regression did not cover
this ordinary Result::Err path.

The owner added two regressions for fresh/existing retention; both were RED,
then GREEN after guarding every rollback step and propagating its first panic
after restoring input. Controller reran the original probe: retention_count=0,
registered=false, restored_bytes_preserved=true; cancellation still propagates.
Fresh controller gates passed 359 all-target tests (34 Generic), 1 doctest, fmt,
strict Clippy/rustdoc, all six examples and the 24-target compile matrix. Graph
refreshed to 1,786 nodes/9,633 edges, with no persisted repository artifact.

### Repeat specification finding: early subscription cancellation

An adjacent pre-engine initialization path remained unguarded: the returned
Subscription stayed local while the initial factory, decoder and initial-state
Clone could run. If initialization and cancellation both panicked, Rust aborted
on a second unwind. Reviewer and controller ran
/tmp/decompose-spec-rollback-probe/src/bin/initial_cancel.rs independently;
exit134, `panic in a destructor during cleanup`. The fix must own a guarded token
immediately after subscribe returns and transfer it safely to the engine.
The owner added a private setup token guard and one subprocess regression covering
initial factory, decoder and pre-engine Clone panics combined with cancellation.
All three subprocess scenarios aborted RED; all preserve their original panic
GREEN, with no leaked provider, retention or Back handler and retryable restored
input. Controller reran the initial_cancel probe: exit0, caught_original=true.
The public Subscription Drop contract is unchanged.

## Final source verification (2026-10-10)

Independent specification re-review: PASS, high confidence. Both P1 follow-up
findings resolved; reviewer reran the original probes and full gates. Independent
whole-stage quality review: **APPROVED**, high confidence, no remaining Critical,
Important or Minor findings. Reviewer independently repeated 360 tests + 1
doctest, fmt, strict Clippy/rustdoc, the generic example and all original probes.

Controller independently reran after the last source correction:
- All-target/all-feature tests: **360 passed**, 14 suites. Breakdown: 121 unit,
  63 Items, 40 Pages, 35 Panels, 35 Slot, 27 Stack, 4 context, **35 Generic**.
- All-feature doctest: **1 passed**, total **361**.
- Formatting, strict Clippy and warning-free rustdoc: passed.
- All six examples and all 24 target checks listed above: passed, exit0 each.
- Original Drop, Clone, cancellation, constructor rollback and initial/cancellation
  public-API probes: GREEN; expected panic propagation remains observable.
- Code graph: **1,791 nodes / 9,655 edges**, persistence disabled, no repo artifact.

- Final `rtk git diff --check`: passed.

### Verified boundary and delivery

The quality reviewer independently confirmed the existing lifecycle panic boundary:
navigation completes Back/provider/source/child-lifecycle cleanup, but propagating
a panic can stop independent Essenty listeners, including default-root keeper
cleanup. Retained keeper lifetime belongs to its hosting owner; the engine cannot
destroy a supplied parent keeper intended to survive lifecycle recreation. README
and ADR0010 explicitly describe this boundary. It is not a remaining production
finding or an Essenty change in this stage.

Generic Navigation delivery is complete and remains reviewable, uncommitted on
**codex/generic-navigation**, based on **b1a7d56** (committed ChildItems). Source,
tests, example and documentation are complete. No merge, push, new dependencies,
platform integrations or changes to Essenty were performed.

### Commit follow-up (2026-10-10)

The user requested committing this verified delivery. Fresh controller checks
again passed 360 all-target/all-feature tests, 1 doctest, formatting, strict
Clippy and diff whitespace checks before the commit. Branch remains
codex/generic-navigation; merge and push were not requested.
