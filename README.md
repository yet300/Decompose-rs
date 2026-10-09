# Decompose-rs

Independent Rust port of [Ark Ivanov’s Decompose](https://github.com/arkivanov/Decompose),
implemented on top of [Essenty-rs](https://github.com/yet300/Essenty).

**Current scope: ComponentContext and ChildStack.** The port includes observable
values, navigation operations, child lifecycle management, Back routing and
configuration/state restoration. Other navigation families and platform/UI
integrations are outside this delivery. Behavior is checked against pinned
Decompose revision `00840bcfed6d55710c8fbb1a72503f2041eda490`, not an unspecified
latest release. See `NOTICE` for attribution.

## Start here

```rust
use decompose_rs::{DefaultComponentContext, LifecycleRegistry, LifecycleState};
use decompose_rs::router::stack::{child_stack, StackNavigation, StackOptions};

#[derive(PartialEq, Eq)]
enum Route { Home, Details(u32) } // No Clone requirement.

let lifecycle = LifecycleRegistry::new();
lifecycle.move_to(LifecycleState::Resumed).unwrap();
let root = DefaultComponentContext::new(lifecycle.clone());
let navigation = StackNavigation::new();
let stack = child_stack(
    &root, &navigation, vec![Route::Home],
    StackOptions { handle_back_button: true, ..StackOptions::default() },
    |_, context| context,
).unwrap();
let subscription = stack.subscribe(|snapshot| {
    println!("depth: {}", snapshot.items().count());
}); // Immediately replays the current value.

navigation.push_new(Route::Details(7));
assert_eq!(stack.get().items().count(), 2);
navigation.pop();
assert_eq!(stack.get().items().count(), 1);
drop(subscription); // Cancels observation.
lifecycle.destroy().unwrap();
```

Run `cargo run --example child_stack` for an enum-route example with automatic
Back, explicit configuration codecs, saving and process-death restoration.
The example is in [examples/child_stack.rs](examples/child_stack.rs).

## Rust API and behavior

- The core is synchronous and single-threaded, using `Rc` and short `RefCell`
  borrows. It requires no Tokio, UI framework, mandatory serde, Send or Sync.
- `ComponentContext`, custom service-bearing contexts and components need no
  Clone implementation. Configurations require `Eq + 'static`; snapshots expose
  `Rc<C>` and `Rc<T>`. The context factory propagates custom services into children.
- A stack has one active child and an ordered `back_stack`. Surviving equal
  configurations retain their instance and stable key. Duplicates are rejected
  unless enabled per stack; enabled duplicates match by occurrence.
- The active child targets Resumed and back-stack children target Created, bounded
  by the parent lifecycle. The outgoing child leaves its active state before
  the incoming child resumes. Removed scopes end even if an old snapshot holds
  their component instances.
- The parent owns navigation. Dropping a `Value` view does not destroy it.
  Subscription Drop cancels observation; `detach` transfers cancellation
  responsibility to the source’s lifetime.
- Nested navigation from factories, lifecycle callbacks, observers, completion
  callbacks and Back handlers is queued. Back-triggered scope changes wait until
  the dispatcher releases its borrow. A child’s enabled Back handler has
  precedence over automatic pop; inactive children do not handle Back.

`StackNavigation` provides `navigate`, `navigate_with`, `push`, `push_new`,
`push_to_front`, `pop`, `pop_while`, `pop_to`, `pop_to_first`, `replace_current`,
`replace_all` and `bring_to_front_by`. Root pop and pushing an already active
configuration with `push_new` are no-ops. `bring_to_front_by` takes a kind
selector so Rust enum variants can replace Kotlin runtime-class matching.

`child_stack` returns a typed construction error. Convenience navigation methods
panic on rejected transformations (for example, an empty stack or disallowed
duplicates). Use `navigate_with` for a `Result<NavigationResult<C>, NavigationError>`
completion callback. A shared source reports once per connected stack; a queued
call has no immediate final result. User panics propagate, with cleanup of
incomplete construction and abandoned queued work. Successful user callback
side effects are outside that rollback boundary.

## State and retained instances

`StackOptions::codec` defaults to None for transient navigation. Supply a
`StackCodec<C>` with configuration-list encode/decode functions to persist stack
order, stable child keys and nested child state through the hosting state keeper.
The versioned envelope checks counts, keys, framing and trailing bytes before
factories run. Corruption returns an error; it does not silently reset the stack.
Codec save failures propagate through `StateKeeper::save`.

A caller-supplied InstanceKeeper can survive parent lifecycle recreation.
Recreated components receive fresh contexts, while their retained service
instances preserve identity. A default root’s owned keeper ends with its
lifecycle. Fixed-child retention follows the parent retained scope’s lifetime.
Explicit child lifecycle destruction retains the existing public panic contract.

The public `StateKeeper` is now a Decompose adapter around Essenty’s primitive;
`SharedKeeper` is `Rc<RefCell<StateKeeper>>`. This API correction lets failed
construction return consumed input for retries and saves, without replacing the
parent keeper or changing Essenty. Convert existing owned primitives with
`StateKeeper::from(essenty_keeper)`; the raw type is reexported as
`EssentyStateKeeper`. Existing shared raw handles need conversion at context
assembly. The restored child frame is also the first-save fallback, so a failed
first child save does not discard previously restored data.

## Structure

One `decompose-rs` crate keeps the shared ownership and lifecycle machinery in
one place. There is no independent consumer or release boundary justifying
separate context, value and stack crates in this scope.

| Modules | Responsibility |
|---|---|
| `context`, `child`, `retention` | Context factories, child scopes and retention lifetime |
| `merged`, `shared_back` | Parent-bounded lifecycle and reentrant Back dispatch |
| `state`, `framing` | Essenty state adapter and child-state encoding |
| public `value` | Observable immutable snapshots and subscription guards |
| public `router::stack` | Stack API, operations and user codecs |
| private stack `engine`, `navigation`, `state` | Scope ownership, queued events and persistence |

Dependencies remain the four published `essenty-* 0.1.0` primitives and
`thiserror`. No Essenty source changes, path/git overrides or additional runtime
are required. [ADR-0005](docs/adr/0005-stack-and-retention-corrections.md) records
implemented corrections and supersedes conflicting assumptions in older
Phase 1 research. The [approved design](docs/superpowers/specs/2026-10-09-component-context-child-stack-design.md)
and [implementation plan](docs/superpowers/plans/2026-10-09-component-context-child-stack.md)
record the delivery boundaries.

## Verification (Rust 1.96.0)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo run --example child_stack
```

Verified on 2026-10-09: 143 unit/integration tests and 1 doctest passed; format,
Clippy, rustdoc and the example passed. Library compilation passed for all
24 installed targets, including WASM, Apple, Android, Linux and Windows.

Regression tests cover restored-state loss, retry after construction failure,
retained recreation, non-Clone custom nesting, identity/lifecycle ordering,
Back priority, repeated callback entry, persistence errors and teardown.
Compile-target results are recorded in the implementation plan; compilation
alone does not establish platform runtime behavior.

## License

Apache-2.0 — see `LICENSE-APACHE` and `NOTICE`.
