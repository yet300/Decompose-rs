# Standard Router Extension Points Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox syntax.

**Goal:** Connect custom event sources to every standard router and expose reusable navigator traits.
**Architecture:** Shared public event/source transport, thin family adapters, existing private engine.
**Tech Stack:** Rust 1.96/edition2024, Rc/RefCell, existing Essenty 0.1.0.

Baseline 9a70ae0: 389 unit/integration tests + 1 doctest, merged and reverified on
main. Current branch codex/router-extension-points. Normative contracts:
docs/superpowers/specs/2026-10-10-router-extension-points-design.md.

## Task 1: Public events, traits and factory integration (one source owner)

Files: src/router/navigation.rs, src/router/mod.rs, src/router/engine.rs,
src/router/{stack,slot,pages,panels,items}/{mod,navigation,engine}.rs where present;
new focused private family navigation modules if needed; tests/router_extension_points.rs.
Controller owns README/lib docs, example and architecture documents.

- [x] Write and run RED public event/source tests before adding APIs.
  ```rust
  let source = SimpleNavigation::<StackEvent<u8>>::new();
  let event = Rc::new(StackEvent::new(|mut state| {
      state.push(Rc::new(2)); state
  }));
  let seen = Rc::new(RefCell::new(None));
  let out = seen.clone();
  let token = NavigationSource::subscribe(&source, Rc::new(move |event| {
      *out.borrow_mut() = Some(event);
  }));
  source.navigate_rc(event.clone());
  assert!(Rc::ptr_eq(&event, seen.borrow().as_ref().unwrap()));
  drop(token);
  ```
  Run rtk cargo test --test router_extension_points; expect missing event aliases.
- [x] Add NavigationEvent<S,R> with shared callbacks, new/with_completion,
  transform/complete and bound-free Clone; export five aliases and concrete
  NavigationSource implementations using SimpleNavigation. Preserve no-replay,
  shared event identity, receiver-local result and broadcast panic/FIFO contracts.
- [x] RED custom navigator helper tests, then move existing helper bodies into
  four generic navigator traits with navigate_with as the required method.
  Preserve all inherent methods via delegation and existing ItemsNavigator API.
  ```rust
  fn push_twice<N: StackNavigator<u8>>(navigator: &N) {
      navigator.push(2); navigator.push(3);
  }
  fn choose_next<N: PagesNavigator<u8>>(navigator: &N) {
      navigator.select_next(false);
  }
  ```
  Custom wrappers implement navigate_with by emitting typed public events.
- [x] RED factory connection tests for custom and dyn sources in all families.
  ```rust
  let events = SimpleNavigation::<StackEvent<u8>>::new();
  let source: &dyn NavigationSource<StackEvent<u8>> = &events;
  let stack = child_stack(&context, source, vec![1], StackOptions::default(),
      |configuration, _| *configuration).unwrap();
  events.navigate(StackEvent::new(|mut state| {
      state.push(Rc::new(2)); state
  }));
  assert_eq!(*stack.get().active.configuration, 2);
  ```
  Generalize argument-position impl Trait without changing named generic counts;
  translate family events/results to private engine Event through existing
  build_with_subscriber. Slot translates Option to Vec; Panels retains typed role
  factories; Items retains NavigationControl for LazyChildItems.
- [x] Add subscription-time emission and reentrant completion tests, multi-receiver
  own-state results, invalid stack/pages errors preserving state, all-family Back
  and destruction cancellation. Add constructor Err/factorypanic rollback tests
  with saved-input retry, release of custom-source registration/captures, and
  original panic preservation under custom cancellation panic.
- [x] Focused tests GREEN, then full test/doc/fmt/strict Clippy; self-review every
  callback/Drop borrow boundary and API compatibility; freeze source for reviews.
  Do not commit or switch branches; controller coordinates final stage delivery.

## Task 2: Controller documentation and executable example

- [x] Add examples/router_extension_points.rs demonstrating an intercepted typed
  Stack source implementing NavigationSource and StackNavigator, generic helpers,
  dyn factory connection, receiver completion and lifecycle destruction.
- [x] Update README, ADR-0012, roadmap, lib docs and observable plan delivery
  record with actual commit/main/branch evidence. Describe source object safety
  and static-dispatch navigator traits, no-op event defaults vs error-panic helpers.
- [x] Inspect the full implementation against the design and preserve scope.

## Task 3: Sequential reviews and verification

- [x] Fresh spec reviewer; original owner fixes concrete findings and re-review.
- [x] Fresh whole-stage quality reviewer after spec PASS; repeat after source fixes.
- [x] Controller independently runs all-target/features tests, doctests, fmt,
  strict Clippy/rustdoc, eight examples, all installed lib target checks, diff check.
- [x] Record actual results and stage readiness on codex/router-extension-points.
  Local commit and main merge were subsequently authorized by the user.
  No remote push or branch deletion was requested.

## Verification before independent reviews (2026-10-10)

Controller independently confirmed 410 unit/integration tests plus 1 doctest
(389 baseline +21 extension tests), formatting, strict Clippy, strict rustdoc and
diff whitespace. All eight examples executed successfully. Library compilation
passed every installed target below; cross-target runtime execution is not claimed.

Commands: `rtk cargo test --all-targets --all-features`,
`rtk cargo test --doc --all-features`, `rtk cargo fmt -- --check`,
`rtk cargo clippy --all-targets --all-features -- -D warnings`,
`rtk proxy env RUSTDOCFLAGS='-D warnings' cargo doc --all-features --no-deps`,
`rtk cargo run --example <each of eight examples>`,
`rtk cargo check --lib --all-features --target <each installed target>`,
`rtk git diff --check`. Source owner independently ran fresh gates and froze
all source/tests for sequential spec and quality reviews.

Initial RED: 24 missing API compile errors before implementation. The focused
suite then passed and expanded to 21 regressions across source transport, generic
helper inheritance, dyn factory connection, state/result adaptation, construction
event buffering, Back, teardown and saved-input/panic rollback. Constructor
combined-panic cases run in isolated subprocesses.

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

## Final independent reviews (2026-10-10)

Spec review: PASS, high confidence, no findings. The fresh reviewer checked
public event contracts, all five source aliases, factory generic compatibility,
helper delegation, local Back, construction buffering and ownership rollback.
It independently ran the 21 focused tests, all 410 tests, the doctest and example.

Subsequent whole-stage quality review: PASS, ready, high confidence, no Critical,
Important or Minor findings. It checked actual APIs, transport, adapters,
construction/pump/teardown, tests, example and documentation, and independently
confirmed focused/full tests, doctest, formatting and diff checks. No source
repair was required after either review.

The stage passed verification on codex/router-extension-points. The user then
explicitly requested committing it and merging it into main. Integration uses a
local fast-forward from the observable baseline 9a70ae0, followed by tests on
main. Commit identity and branch membership are recorded in Git. No remote push
or branch deletion was requested.
