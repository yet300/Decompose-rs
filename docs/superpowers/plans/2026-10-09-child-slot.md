# ChildSlot implementation plan

> For agentic workers: use superpowers:subagent-driven-development with one
> implementation owner, then independent spec and quality reviews.

Goal: implement the authorized ChildSlot model and preserve existing Stack behavior.
Architecture: one crate, a private shared engine/source/scope implementation, model
policies for Stack and Slot. Public APIs use Rc and Option with no Clone bounds.
Tech stack: Rust 1.96/2024, existing four Essenty 0.1.0 primitives and thiserror.

## Task 1: behavioral tests and shared machinery

- [x] Add tests/child_slot.rs with empty initialization and activation/dismiss
  tests using the intended API below; run rtk cargo test --test child_slot and
  verify missing API before implementing, then behavioral failures with stubs.
- [x] Extract common Child/NavigationError and shared event source while preserving
  existing stack imports; extract private engine policies instead of duplicating
  transaction, scope and queued Back machinery.
- [x] Implement Slot API/transactions incrementally against failing behavior tests.
- [x] Preserve all existing tests via rtk cargo test --all-features after refactors.

```rust
let root = DefaultComponentContext::new(resumed());
let navigation = SlotNavigation::new();
let slot = child_slot(&root, &navigation, None::<u8>, SlotOptions::default(),
    |_, context| context).unwrap();
assert!(slot.get().child.is_none());
navigation.activate(7);
let child = slot.get().child.as_ref().unwrap().clone();
navigation.activate(7);
assert!(Rc::ptr_eq(&child.instance, &slot.get().child.as_ref().unwrap().instance));
navigation.dismiss();
assert!(slot.get().child.is_none());
assert_eq!(child.instance.lifecycle().state(), LifecycleState::Destroyed);
```

## Task 2: completion, Back, persistence and failure paths

- [x] Add failing tests for activate_with/dismiss_with completion, equal/empty no-ops
  and per-connected-slot broadcast results; implement explicit Result callbacks.
- [x] Test and implement automatic dismiss, child Back precedence, predictive
  cancellation, deferred callback navigation and panic queue cleanup.
- [x] Add SlotCodec, DSL1 empty/populated envelope and strict framing tests.
- [x] Test persisted empty membership overriding Some(initial), retained recreation,
  process-death nested state, malformed bytes/codec errors and retryable rollback.
- [x] Test Stack/Slot shared-parent and nested custom non-Clone contexts, factory/
  lifecycle/observer/completion reentry, parent-destroy abort and key reservations.
- [x] Test view Drop and scope/source teardown; review Rc edges and borrows across
  callbacks. Keep Stack API and persisted DST1 compatibility.

```rust
let reported = Rc::new(RefCell::new(Vec::new()));
let output = reported.clone();
navigation.dismiss_with(move |result| output.borrow_mut().push(result.unwrap()));
assert_eq!(*reported.borrow(), [true]);
let saved = root.state_keeper().borrow().save().unwrap();
let fresh = DefaultComponentContext::with_state_keeper(resumed(),
    Rc::new(RefCell::new(StateKeeper::with_restored(saved))));
let restored = child_slot(&fresh, &SlotNavigation::new(), Some(9),
    SlotOptions { codec: Some(codec()), ..SlotOptions::default() },
    |_, context| context).unwrap();
assert!(restored.get().child.is_none());
```

## Task 3: reviews, docs, example and verification

- [x] Independent spec review against docs/superpowers/specs/2026-10-09-child-slot-design.md.
- [x] Fix findings; independent code quality/ownership review after spec approval.
- [x] Update README, crate docs, roadmap/ADR and add examples/child_slot.rs showing
  combined Stack/Slot, Back and empty-state restore without runtime/UI dependencies.
- [x] Run rtk cargo fmt --all -- --check.
- [x] Run rtk cargo clippy --all-targets --all-features -- -D warnings.
- [x] Run rtk cargo test --all-targets --all-features and rtk cargo test --doc --all-features.
- [x] Run rtk proxy env RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features.
- [x] Run rtk cargo run --example child_slot and preserve child_stack example.
- [x] Run rtk cargo check --lib --target for installed targets; record evidence.
- [x] Review final diff and leave completed local changes on codex/child-slot.

## Final evidence — 2026-10-09

Rust 1.96.0, codex/child-slot, baseline main@e96eaf1. Commands executed through
RTK; all final commands exited 0. Independent SPEC PASS and QUALITY APPROVAL.

| Gate | Evidence |
|---|---|
| cargo fmt --all -- --check | Passed |
| cargo clippy --all-targets --all-features -- -D warnings | Passed |
| cargo test --all-targets --all-features | 180 passed: 115 unit +34 Slot +27 Stack +4 context integration |
| cargo test --doc --all-features | 1 passed; total 181 |
| RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features | Passed |
| cargo run --example child_slot | Combined Back dismiss/pop, same-instance equality, saved empty restore passed |
| cargo run --example child_stack | Existing stack save/restore example passed |
| git diff --check | Passed |
| cargo check --lib --target <installed target> | All 24 passed; compile-only outside host |

Baseline:144 tests. Added 34 Slot tests, 2 Stack panic-recovery regressions and
1 immutable pre-Slot DST1 fixture. The shared engine preserves existing APIs and
serialized Stack bytes. Slot's empty DSL1 state never invokes configuration codecs.

During quality review an independent standalone reproduction exposed Eq reuse of
ended old scopes after outgoing onDestroy or incoming onResume panic. Four tests
(two per model) failed with Destroyed vs Resumed. Matching now checks local
lifecycle and retained keeper liveness; retries construct a fresh key/component,
finish stale retention cleanup, and preserve ordinary live equal identity.
All four regressions passed, and independent probes confirmed fresh Resumed
instances plus stale keeper cleanup. A custom Eq probe confirmed the liveness
borrow ends before user equality runs. A transient-save regression also verifies
that codecs disabled means child state providers are not invoked.

Compile target matrix:

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

Delivery complete within the approved scope. One crate and unchanged Essenty
primitive dependencies; no UI/runtime integration or public generic engine.
Committed on the user's request as e293ab9 (`feat: implement ChildSlot with shared
navigation engine`). The next-stage branch codex/child-pages starts at that commit.
No merge or publication was requested in that continuation.
