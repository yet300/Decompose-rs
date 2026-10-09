# Decompose-rs

Independent Rust port of [Ark Ivanov’s Decompose](https://github.com/arkivanov/Decompose),
implemented on top of [Essenty-rs](https://github.com/yet300/Essenty).

**Current scope: ComponentContext, ChildStack, ChildSlot, ChildPages, ChildPanels and ChildItems.** The port includes observable
values, navigation operations, child lifecycle management, Back routing and
configuration/state restoration. ChildSlot represents zero-or-one children and
supports activation, replacement and dismissal. ChildPages keeps page identities and
saved state while creating or destroying instances according to a lifecycle policy.
ChildPanels manages typed Main/Details/Extra components in Single, Dual or Triple mode.
ChildItems manages a unique list with per-configuration lifecycle targets and
explicit lazy component creation.
Other navigation families and platform/UI
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
Run `cargo run --example child_slot` for combined Stack/Slot navigation, Back
dismissal and restoration of a saved empty slot.
Run `cargo run --example child_pages` for lazy pages, destruction/recreation under
the same key, child-state recovery and restored selection.
Run `cargo run --example child_panels` for distinct role types, mode transitions,
Single-only automatic Back and restored mode/child state.
Run `cargo run --example child_items` for lazy activation, viewport lifecycle
targets, deactivation/recreation and restoration of inactive child state.

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
side effects are outside that rollback boundary. If a lifecycle callback panic
ends a child before membership commits, subsequent navigation reconstructs that
child instead of reusing its ended scope.

## ChildSlot

`router::slot` exposes `ChildSlot<C, T> { child: Option<Child<C, T>> }`,
`SlotNavigation<C>`, `SlotOptions<C>`, `SlotCodec<C>` and `child_slot`.
Use None as the initial configuration for an empty slot. The factory receives
`&C` and the same custom context type used by the parent.

```rust
use decompose_rs::router::slot::{child_slot, SlotNavigation, SlotOptions};
use decompose_rs::{DefaultComponentContext, LifecycleRegistry, LifecycleState};

let lifecycle = LifecycleRegistry::new();
lifecycle.move_to(LifecycleState::Resumed).unwrap();
let root = DefaultComponentContext::new(lifecycle.clone());
let navigation = SlotNavigation::new();
let slot = child_slot(&root, &navigation, None::<String>,
    SlotOptions { handle_back_button: true, ..SlotOptions::default() },
    |_, context| context,
).unwrap();

navigation.activate("Confirm deletion".into());
assert!(slot.get().child.is_some());
navigation.dismiss();
assert!(slot.get().child.is_none());
lifecycle.destroy().unwrap();
```

`activate` with an equal configuration preserves the child's instance, stable
key and lifecycle. Replacement ends the outgoing child before resuming the new
one; `dismiss` destroys the child's lifecycle and retained scope. Empty dismissal
is a no-op. A child targets Resumed, bounded by the parent. No-op navigation still
publishes an observable snapshot and runs completion, as in upstream.

`navigate`/`navigate_with` transform `Option<Rc<C>>`. `activate_with` reports
old/new configuration snapshots through a typed completion;
`dismiss_with` reports `Result<bool, NavigationError>`, true if a child existed.
Convenience calls panic on rejected transitions, while explicit completions
receive errors. Shared sources report independently for each connected slot.

`SlotCodec::new` accepts callbacks for one configuration. With a codec,
DSL1 persists explicit empty/populated membership, child key, allocation counter
and nested state. Empty saves invoke neither configuration callback, and restore
as empty even when a populated initial configuration is supplied. This prevents
a dismissed dialog reappearing from an earlier save. Without a codec, the slot
is transient. Malformed state is rejected before child factories.

Stack and Slot may coexist under different keys, and may be nested through
custom contexts. `handle_back_button` defaults to false; when enabled, an
unhandled Back dismisses a populated slot. Child handlers have precedence over
automatic navigation. Sibling handlers follow SharedBack priority/LIFO ordering;
an empty slot does not consume Back. Predictive cancellation preserves membership.

## ChildPages

`router::pages` exposes `Pages<C>`, `ChildPages<C, T>`, `PageChild<C, T>`,
`PageStatus`, `PagesNavigation`, `PagesOptions`, `PagesCodec` and `child_pages`.
`Pages::new(configurations, selected_index)` validates the initial position.
Snapshots use `Option<usize>`: an empty collection selects None, while a nonempty
collection always selects a valid index. Configurations and components need no Clone.

By default, the selected page targets Resumed, its immediate neighbors target
Created, and all other pages are Destroyed. Only live pages invoke the component
factory. A destroyed page exposes its configuration and stable key through
`PageChild`, with `instance()` returning None. It retains saved child state.
Selecting it later creates a new component and keeper using that same key and
state. An old snapshot may keep the previous component object alive, but its
lifecycle has ended. Equal configurations match by occurrence and preserve keys
across list reorders; duplicates require explicit opt-in.

`PagesOptions::page_status` accepts a policy callback that chooses Created,
Started, Resumed or Destroyed for each position. This supports circular neighbors
or keeping additional pages live. The hosting lifecycle bounds each live page.
The default policy and a circular navigation operation are independent choices.

`PagesNavigation` provides `navigate`/`navigate_with`, `select`/`select_with`,
`select_next(circular)`, `select_prev(circular)`, `select_first`, `select_last`,
`clear` and `set_items`. Boundary movement clamps unless circular behavior is
requested. Empty selection operations are no-ops. `set_items` clamps the previous
numeric index to the new list; it does not follow the selected configuration
when items move. Explicit completion callbacks receive typed errors for invalid
nonempty selection; convenience methods panic on rejection.

With `handle_back_button: true`, unhandled Back selects the previous page when
the selected index exceeds zero. Enabled child handlers take precedence.
Created and Destroyed pages cannot handle Back. Shared source events, repeated
callback entry, parent ownership and deferred Back transitions follow the other
router models.

`PagesCodec::new` uses configuration-list encode/decode callbacks, like Stack.
DPG1 persists order, selection, child keys and allocation counter, and state for
both live and destroyed pages. Empty saved membership overrides initial pages.
Restoration recomputes statuses using the current policy. Without a codec the
router is transient; it does not invoke child providers for a parent save.
Codec/framing failures return errors and constructor rollback keeps input
available for retry. DST1 and DSL1 remain compatible with earlier router saves.

## ChildPanels

`router::panels` exposes `Panels<MC, DC, EC>`,
`ChildPanels<MC, MT, DC, DT, EC, ET>`, `ChildPanelsMode`, `PanelsNavigation`,
`PanelsOptions`, `PanelsCodec` and `child_panels`. Each role has its own
configuration and component type, with no Clone requirement. Main is required;
Details and Extra are independently optional. `Panels::new(main)` defaults to
Single mode with no optional panels. Public fields allow whole-state transforms.

| Mode | Main | Details | Extra |
|---|---|---|---|
| Single | Resumed only with no optional panels; otherwise Created | Resumed without Extra; otherwise Created | Resumed |
| Dual | Resumed | Resumed without Extra; otherwise Created | Resumed |
| Triple | Resumed | Resumed | Resumed |

The table applies to present roles; the parent lifecycle bounds each target.
Every present panel stays live. Mode changes preserve component, retained keeper
and stable key. Equality preserves identity within a role; equal underlying
values in different roles remain separate children. Factories run Main, Details,
Extra order and destruction runs in reverse. Replacing Main leaves optional
panels unchanged. Dismissing Details leaves Extra unchanged.

Navigation provides `navigate`/`navigate_with`, `set_panels`,
`set_details_extra`, `set_extra`, `activate_main`, `activate_details`,
`activate_extra`, `dismiss_details`, `dismiss_extra`, `pop` and `set_mode`.
Helpers have `_with` forms reporting typed old/new state per connected receiver.
`pop` removes Extra first, then Details, and preserves Main. Explicit pop works
in every mode; automatic Back requires Single and `handle_back_button: true`.
Enabled child handlers have precedence; Created children cannot intercept Back.

`PanelsOptions::on_state_changed` receives the new state and optional old state.
The initial notification has old=None and runs after provider/Back registration,
while initialization still holds queued work. Later notifications run after
observable publication and Back enablement, before event completion. No-op
events and automatic Back also notify. Reentrant calls remain queued. Callback
panics propagate; committed state and successful callback effects persist.

`child_panels_without_extra` accepts just Main/Details factories and uses
`std::convert::Infallible` for both Extra configuration and component types.
This expresses an unsupported role without a fictitious extra component.
`PanelsCodec::without_extra` adapts a two-role codec to the same representation.

`PanelsCodec::new` accepts typed encode/decode callbacks for Main and optional
Details/Extra configurations. DPN1 adds mode, role presence, stable keys,
allocation counter and nested child states. Decoded membership must match the
envelope's roles. Structural corruption is rejected before configuration codecs
and factories; failed construction keeps original input available for retry.
Without a codec Panels is transient and parent saves skip its child providers.
Existing DST1, DSL1 and DPG1 frames remain compatible.

## ChildItems

`router::items` exposes `Items<C>`, `ChildItems<C, T>`, `ActiveChild<C, T>`,
`ActiveLifecycleState`, `ItemMap<C, V>`, `ItemsNavigation`, `ItemsNavigator`,
`ItemsOptions`, `ItemsCodec`, `LazyChildItems` and `child_items`.
`Items::new(configurations)` validates unique membership; an empty list is valid.
Each active configuration has a Created, Started or Resumed target. Absent
activity means the component is inactive, with no factory invocation or keeper.
Parent lifecycle bounds the actual state of each active component.

Activity is keyed by configuration equality, so list reorders preserve the
component and its target. Equal configuration replacement also preserves a live
component; after deactivation, a fresh factory receives the current configuration,
including updated fields ignored by Eq. `ItemMap` provides Eq-based map operations over Rc keys,
without imposing Hash, Ord or configuration Clone. Lookups are linear. Inserting
an equal key replaces its value; active keys absent from membership are pruned,
and surviving keys use the canonical membership handle. Duplicate member
configurations are always rejected; there is no opt-in for duplicates.

```rust
use decompose_rs::router::items::{child_items, Items, ItemsNavigation, ItemsOptions};
use decompose_rs::{ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState};

let lifecycle = LifecycleRegistry::new();
lifecycle.move_to(LifecycleState::Resumed).unwrap();
let root = DefaultComponentContext::new(lifecycle.clone());
let navigation = ItemsNavigation::new();
let items = child_items(&root, &navigation, Items::new(vec![1u8, 2, 3]).unwrap(),
    ItemsOptions::default(), |_, context| context,
).unwrap();
assert!(items.get().active_items.is_empty());
let component = items.get_or_create(&2).unwrap();
assert_eq!(component.lifecycle().state(), LifecycleState::Created);
items.set_active_items(|mut active| { active.remove(&2); active });
assert_eq!(component.lifecycle().state(), LifecycleState::Destroyed);
lifecycle.destroy().unwrap();
```

Both `ItemsNavigation` and `LazyChildItems` provide `navigate`/`navigate_with`,
`set_items`/`set_items_with` and `set_active_items`/`set_active_items_with`.
The helpers accept transformers over the current list or active map. The source
broadcasts to each receiver; the lazy handle changes only its own receiver.
Explicit completions receive typed old/new Items states or errors; convenience
calls panic on rejected navigation. No-op events publish and complete.

`LazyChildItems::get()` returns the current snapshot, `value()` returns an
observable Value, and `subscribe()` observes it. `get_or_create(&configuration)`
returns an active component unchanged or synchronously activates an inactive
member as Created. An absent member returns UnknownConfiguration. An inactive
fetch during navigation/construction/Back dispatch returns NavigationInProgress
without queuing activation. If queued user work removes or deactivates the new
component before the call completes, it returns ChildUnavailable. The same error
rejects an ended scope still present in a snapshot after a caught lifecycle panic;
the getter checks actual scope availability before returning a component. A destroyed
handle returns Destroyed. Dropping the source leaves local navigation available;
dropping views does not end parent-owned navigation.

Deactivation saves child state and ends the component/keeper. Reactivation uses
the same stable key and saved state with fresh component/keeper instances. Removal
forgets the key/state; re-adding an equal configuration starts afresh. The model
has no automatic Back operation. Effective Started/Resumed child handlers can
handle Back; Created and inactive children cannot. Queued recursive events,
predictive cancellation and deferred Back scope changes use the common engine.

`ItemsCodec` has ordered configuration-list callbacks. DPI1 separately records
activity, keys, counter and child states, including inactive entries. Saved empty
or all-inactive membership overrides initial input. Structural corruption is
rejected before user decode/factories; decoded count/uniqueness is checked before
factories. Transient Items skips child providers during parent saves and resets
on recreation. Existing DST1, DSL1, DPG1 and DPN1 bytes remain compatible.

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
| public `router::stack`, `router::slot`, `router::pages`, `router::panels`, `router::items` | Model APIs, operations, snapshots and codecs |
| private router machinery | Shared scope ownership, transactions and queued events |
| private model state modules | DST1 stack / DSL1 slot / DPG1 pages / DPN1 panels / DPI1 items envelopes |

Dependencies remain the four published `essenty-* 0.1.0` primitives and
`thiserror`. No Essenty source changes, path/git overrides or additional runtime
are required. [ADR-0005](docs/adr/0005-stack-and-retention-corrections.md) records
implemented corrections and supersedes conflicting assumptions in older
Phase 1 research. The [approved design](docs/superpowers/specs/2026-10-09-component-context-child-stack-design.md)
and [implementation plan](docs/superpowers/plans/2026-10-09-component-context-child-stack.md)
record the first delivery boundaries.
[ADR-0006](docs/adr/0006-child-slot-and-private-navigation-engine.md) and the
[ChildSlot design](docs/superpowers/specs/2026-10-09-child-slot-design.md) describe
the optional-child model and the shared private engine. Existing
`router::stack::Child` and `router::stack::NavigationError` imports remain valid;
the common types are also available under `router` and `router::slot`.
[ADR-0007](docs/adr/0007-child-pages-and-destroyed-page-state.md) and the
[ChildPages design](docs/superpowers/specs/2026-10-09-child-pages-design.md)
describe stateful navigation policies and page destruction/recreation.
[ADR-0008](docs/adr/0008-typed-child-panels.md) and the
[ChildPanels design](docs/superpowers/specs/2026-10-09-child-panels-design.md)
describe heterogeneous role types and lifecycle modes.
[ADR-0009](docs/adr/0009-lazy-child-items.md) and the
[ChildItems design](docs/superpowers/specs/2026-10-09-child-items-design.md)
describe configuration-keyed activity and receiver-local lazy activation.

## Verification (Rust 1.96.0)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo run --example child_stack
cargo run --example child_slot
cargo run --example child_pages
cargo run --example child_panels
cargo run --example child_items
```

Verified on 2026-10-09: **325 unit/integration tests and 1 doctest passed**.
Format, strict Clippy, strict rustdoc and all five runnable examples passed. Library
compilation passed all 24 installed targets, including WASM, Apple, Android,
Linux and Windows. The [ChildItems implementation plan](docs/superpowers/plans/2026-10-09-child-items.md)
records counts, commands, target matrix and review findings.

Regression tests cover restored-state loss, retry after construction failure,
retained recreation, non-Clone custom nesting, identity/lifecycle ordering,
Back priority, repeated callback entry, persistence errors and teardown.
Pages tests also cover lazy creation, state-preserving tombstones, fresh-instance
recreation under a stable key, custom lifecycle policies and restored selection.
Panels tests cover all twelve mode/presence combinations, 36 mode transitions,
typed role identity, hook ordering/reentry, panic recovery, Back modes and strict
DPN1 restoration. Old DST1, DSL1 and DPG1 fixture bytes remain compatible.
Items tests cover lazy/local navigation, inactive state recovery, updated equal
configuration data, lifecycle/Back gates, strict DPI1 framing and panic/ownership
recovery; the DPN1 fixture also remains compatible.
Compile-target results are recorded in the implementation plan; compilation
alone does not establish platform runtime behavior.

## License

Apache-2.0 — see `LICENSE-APACHE` and `NOTICE`.
