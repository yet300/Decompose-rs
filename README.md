# Decompose-rs

Independent Rust port of [Ark Ivanov’s Decompose](https://github.com/arkivanov/Decompose),
implemented on top of [Essenty-rs](https://github.com/yet300/Essenty).

**Current scope: the independent Decompose core, including portable navigation history.** The port includes observable
values, navigation operations, child lifecycle management, Back routing and
configuration/state restoration. ChildSlot represents zero-or-one children and
supports activation, replacement and dismissal. ChildPages keeps page identities and
saved state while creating or destroying instances according to a lifecycle policy.
ChildPanels manages typed Main/Details/Extra components in Single, Dual or Triple mode.
ChildItems manages a unique list with per-configuration lifecycle targets and
explicit lazy component creation. Generic Navigation lets consumers define custom
state, typed events, child lifecycle targets and an observable result.
Standard routers accept custom event sources and lazy initial-state factories.
Selective and whole-state codecs, owned memory navigation snapshots, explicit
`DecomposeConfig` with typed diagnostics and a public test context complete the portable
state/recreation contracts. History supports recursive WebNavigation and the older
stack-only algorithm over an explicit host backend.
The [compatibility matrix](docs/COMPATIBILITY.md) records verified behavior,
Rust adaptations, evidence and excluded integrations.
Platform/UI integrations are outside this delivery. Behavior is checked against pinned
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

navigation.push_new_or_panic(Route::Details(7));
assert_eq!(stack.get().items().count(), 2);
navigation.pop_or_panic();
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
Run `cargo run --example generic_navigation` for a custom empty-capable model,
typed events, metadata, lazy child state, conditional Back and restoration.
Run `cargo run --example observable_values` for cached mapping, identity-based
updates and lifecycle-bound subscriptions.

## Standard navigation extension points

The crate provides public `StackEvent`, `SlotEvent`, `PagesEvent`,
`PanelsEvent` and `ItemsEvent` aliases. Each describes a shared
`NavigationEvent<State, Result>` with `new`, `modify`, `with_completion`, `transform` and
`complete`. `new` uses a no-op completion; `try_*` navigator methods return
`Result<NavigationOutcome<R>, NavigationError>` (a `Pending` receipt when nested work is
accepted during a surrounding dispatch), `*_or_panic` variants panic with caller location,
while `navigate_with` reports typed errors per receiver.

Standard child factories accept `NavigationSource<FamilyEvent>`, including
`dyn NavigationSource`. A custom source can wrap `SimpleNavigation` to filter,
log or generate events. The concrete navigation constructors implement the same
protocol. Connected receivers apply events to their own current state, complete
after publication and cancel their connection when the hosting lifecycle ends.

`StackNavigator`, `SlotNavigator`, `PagesNavigator`, `PanelsNavigator` and
`ItemsNavigator` expose reusable operations. Custom implementations supply
`navigate_with` and inherit the family helpers. These traits use static dispatch
because their callbacks are generic; `NavigationSource` is object-safe. Existing
inherent concrete methods remain available without importing navigator traits.

The shared `SimpleNavigation` transport preserves event Rc identity, registration
order and nested FIFO delivery. It discards disconnected events and never replays
them. Subscription-time and construction-time events enter the engine's guarded
queue. A custom `subscribe` that panics before returning its token is responsible
for rolling back its own incomplete registration.

Run `cargo run --example router_extension_points` for an intercepted Stack source,
a generic navigator helper, dyn source connection, completion and teardown.
The [design](docs/superpowers/specs/2026-10-10-router-extension-points-design.md)
records the event, source and navigator contracts.

## Lazy initial state

Existing factories accept an eager state. Each also has a `_with_initial`
counterpart taking `impl FnOnce() -> State`:

| Function | Initializer result |
|---|---|
| `stack::child_stack_with_initial` | `Vec<C>` |
| `slot::child_slot_with_initial` | `Option<C>` |
| `pages::child_pages_with_initial` | `Pages<C>` |
| `panels::child_panels_with_initial` | `Panels<MC, DC, EC>` |
| `panels::child_panels_without_extra_with_initial` | `Panels<MC, DC>` |
| `items::child_items_with_initial` | `Items<C>` |

The initializer may borrow local data or consume non-Clone captures; it has no
`'static` bound. The router invokes it synchronously during construction once
when no usable state is restored. Valid restored state skips it entirely,
including empty Slot/Pages/Items and inactive Items. Initialization is independent
of later observable reads and lazy Items component creation. Existing eager
calls and named generic parameter counts remain compatible.

```rust
use decompose_rs::router::stack::{child_stack_with_initial, StackNavigation, StackOptions};
use decompose_rs::{DefaultComponentContext, LifecycleRegistry, LifecycleState};

let lifecycle = LifecycleRegistry::new();
lifecycle.move_to(LifecycleState::Resumed).unwrap();
let context = DefaultComponentContext::new(lifecycle);
let navigation = StackNavigation::new();
let routes = vec![String::from("home")];
let mut calls = 0;
let stack = child_stack_with_initial(&context, &navigation,
    || { calls += 1; routes }, StackOptions::default(), |_, context| context,
).unwrap();
assert_eq!(calls, 1);
assert_eq!(stack.get().active.configuration.as_str(), "home");
context.lifecycle().destroy().unwrap();
```

Destroyed/duplicate-key preflight and invalid restoration never evaluate the
initializer. Codec errors propagate rather than falling back to it. Invalid
initial state or initializer panic uses existing construction rollback and source
cancellation. Events emitted during initialization are queued until initial
publication; captures are released before construction returns, even when skipped.
Run `cargo run --example lazy_initial_state` for move/borrow initialization and
restoration of routes, stable keys and nested child state without initial evaluation.

## Observable values

`value::MutableValue<T>` owns a writable source; `value()` returns its read-only
`Value<T>`. `get()` returns an `Rc<T>` snapshot. `set`/`set_rc` commit immediately
and synchronously notify observers; nested writes conflate to the latest snapshot
after the current notification round. Payloads need no Clone or Eq implementation.
`subscribe` replays immediately and returns a cancellation token; Drop cancels,
while `detach` leaves the observer registered.

`Value::map` computes its initial output and caches by input Rc identity. Reads
refresh even without subscribers. Multiple readers/observers share cached output
and avoid repeated mapper work for the same input snapshot. A fresh equal payload
is a new input; assigning the same Rc can notify without remapping. Mapping can
be chained. Mapper panic leaves the previous cache usable for retry. Recursive
reads of the same in-flight uncached input panic instead of recursing indefinitely.
When a mapper reentrantly changes its source, its result still belongs to the
input snapshot it received; a newer cache populated during that call is preserved.

`MutableValue::compare_exchange(&expected, next)` exchanges Rc identity, returning the
predecessor on success or the current snapshot on failure. `update_rc`,
`update_rc_and_get` and `get_and_update_rc` take `FnMut(Rc<T>) -> Rc<T>` transformers
and retry if reentrant mutation makes their expected snapshot stale. A transformer
may run multiple times. The latter two return the installed snapshot or its
predecessor, even if observer reentry immediately changes the source again. These
are synchronous operations without multithreaded atomicity guarantees. `modify`
edits a cloned payload through `&mut T` and publishes a new snapshot; `get_cloned`,
`with`, `subscribe_ref` and `subscribe_cloned` provide owned/borrowed reads without
Rc ceremony for `Clone` payloads.

`subscribe_with_lifecycle(&lifecycle, mode, observer)` observes at Created+, Started+
or Resumed using `ObserveLifecycleMode::{CreateDestroy, StartStop, ResumePause}`;
StartStop is the default mode. It skips updates while inactive and replays the
latest value whenever observation resumes. Its returned token owns both source
and lifecycle registrations: cancelling/dropping removes both, while detach keeps
the binding until lifecycle destruction. Detached lifecycle bindings require an
explicit `Destroy` transition to release resources: Essenty has no weak registry
handle, so they retain a lifecycle witness. Ordinary RAII bindings release their
captures on cancellation even without `Destroy`. Setup handles lifecycle shutdown
during synchronous replay and rolls back registrations when replay panics.

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

`StackNavigation` provides `navigate_with`/`navigate_or_panic`, `try_push`/`push_or_panic`,
`try_push_new`/`push_new_or_panic`, `try_push_to_front`/`push_to_front_or_panic`,
`try_pop`/`pop_or_panic`, `try_pop_while`/`pop_while_or_panic`,
`try_pop_to`/`pop_to_or_panic`, `try_pop_to_first`/`pop_to_first_or_panic`,
`try_replace_current`/`replace_current_or_panic`,
`try_replace_all`/`replace_all_or_panic` and
`try_bring_to_front_by`/`bring_to_front_by_or_panic` (plus
`try_replace_kind_and_push`). Root pop and pushing an already active
configuration with `push_new_or_panic` are no-ops. `bring_to_front_by_or_panic` takes a kind
selector so Rust enum variants can replace Kotlin runtime-class matching.

`child_stack` returns a typed construction error. Fallible `try_*` methods return
`Result<NavigationOutcome<R>, NavigationError>`; `*_or_panic` variants panic with
caller location on rejected transformations (for example, an empty stack or disallowed
duplicates). Use `navigate_with` for a `Result<NavigationResult<C>, NavigationError>`
completion callback. A shared source reports once per connected stack; accepted nested
work returns a `Pending` receipt whose results are taken after the surrounding
dispatch finishes. Abandoned queued work (after an observer panic) reports
`ChildUnavailable`. User panics propagate, with cleanup of
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

navigation.activate_or_panic("Confirm deletion".into());
assert!(slot.get().child.is_some());
navigation.dismiss_or_panic();
assert!(slot.get().child.is_none());
lifecycle.destroy().unwrap();
```

`activate` with an equal configuration preserves the child's instance, stable
key and lifecycle. Replacement ends the outgoing child before resuming the new
one; `dismiss` destroys the child's lifecycle and retained scope. Empty dismissal
is a no-op. A child targets Resumed, bounded by the parent. No-op navigation still
publishes an observable snapshot and runs completion, as in upstream.

`try_navigate`/`navigate_or_panic` transform `Option<Rc<C>>`. `activate_with` reports
old/new configuration snapshots through a typed completion;
`dismiss_with` reports `Result<bool, NavigationError>`, true if a child existed.
Fallible helpers return `Result<NavigationOutcome<R>, NavigationError>` and explicit
completions receive errors. Shared sources report independently for each connected slot.

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

`PagesNavigation` provides `try_navigate`/`navigate_or_panic`,
`try_select`/`select_or_panic`, `try_select_next`/`select_next_or_panic`,
`try_select_prev`/`select_prev_or_panic`, `try_select_first`/`select_first_or_panic`,
`try_select_last`/`select_last_or_panic`, `try_clear`/`clear_or_panic` and
`try_set_items`/`set_items_or_panic`. Boundary movement clamps unless circular behavior is
requested. Empty selection operations are no-ops. `set_items_or_panic` clamps the previous
numeric index to the new list; it does not follow the selected configuration
when items move. Explicit completion callbacks receive typed errors for invalid
nonempty selection; `*_or_panic` variants panic on rejection.

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

Navigation provides `try_navigate`/`navigate_or_panic`, `try_set_panels`/`set_panels_or_panic`,
`try_set_details_extra`/`set_details_extra_or_panic`, `try_set_extra`/`set_extra_or_panic`,
`try_activate_main`/`activate_main_or_panic`, `try_activate_details`/`activate_details_or_panic`,
`try_activate_extra`/`activate_extra_or_panic`, `try_dismiss_details`/`dismiss_details_or_panic`,
`try_dismiss_extra`/`dismiss_extra_or_panic`, `try_pop`/`pop_or_panic` and
`try_set_mode`/`set_mode_or_panic`.
Helpers have `*_with` forms reporting typed old/new state per connected receiver.
`pop_or_panic` removes Extra first, then Details, and preserves Main. Explicit pop works
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
items.set_active_items_or_panic(|mut active| { active.remove(&2); active });
assert_eq!(component.lifecycle().state(), LifecycleState::Destroyed);
lifecycle.destroy().unwrap();
```

Both `ItemsNavigation` and `LazyChildItems` provide `try_navigate`/`navigate_or_panic`,
`try_set_items`/`set_items_or_panic` and
`try_set_active_items`/`set_active_items_or_panic`.
The helpers accept transformers over the current list or active map. The source
broadcasts to each receiver; the lazy handle changes only its own receiver.
Explicit completions receive typed old/new Items states or errors; `*_or_panic`
variants panic on rejected navigation. No-op events publish and complete.

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

## Generic Navigation

`router::children` exposes `children`, `NavState<C>`, `ChildNavState<C>`,
`ChildStatus`, `ChildItem<C, T>`, `NavigationSource<E>`, `SimpleNavigation<E>`,
`ChildrenOptions<E, N>`, `NavigationResult<N>` and `NavStateCodec<N>`.
It supports custom navigation models, including an empty stack or several active
components. This follows the [upstream Generic Navigation API](https://arkivanov.github.io/Decompose/navigation/children/overview/).

Implement `NavState::children()` on an immutable Clone whole-state, or use
`Vec<ChildNavState<C>>` directly. Each descriptor contains an Rc configuration
and Created, Started, Resumed or Destroyed status. Configurations require Eq,
without Clone, Hash or Ord. Destroyed entries retain identity/state and own no
live component. A later live status creates a fresh component under the same key.
Equal live configurations retain the component. A fresh factory uses current
descriptor data, including updated payload fields ignored by Eq. Inactive records
use the current descriptor handle. Removing the descriptor discards its saved
state and identity. Duplicates require
explicit `allow_duplicates`; matching then follows equal occurrence.

`children` accepts a context, event source, lazy initial-state factory, options,
a `Fn(N, &E) -> N` transformer, a `Fn(&N, Vec<ChildItem<C, T>>) -> S` mapper and
a child factory. It returns `Value<S>`. The initial factory is skipped when
restoration supplies a state, including an empty one. ChildItem exposes the
same live/destroyed records as Pages: `pages::PageChild` and `pages::PageStatus`
remain valid aliases of the neutral types.

`SimpleNavigation::navigate(event)` broadcasts a typed event to each connected
receiver; `navigate_rc` accepts an existing shared handle. Event, component and
result payloads need no Clone. A custom object-safe `NavigationSource<E>`
implements `subscribe(Rc<dyn Fn(Rc<E>)>) -> Subscription`, using the public
subscription cancellation guard. SimpleNavigation has no replay; events without
subscribers are discarded. Nested broadcasts finish the current broadcast before
delivering queued events. Subscription Drop cancels; detach leaves the observer
registered. Construction subscribes before initial/factory callbacks and buffers
initialization events until the receiver is ready.

Optional `on_state_changed` runs initially with old=None after provider/Back
registration and later after observable publication, before explicit event
completion. `on_event_complete` receives the original event and
`Result<NavigationResult<N>, NavigationError>` once per receiver. Rejected
transitions report errors without replacing committed state. Back has no typed
event completion. With no completion hook, a rejected explicit event panics;
install the hook to handle typed errors. Callback panics propagate and queued work is abandoned;
successful callback effects and committed state remain.

Navigation teardown completes its own cleanup before propagating a cancellation
panic. Essenty lifecycle dispatch can then stop before calling other independent
listeners, including a default root's attached InstanceKeeper cleanup. Retained
keeper lifetime still belongs to its parent owner; callers catching such a panic
must finish cleanup of any owner whose lifecycle listener was skipped.

`back_transformer` derives an optional zero-argument action from each committed
state. The action is cached; None disables automatic Back. Effective Started/
Resumed child handlers take precedence. Predictive cancellation preserves
navigation; invoked Back shares the deferred receiver queue.
A rejected user Back action panics with the navigation error and produces no
typed event completion.

`NavStateCodec::new` supplies whole-state encode/decode callbacks returning
`Result<Option<_>, String>`. Encode None skips persistence and child provider
queries. Decode None explicitly
declines valid saved input and runs the initial factory; decoder errors propagate.
DCH1 stores the opaque caller payload plus allocation counter, ordered stable
keys and nested states for all children. Structural validation precedes decode,
then child count/uniqueness are checked before factories. The codec must preserve
configuration identity and order; restored lifecycle statuses may differ.
Without a codec or explicit memory selection, navigation resets on recreation
and parent saves skip its child providers.
All five existing family save formats remain compatible.

Run `cargo run --example generic_navigation` for a custom gallery with non-Clone
events, metadata, live/destroyed children, state-preserving recreation, conditional
Back down to empty and whole-state restoration.

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

The public `StateKeeper` is now a Decompose adapter around Essenty’s primitive.
Contexts expose opaque `StateStore` and `InstanceStore` services whose operations
return owned data instead of `RefCell` guards, so no caller-held borrow can
conflict with navigation or teardown. This lets failed construction return
consumed input for retries and saves, without replacing the parent keeper or
changing Essenty. Convert existing owned primitives with
`StateKeeper::from(essenty_keeper)`; the raw type is reexported as
`EssentyStateKeeper`. The restored child frame is also the first-save fallback, so a failed
first child save does not discard previously restored data.

## Selective, whole-state and memory persistence

All five standard codecs provide `should_save` and `should_restore` predicates.
Skipping a save avoids the encoder and nested providers; declining fully decoded,
validated state runs the lazy initializer and discards that saved subtree. Empty
state restores normally; damaged framing/invalid membership remains an error.
Existing codec constructors preserve their old behavior and Rust byte formats.
`PagesCodec::new_state`, `ItemsCodec::new_state` and `PanelsCodec::new_state` also
expose the whole model state: a decoder may choose a different page selection,
activity map or panel mode while preserving ordered child membership/roles.
`SlotCodec::new_state` additionally accepts a nullable decoder: a populated save
can restore as empty without invoking the initializer. Empty DSL1 slots have no
payload and keep their existing callback-skipping behavior.
Run `selective_persistence` and `whole_state_persistence` examples for these APIs.

Enable memory-only navigation on a context's state keeper **before** creating a
router with `enable_memory_navigation(key)`. `save_snapshot()` produces an owned
`StateSnapshot`; `StateKeeper::from_snapshot(snapshot)` recreates from it, including
nested scopes, stable keys and actual Rc configurations without a byte codec.
`save()` and `snapshot.into_persistent()` omit the memory-only router subtree.
Memory selection takes precedence over a byte codec and its predicates.
Without that selection, `codec: None` retains its existing reset-on-recreation
behavior. Snapshots have explicit owners and release data when the last owner
is dropped; they do not retain running controllers or components.
Run `cargo run --example memory_navigation`.

## Configuration and test contexts

`DecomposeConfig` carries explicit duplicate policy and an opt-in diagnostic hook;
it is attached to contexts with `with_config` and to router options with
`with_config`. Stack/Pages/Generic options default to rejecting duplicates;
explicit `allow_duplicates` always wins, and running routers keep captured policy.
Items remains unique-only. The configured `on_error` hook observes typed diagnostics
while `Result`/completion errors and `*_or_panic` panics retain their existing
behavior. There is no global mutable state and no OS/UI main-thread check:
Rc/RefCell enforce the single-thread architecture, while host adapters own
UI-thread identity. Run `cargo run --example rust_api` for explicit configuration,
owned value reads and fallible navigation.

Public `TestComponentContext` requires no feature or extra crate. It starts
Resumed by default, exposes the standard service handles, and supports explicit
lifecycle states and supplied services. `recreate(true)` saves memory state and
preserves retained instances; `recreate(false)` saves durable bytes and resets
instances/memory navigation. Failed saves preserve the old context.
Run `cargo run --example test_context`.

## Portable browser history

`router::webhistory` provides `WebNavigation`, `TypedWebNavigation`, explicit
`RouteCodec`, standard Stack/Pages/Panels bridges, `HistoryController`,
`HistoryBackend` and `MemoryHistoryBackend`. The controller synchronizes paths,
merged/escaped query parameters and nested navigation with Back/Forward, guards,
queued restoration completions and lifecycle cancellation.
The recursive controller initializes one entry for the entire starting tree;
`StackHistoryController` implements the older upstream contract with one initial
entry per configuration, `history_paths`, suffix rewrites and a Forward path mapper.
Keep the returned binding alive while synchronization is needed.
Run `cargo run --example web_history` without any browser.

The host adapter must serialize managed Rust history entries and wire the actual
browser API/pop events. DOM/wasm bindings are outside this crate. Startup keeps an
existing managed entry without automatically restoring the model; applications
choose their initial deep-link/history reconstruction. Neither Rust navigation
frames nor history state decode Kotlin SerializableContainer or Kotlin JSON bytes.

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
| public `settings`, `test_utils` | Explicit `DecomposeConfig`/diagnostics and recreation harness |
| public `router::webhistory` | Recursive/legacy history, model bridges and host backend contract |
| public `router::children` | Custom models, typed event sources, neutral child records and whole-state codec |
| private router machinery | Shared scope ownership, transactions and queued events |
| private model state modules | DST1 stack / DSL1 slot / DPG1 pages / DPN1 panels / DPI1 items / DCH1 custom envelopes |

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
[ADR-0010](docs/adr/0010-public-generic-navigation.md) and the
[Generic Navigation design](docs/superpowers/specs/2026-10-09-generic-navigation-design.md)
describe custom event/state adapters over the shared engine.
[ADR-0011](docs/adr/0011-observable-values-completion.md) describes identity-based
updates, cached mapping and the RAII lifecycle binding.
[ADR-0012](docs/adr/0012-standard-router-extension-points.md) describes typed
standard events, source adapters and reusable navigator traits.
[ADR-0013](docs/adr/0013-lazy-initial-state-and-compatibility.md) describes lazy
standard initialization and the [compatibility matrix](docs/COMPATIBILITY.md).

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
cargo run --example generic_navigation
cargo run --example observable_values
cargo run --example router_extension_points
cargo run --example lazy_initial_state
cargo run --example selective_persistence
cargo run --example memory_navigation
cargo run --example rust_api
cargo run --example test_context
cargo run --example web_history
cargo run --example whole_state_persistence
```

Verified on 2026-10-10: **519 unit/integration tests and 1 doctest passed**.
Formatting, strict Clippy/rustdoc, all 15 examples and library compilation on all
24 installed targets passed. The no-default-features run passed the same tests.
Complete commands, target names and independent review evidence are recorded in the
[independent-core completion plan](docs/superpowers/plans/2026-10-10-independent-core-completion.md).
The stage adds regression coverage for selective/whole-state persistence,
in-process memory ownership, explicit configuration with isolated diagnostics,
opaque store services, public test utilities and both
history algorithms, including deferred completion, veto, cancellation and panic
recovery. All old format fixtures remain covered. Cross-target compilation is
not runtime validation of an OS/UI/browser integration.

Lazy initialization tests cover all six factory variants, moved/borrowed FnOnce
captures, authoritative restoration including empty/inactive states, transient
mode, preflight/decoder rejection, initialization FIFO, capture Drop reentry and
parent destruction, panic/cancellation cleanup and exact saved-input retry.
Router extension tests cover non-Clone public events, custom/dyn sources, generic
helpers, shared event identity, FIFO, per-receiver results, construction-time
buffering, every family's Back policy and guarded cancellation/rollback.
Observable tests cover identity updates, stale retries, cached/chained mapping,
mapper reentry and panic recovery, every lifecycle gate, replay shutdown,
registration rollback and RAII/detached capture release.
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
Generic Navigation tests cover custom sources and initialization buffering,
event identity, state projection, callback order, cached Back and rejected actions,
strict DCH1 validation, optional codecs, retained recreation and coherent saves.
Additional regressions cover reentrant custom state Clone/Drop, preservation of
committed scopes when Drop panics, and complete navigation teardown after cancellation or
multiple cleanup panics. Constructor rollback restores saved input and prior
retention after cancellation panics. Subprocess regressions verify that combined
initialization/cancellation panics preserve the original unwind, including lazy
initial-state, decoder and pre-engine Clone failures.
Compile-target results are recorded in the implementation plan; compilation
alone does not establish platform runtime behavior.

## License

Apache-2.0 — see `LICENSE-APACHE` and `NOTICE`.
