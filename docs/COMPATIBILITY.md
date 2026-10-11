# Decompose compatibility matrix

Audit date: 2026-10-10. Reference: upstream Decompose
[`00840bcfed6d55710c8fbb1a72503f2041eda490`](https://github.com/arkivanov/Decompose/tree/00840bcfed6d55710c8fbb1a72503f2041eda490).
Rust baseline for this revision: `2c45df6` on `main`. Dependencies are the four published
`essenty-* 0.1.0` crates and `thiserror 2`.

This is a behavioral/API mapping for the independent Rust core, not a promise
of Kotlin source or serialization compatibility. Links below use the pinned
revision. Source inspection and Rust regression tests support the conclusions;
the Kotlin test suite has not been executed as a differential oracle.

Status meanings: **Implemented** means the stated behavior is present;
**Adapted** means it is present with the explicitly described Rust contract;
**Outside scope** identifies
an excluded integration. Verification results and their practical limits are
recorded below and in the stage plan.

## Contexts, ownership and Back

| Upstream contract | Rust mapping and status | Evidence |
|---|---|---|
| GenericComponentContext, ComponentContext, factory owner | **Adapted:** one `ComponentContext: Sized` trait with opaque `state_store`/`instance_store` services plus `lifecycle`, `back_handler`, `context_factory()` and `make_context()`. Custom contexts propagate services without Clone. | [upstream generic context][generic-context]; [Rust context](../src/context.rs), [opaque stores](../src/context/stores.rs), [context regressions](../tests/component_context.rs), [nested routers](../tests/child_stack.rs) |
| DefaultComponentContext | **Implemented:** default and supplied store/Back handles with explicit `DecomposeConfig`; a default-owned instance scope attaches to lifecycle destruction. | [upstream default][default-context]; [Rust context](../src/context.rs), its unit tests |
| ComponentContextFactory | **Adapted:** `Rc<dyn Fn(ContextParts) -> Ctx>` consumes prepared handles; the factory is detached. Whole-parts delegation preserves custom context assembly. | [upstream factory][context-factory]; [Rust context](../src/context.rs), [ADR-0002](adr/0002-context-factory.md) |
| childContext, explicit lifecycle and Back priority | **Implemented:** namespaced state/instances/Back, parent-held scope lifetime and duplicate-key rejection. Fixed children use the parent's lifecycle; explicit child scopes are bounded by it. | [upstream extensions][context-ext]; [child scopes](../src/child.rs), [context regressions](../tests/component_context.rs), context unit tests |
| MergedLifecycle | **Implemented:** child lifecycle is bounded by parent and requested target; terminal destruction and callback-driven reentry are handled. | [upstream merged lifecycle][merged-lifecycle]; [Rust driver](../src/merged.rs), unit tests and all family lifecycle tests |
| ChildStateKeeper | **Adapted:** `StateKeeper` wraps Essenty, keeps byte providers and restores consumed frames after failed construction; root raw primitive is `EssentyStateKeeper`. | [upstream child state][child-state]; [Rust adapter](../src/state.rs), [scope framing](../src/framing.rs), [context regressions](../tests/component_context.rs) |
| ChildInstanceKeeper and default attachTo | **Implemented:** parent keeper owns retained scopes across lifecycle recreation; removed/deactivated child scopes release retention. Holding a snapshot can keep an object alive after its lifecycle ends. | [upstream child instances][child-instances]; [retention](../src/retention.rs), [context regressions](../tests/component_context.rs), [router restoration](../tests/child_stack.rs) |
| ChildBackHandler and predictive phases | **Adapted:** `SharedBack` provides priority/LIFO selection, effective-lifecycle gates and deferred FIFO work; predictive cancellation preserves membership. Host supplies gesture events. | [upstream child Back][child-back]; [Rust dispatcher](../src/shared_back.rs), unit tests and [family Back tests](../tests/child_panels.rs) |
| Manual destruction and callback failures | **Adapted:** typed construction errors, documented manual-child destruction panic, guarded rollback; successful user callback side effects remain. Essenty lifecycle dispatch may stop independent listeners after a panic. | [context errors](../src/error.rs), [engine](../src/router/engine.rs), [panic regressions](../tests/generic_navigation.rs), [ADR-0005](adr/0005-stack-and-retention-corrections.md) |

## Standard navigation families

| Upstream contract | Rust mapping and status | Evidence |
|---|---|---|
| ChildStack: active child plus back stack | **Implemented:** nonempty state, active Resumed/back Created, stable identity, parent bounds and automatic Back excluding root pop. | [upstream Stack factory][stack-factory]; [stack module](../src/router/stack/mod.rs), [tests](../tests/child_stack.rs) |
| Stack helper operations | **Adapted:** `try_push`/`push_or_panic`, `try_push_new`/`push_new_or_panic`, `try_push_to_front`/`push_to_front_or_panic`, `try_pop`/`pop_or_panic`, `try_pop_while`/`pop_while_or_panic`, `try_pop_to`/`pop_to_or_panic`, `try_pop_to_first`/`pop_to_first_or_panic`, `try_replace_current`/`replace_current_or_panic`, `try_replace_all`/`replace_all_or_panic`, `try_bring_to_front_by`/`bring_to_front_by_or_panic` (hidden alias `try_replace_kind_and_push` forwards to `try_bring_to_front_by`). `bring_to_front_by_or_panic` takes an explicit group selector instead of Kotlin runtime class. Fallible helpers return `NavigationOutcome` (a `Pending` receipt for accepted nested work); `*_or_panic` variants panic with caller location. Helper-specific Kotlin callback overloads are not copied. | [upstream Stack helpers][stack-helpers]; [Rust helpers](../src/router/stack/navigation.rs), module unit tests and [stack tests](../tests/child_stack.rs) |
| ChildSlot and activate/dismiss | **Implemented:** Option membership, equal-child reuse, replacement/destruction, empty no-op and optional automatic dismissal on Back. `try_activate`/`activate_or_panic`, `try_dismiss`/`dismiss_or_panic` and the `*_with` typed-completion forms. | [upstream Slot factory][slot-factory], [helpers][slot-helpers]; [slot module](../src/router/slot/mod.rs), [navigation](../src/router/slot/navigation.rs), [tests](../tests/child_slot.rs) |
| ChildPages, statuses and navigation | **Adapted:** configurable four-state lifecycle policy, live/destroyed records and state-preserving resurrection; `try_select`/`select_or_panic`, next/prev/first/last, `try_clear`/`clear_or_panic`, `try_set_items`/`set_items_or_panic` plus `*_with` completions. Empty selection is `None` rather than Kotlin -1. | [upstream Pages factory][pages-factory], [helpers][pages-helpers]; [pages module](../src/router/pages/mod.rs), [navigation](../src/router/pages/navigation.rs), [tests](../tests/child_pages.rs) |
| ChildPanels: typed roles and Single/Dual/Triple | **Implemented:** three-role and no-extra adapters, typed factories, mode lifecycles, activation/dismissal/pop/mode changes and Single-only automatic Back. Kotlin navigate overloads map to named `try_set_panels`/`set_panels_or_panic`, `set_details_extra`, `set_extra`, `activate_*`, `dismiss_*`, `pop`, `set_mode` pairs plus `*_with` completions. | [upstream Panels factory][panels-factory], [helpers][panels-helpers]; [panels module](../src/router/panels/mod.rs), [navigation](../src/router/panels/navigation.rs), [tests](../tests/child_panels.rs) |
| ChildItems and LazyChildItems | **Adapted:** ordered unique Eq configurations, activity map and receiver-local helpers; `try_navigate`/`navigate_or_panic`, `try_set_items`/`set_items_or_panic`, `try_set_active_items`/`set_active_items_or_panic` plus `*_with` completions. `get_or_create(&C)` explicitly reports errors and preserves live identity. Linear `ItemMap` avoids mandatory Hash/Ord. No automatic model Back action. | [upstream Items factory][items-factory], [lazy interface][lazy-items]; [items module](../src/router/items/mod.rs), [map](../src/router/items/map.rs), [tests](../tests/child_items.rs) |
| Lazy initial state of all five families | **Adapted:** six `_with_initial` functions accept borrowed/owned FnOnce, including Panels without extra. Evaluation is synchronous during construction, zero times with valid restoration and once on the no-restoration path. Eager calls remain. | [upstream factories][stack-factory] / [Slot][slot-factory] / [Pages][pages-factory] / [Panels][panels-factory] / [Items][items-factory]; standard factory modules, [lazy regression suite](../tests/lazy_initial_state.rs), [example](../examples/lazy_initial_state.rs) |
| Navigation/Navigator event interfaces | **Adapted:** concrete constructors plus five generic navigator traits, public family event aliases and `NavigationEvent<S,R>`. Sources are object-safe; navigator traits use static dispatch for generic callbacks. Helpers are defined only on those traits, which concrete helper callers import; `new` and `navigate_with` remain inherent. | [upstream Stack source][stack-navigation]; [public event](../src/router/navigation.rs), each family navigation module, [extension tests](../tests/router_extension_points.rs) |
| Custom sources in standard factories | **Implemented:** every factory accepts borrowed/dyn `NavigationSource<FamilyEvent>`, allowing wrappers, interception or direct emission while keeping standard model policies. | [upstream source][navigation-source], all factory links above; [extension tests](../tests/router_extension_points.rs), [example](../examples/router_extension_points.rs) |
| Event delivery and completion | **Adapted:** shared Rc event identity, no replay, nested FIFO, receiver-specific state/results and post-publication completion. `NavigationEvent::new` is no-op completion; `try_*` helpers return `NavigationOutcome` (a `Pending` receipt for accepted nested work, abandoned work reports `ChildUnavailable`), `*_or_panic` variants panic with caller location, and explicit completion receives Result. | [upstream SimpleNavigation][simple-navigation]; [transport](../src/router/children/navigation.rs), [adapter](../src/router/navigation.rs), [extension tests](../tests/router_extension_points.rs) |
| Construction and teardown | **Implemented with documented boundaries:** initialization events are buffered; source tokens are cancelled on rollback/Destroy. Custom subscribe owns cleanup before returning its token. Parent owns live navigation even after view/source handles are dropped. | [engine](../src/router/engine.rs), [extension regressions](../tests/router_extension_points.rs), [ADR-0012](adr/0012-standard-router-extension-points.md) |

## Generic Navigation and observable values

| Upstream contract | Rust mapping and status | Evidence |
|---|---|---|
| children with arbitrary NavState | **Adapted:** immutable Clone whole-state, Eq-only configuration descriptors, arbitrary observable projection and four lifecycle statuses. Internal upstream navigator/controller classes remain private engine machinery. | [upstream children][children-factory]; [public adapter](../src/router/children/mod.rs), [types](../src/router/children/types.rs), [tests](../tests/generic_navigation.rs) |
| Generic hooks, conditional Back and lazy initial state | **Implemented:** initial state-change hook, per-event hook/errors, cached optional Back action, FnOnce initial state and initialization FIFO. | [upstream children][children-factory]; [generic tests](../tests/generic_navigation.rs), [example](../examples/generic_navigation.rs) |
| Value/MutableValue, immediate replay | **Adapted:** `Value<T>` is a read-only Rc snapshot view, `MutableValue<T>` owns writes; no payload Clone/Eq. Subscription is a RAII token. Reentrant writes conflate to latest snapshot after the current round. | [upstream Value][value], [MutableValue implementation][mutable-value]; [Rust value](../src/value.rs), unit tests and [observable regressions](../tests/observable_values.rs) |
| compareAndSet/update/updateAndGet/getAndUpdate | **Adapted as explicit Rust names:** `compare_exchange` returns `Result<Rc<T>, Rc<T>>`; `update_rc`, `update_rc_and_get` and `get_and_update_rc` take `FnMut(Rc<T>) -> Rc<T>` and retry stale reentrant snapshots with exact committed identities. `modify` edits a cloned payload via `&mut T`; `get_cloned`, `with`, `subscribe_ref` and `subscribe_cloned` avoid Rc ceremony for `Clone` payloads. Synchronous only, no multithreaded atomicity guarantee. | [upstream mutable extensions][mutable-ext]; [Rust value](../src/value.rs), [observable tests](../tests/observable_values.rs) |
| Value.map | **Implemented:** eager initial mapping, identity cache, shared derived snapshots, chained reads and panic/reentry recovery. Recursive evaluation of the same uncached input has a documented panic guard. | [upstream Map][value-map]; [mapped value](../src/value/mapped.rs), [observable tests](../tests/observable_values.rs) |
| Lifecycle-bound observation | **Adapted:** CreateDestroy/StartStop/ResumePause gates and latest replay on reopening. Token Drop cancels both registrations; detach lasts until explicit Destroy, retaining an Essenty lifecycle witness. | [upstream lifecycle subscription][value-ext]; [Rust binding](../src/value/lifecycle.rs), [observable tests](../tests/observable_values.rs), [ADR-0011](adr/0011-observable-values-completion.md) |
| Kotlin delegated-property Value access | **Adapted:** explicit `get()` returns Rc; Kotlin property-delegate syntax has no direct Rust API counterpart. | [upstream Value extensions][value-ext]; [Rust value](../src/value.rs) |

## Persistence, settings, testing and history

| Upstream capability | Rust status and practical consequence | Evidence |
|---|---|---|
| Configuration/state serializers | **Adapted:** caller-supplied codecs to bytes, no mandatory serde or Kotlin serialization dependency. Standard family metadata belongs to the library; Generic codec saves whole state. State failures use only `NavigationError::StateSource(ErrorSource)`; `state_source` preserves typed errors and string codecs retain their original displayed messages. | [upstream NavStateSaver][state-saver]; standard codec modules and [generic codec](../src/router/children/state.rs) |
| Keys, nested child state and retained recreation | **Implemented:** stable identities/counter, strict framing, inactive child state, saved authoritative membership and guarded retries. | [stack](../tests/child_stack.rs), [slot](../tests/child_slot.rs), [pages](../tests/child_pages.rs), [panels](../tests/child_panels.rs), [items](../tests/child_items.rs), [generic tests](../tests/generic_navigation.rs) |
| Serialized bytes across languages | **Adapted boundary:** Rust DCS1/DST1/DSL1/DPG1/DPN1/DPI1/DCH1 remain local versioned formats. Kotlin bytes are not required by the selected scope; migration requires an explicit application converter. | [framing](../src/framing.rs), existing family fixture regressions |
| Declining valid saved state / skipping a particular save | **Implemented:** all five standard codecs expose should_save / should_restore. Skip runs before codecs/nested providers; decline runs only after full decode, membership and stable-key validation and invokes the lazy initializer. Empty state remains distinct. Generic NavStateCodec retains its optional encode/decode contract. | [selective tests](../tests/selective_persistence.rs), [ADR-0014](adr/0014-selective-standard-router-persistence.md) |
| transientNavStateSaver: retain in memory but not on disk | **Adapted:** StateKeeper.enable_memory_navigation(key), save_snapshot/from_snapshot and owned StateSnapshot preserve actual model state, child identities/counter and nested snapshots for all standard families and Generic. Explicit memory selection overrides byte codecs/predicates; codec=None alone still resets. | [memory tests](../tests/memory_navigation.rs), [ADR-0015](adr/0015-owned-memory-navigation-snapshots.md), [example](../examples/memory_navigation.rs) |
| Whole-state standard persistence callbacks | **Implemented:** additive PagesCodec/ItemsCodec/PanelsCodec.new_state encodes/decodes whole model state. Valid same-membership restoration may replace selection/activity/mode; old new codecs and framing stay compatible. Child count/order and panel-role presence follow upstream ownership contracts. SlotCodec.new_state accepts empty restoration from a populated payload without invoking the initializer. | [whole-state tests](../tests/whole_state_codecs.rs), [ADR-0018](adr/0018-whole-state-standard-codecs.md), [example](../examples/whole_state_persistence.rs) |
| DecomposeSettings duplicate policy | **Adapted as explicit configuration:** `DecomposeConfig` attached via `with_config` (contexts and `StackOptions`/`PagesOptions`/`ChildrenOptions`) carries the duplicate default; there is no global mutable state. Explicit `allow_duplicates` wins, even false; previously created options/running routers retain captured policy. Items remains unique-only. | [explicit-config tests](../tests/rust_api.rs), [settings](../src/settings.rs), [ADR-0017](adr/0017-settings-and-public-test-context.md), [ADR-0019](adr/0019-explicit-rust-api-surface.md) |
| DecomposeSettings error handler and main-thread logging | **Adapted as an opt-in hook:** `DecomposeConfig::with_error_handler` receives typed diagnostics alongside normal `Result`/completion/`*_or_panic` delivery; the default is silent and reentrant reports are suppressed. There is no fictitious OS/UI main-thread check: Rc/RefCell prevent cross-thread handle movement, while host adapters own UI-thread identity. | [explicit-config tests](../tests/rust_api.rs), [ADR-0017](adr/0017-settings-and-public-test-context.md), [ADR-0019](adr/0019-explicit-rust-api-surface.md) |
| Public decompose-test-utils | **Adapted:** public TestComponentContext in the same crate, initially Resumed or explicit state, supplied services, child factory and lifecycle/Back controls. recreate(true) preserves memory and retained services; false uses durable bytes and resets them. Save failure leaves the old context intact. | [external integration tests](../tests/test_utils.rs), [test_utils](../src/test_utils.rs), [example](../examples/test_context.rs) |
| WebNavigation / browser history | **Implemented portable behavior:** recursive WebNavigation, typed key/URL bridges for Stack/Pages/Panels, HistoryController, HistoryBackend and deterministic MemoryHistoryBackend. Active-child observation, Back/Forward, guards, deferred FIFO completions, cancellation and panic recovery; actual DOM/wasm host binding excluded. Direct optional Rc child represents owner capability. | [history tests](../tests/web_history.rs), [ADR-0016](adr/0016-portable-navigation-history.md), [example](../examples/web_history.rs) |
| Older stack WebHistoryController | **Implemented portable behavior:** StackHistoryController.history_paths before attachment, one initial entry per configuration, common-prefix suffix rewrites, desired/current target guard and Forward configuration reconstruction from stored paths. It deliberately differs from the newer recursive controller's one-entry startup. | [legacy and recursive history tests](../tests/web_history.rs), [pinned older implementation](https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/webMain/kotlin/com/arkivanov/decompose/router/stack/webhistory/DefaultWebHistoryController.kt), [ADR-0016](adr/0016-portable-navigation-history.md) |
| Android/Jetpack/Compose/SwiftUI/Reaktive and host adapters | **Outside scope:** core compiles for applicable targets but supplies no UI framework adapters or automatic host lifecycle/gesture wiring. No dependency integrations are ported just to copy upstream modules. | [pinned upstream project tree][upstream-tree]; [Cargo.toml](../Cargo.toml), [exports](../src/lib.rs) |
| Thread safety, concurrency and no_std | **Outside the selected Rust architecture:** synchronous Rc/RefCell core, no Send/Sync promise, runtime or no_std support. A separate concurrent mirror would require its own design. | [Cargo.toml](../Cargo.toml), [Rust value](../src/value.rs), [architecture decisions](PORTING_ROADMAP.md) |

## Public-source audit and verification boundary

The final audit examined commonMain public contexts/Child, all five navigation
factories/models/source/navigator helpers, Generic NavState/Saver, values and
operators, settings, Cancellation, public test utilities and WebNavigation. It
also extracted both independent history algorithms from webMain. The starting
matrix missed the older WebHistoryController and full-state Pages/Items/Panels plus nullable Slot
restoration callbacks; both are now implemented rather than classified away.

Kotlin overloads, delegated properties, variance, data-class copy and experimental
annotations map to named Rust operations (`try_*` fallible helpers plus explicit
`*_or_panic` variants), public snapshot fields and Clone of Rc
handles. SimpleChildNavState maps to ChildNavState; Cancellation maps to
Subscription; factory-owner behavior is on ComponentContext.
Ref, hashString/keyHashString, Relay, locking and internal controllers
remain internal implementation details, not omitted public component facilities.
Public test StateKeeper register/consume/recreation conveniences map to Essenty
registration/consume_value and StateKeeper::{with_restored,from_snapshot}; byte
serialize/deserialize roundtrips remain caller-codec work rather than Kotlin
serializer syntax. Direct optional Rc WebNavigation replaces WebNavigationOwner;
None covers a fake component without child navigation.

No unresolved functional gap was found in the selected independent-core scope
against pinned revision `00840bcfed6d55710c8fbb1a72503f2041eda490` after these fixes.
Confidence is high for the explicit tested contracts, moderate for exhaustive
behavioral equivalence: the Kotlin suite was inspected but not run as a
differential oracle. This is not a claim that every upstream module or source
API spelling was ported.

Final fresh checks passed: **519 unit/integration tests and 1 doctest**, format,
strict Clippy/rustdoc, all 15 runnable examples and all 24 installed library
compile targets. Full commands/results are recorded in the
[completion plan](superpowers/plans/2026-10-10-independent-core-completion.md) and the
[explicit-API revision](adr/0019-explicit-rust-api-surface.md).
Cross compilation confirms library type-checking, not runtime behavior on those
OS/UI/browser hosts. Actual host bindings, Android/Jetpack, Compose, SwiftUI,
Reaktive, OS-main-thread checks, thread-safe mirrors and no_std remain outside
scope. Rust saved formats require explicit migration from Kotlin containers.

[upstream-tree]: https://github.com/arkivanov/Decompose/tree/00840bcfed6d55710c8fbb1a72503f2041eda490
[generic-context]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/GenericComponentContext.kt
[default-context]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/DefaultComponentContext.kt
[context-factory]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/ComponentContextFactory.kt
[context-ext]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/ComponentContextExt.kt
[merged-lifecycle]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/lifecycle/MergedLifecycle.kt
[child-state]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/statekeeper/ChildStateKeeper.kt
[child-instances]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/instancekeeper/ChildInstanceKeeper.kt
[child-back]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/backhandler/ChildBackHandler.kt
[stack-factory]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/stack/ChildStackFactory.kt
[stack-helpers]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/stack/StackNavigatorExt.kt
[slot-factory]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/slot/ChildSlotFactory.kt
[slot-helpers]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/slot/SlotNavigatorExt.kt
[pages-factory]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/pages/ChildPagesFactory.kt
[pages-helpers]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/pages/PagesNavigatorExt.kt
[panels-factory]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/panels/ChildPanelsFactory.kt
[panels-helpers]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/panels/PanelsNavigatorExt.kt
[items-factory]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/items/ChildItemsFactory.kt
[lazy-items]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/items/LazyChildItems.kt
[stack-navigation]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/stack/StackNavigation.kt
[navigation-source]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/children/NavigationSource.kt
[simple-navigation]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/children/SimpleNavigation.kt
[children-factory]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/children/ChildrenFactory.kt
[value]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/value/Value.kt
[mutable-value]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/value/MutableValueBuilder.kt
[mutable-ext]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/value/MutableValueExt.kt
[value-map]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/value/operator/Map.kt
[value-ext]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/value/ValueExt.kt
[state-saver]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/children/NavStateSaver.kt
[transient-saver]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/children/TransientNavStateSaver.kt
[settings]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/DecomposeSettings.kt
[test-context]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose-test-utils/src/commonMain/kotlin/com/arkivanov/decompose/testutils/TestComponentContext.kt
[web-navigation]: https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/webhistory/WebNavigation.kt
