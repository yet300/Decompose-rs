# ComponentContext and ChildStack implementation plan

> For agentic workers: use superpowers:subagent-driven-development where independent modules permit it; integrate and verify in this checkout.

Goal: deliver the approved ComponentContext fixes and a runnable, persistent ChildStack without UI/runtime dependencies.

Architecture: keep one crate. Configuration and component snapshots use Rc; stack lifecycle management and event processing are private. Existing Essenty primitives remain dependencies.

Tech stack: Rust 1.96, edition 2024, four Essenty 0.1 crates, thiserror.

## Task 1: regression tests for context semantics

- [x] Add tests in tests/component_context.rs for restored-frame first-save failure, retry after factory panic, and fixed-child identity after lifecycle recreation.
- [x] Run `rtk cargo test --test component_context`; expect these new cases to fail against the baseline.
- [x] Fix last-good initialization, restoration rollback, and retained-scope ownership; keep the manual-destroy contract.
- [x] Run `rtk cargo test --all-features`; replace any obsolete assertions encoding the contradicted lifecycle/retention behavior.

Reference regression shape:

```rust
let saved = root.state_keeper().borrow().save().unwrap();
let old_scope = child.instance_keeper();
old_lifecycle.destroy().unwrap();
let root2 = DefaultComponentContext::with_all(new_lifecycle, restored_keeper, retained_keeper, SharedBack::new());
let child2 = root2.child_context("child").unwrap();
assert!(Rc::ptr_eq(&old_scope, &child2.instance_keeper()));
assert!(!old_scope.borrow().is_destroyed());
```

## Task 2: observable values

- [x] Add `src/value.rs`: read-only Value and MutableValue, Rc snapshots, Subscription cancel/detach.
- [x] Test synchronous replay, nested update, cancellation during emission, non-Clone values, and unwind recovery before implementation.
- [x] Run the value tests, watching the new tests fail then pass.

## Task 3: public stack types and navigation

- [x] Add `src/router/stack/navigation.rs`: Rc configuration event snapshots, FIFO dispatch, completion Results, all approved operation transforms.
- [x] Test push_new/root-pop/no-op boundaries and reentrant broadcast before implementation.
- [x] Add `src/router/stack/mod.rs`: immutable Child and ChildStack snapshots, options and construction entry points.

## Task 4: private stack engine

- [x] Add `src/router/stack/engine.rs`: validate, match by equality/occurrence, prepare scopes, stop old active, destroy removed, activate new top, publish and complete.
- [x] Add behavioral tests in `tests/child_stack.rs` before implementing each behavior.
- [x] Use ContextParts directly for navigation scopes; never call child_context_with_lifecycle to destroy stack entries.
- [x] Add private SharedBack after-dispatch hooks and test navigation from Back callbacks before using them.
- [x] Verify old snapshots retain instances while removed child lifecycles are Destroyed, and surviving configurations retain keys/identity.

## Task 5: persistence and retention

- [x] Add `src/router/stack/state.rs`: versioned, length-delimited frame, configuration codec, child key/state positional correspondence.
- [x] Add malformed-frame, codec failure, process recreation and retained recreation tests first.
- [x] Register one stack provider under the caller key; transient stacks do not restore configuration state.
- [x] Verify nested child state and configuration order round-trip and subscriptions end with the host lifecycle.

## Task 6: documentation, example, final verification

- [x] Update README, crate docs, roadmap and approved spec with actual behavior and public API adaptations.
- [x] Add `examples/child_stack.rs`, using enum routes and no GUI/runtime.
- [x] Run `rtk cargo fmt --all -- --check`.
- [x] Run `rtk cargo clippy --all-targets --all-features -- -D warnings`.
- [x] Run `rtk cargo test --all-features`.
- [x] Run `rtk proxy env RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features`.
- [x] Run `rtk cargo run --example child_stack` and host/WASM cargo checks.
- [x] Review final diff and document any remaining scope limitation truthfully.

## Final verification evidence — 2026-10-09

Rust 1.96.0. All commands completed with exit code 0 in the implementation checkout.

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --all-targets --all-features -- -D warnings` | Passed |
| `cargo test --all-targets --all-features` | 114 unit + 25 ChildStack integration + 4 ComponentContext integration tests passed |
| `cargo test --doc --all-features` | 1 doctest passed |
| `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features` | Passed |
| `cargo run --example child_stack` | Back returned Details to Home; restored Settings at depth 2 |
| `git diff --check` | Passed |
| `cargo check --lib --target <target>` | All 24 installed targets passed |

Compile-only target matrix (runtime behavior was exercised on the host):

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

Independent spec/quality review of Value and of the integrated stack completed
without remaining blocking findings. Reproduced retention/state and stack
regressions were fixed before these final gates. The final implementation
retains existing module paths; ADR-0005 records the public state-adapter change
and the successful-callback side-effect boundary of rollback.

The approved delivery is complete. Other navigation families, a public generic
children engine, web history and UI/platform integrations remain outside scope.
Changes are local; no package or remote branch was published.
