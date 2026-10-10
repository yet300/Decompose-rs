# Decompose compatibility matrix

Audit date: 2026-10-10. Reference: upstream Decompose
[`00840bcfed6d55710c8fbb1a72503f2041eda490`](https://github.com/arkivanov/Decompose/tree/00840bcfed6d55710c8fbb1a72503f2041eda490).
Rust baseline: `e129ab1`, with this stage developed on
`codex/lazy-initial-state-compatibility`. Dependencies are the four published
`essenty-* 0.1.0` crates and `thiserror 2`.

This is a behavioral/API mapping for the independent Rust core, not a promise
of Kotlin source or serialization compatibility. Links below use the pinned
revision. Source inspection and Rust regression tests support the conclusions;
the Kotlin test suite has not been executed as a differential oracle.

Status meanings: **Implemented** means the stated behavior is present;
**Adapted** means it is present with the explicitly described Rust contract;
**Gap** identifies an absent upstream capability; **Outside scope** identifies
an excluded integration. Verification results and their practical limits are
recorded below and in the stage plan.

## Contexts, ownership and Back

| Upstream contract | Rust mapping and status | Evidence |
|---|---|---|
| GenericComponentContext, ComponentContext, factory owner | **Adapted:** one `ComponentContext: Sized` trait, four handles and `context_factory()`. Custom contexts propagate services without Clone. | [upstream generic context][generic-context]; [Rust context](../src/context.rs), [context regressions](../tests/component_context.rs), [nested routers](../tests/child_stack.rs) |
| DefaultComponentContext | **Implemented:** default and supplied state/instance/Back handles; a default-owned instance keeper attaches to lifecycle destruction. | [upstream default][default-context]; [Rust context](../src/context.rs), its unit tests |
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
| Stack helper operations | **Adapted:** push/push_new/push_to_front, pop/pop_while/pop_to/pop_to_first, replace_current/replace_all. `bring_to_front_by` takes an explicit group selector instead of Kotlin runtime class. Stack helper completion is available through general `navigate_with`; helper-specific Kotlin callback overloads are not copied. | [upstream Stack helpers][stack-helpers]; [Rust helpers](../src/router/stack/navigation.rs), module unit tests and [stack tests](../tests/child_stack.rs) |
| ChildSlot and activate/dismiss | **Implemented:** Option membership, equal-child reuse, replacement/destruction, empty no-op and optional automatic dismissal on Back. `_with` helpers report typed results. | [upstream Slot factory][slot-factory], [helpers][slot-helpers]; [slot module](../src/router/slot/mod.rs), [navigation](../src/router/slot/navigation.rs), [tests](../tests/child_slot.rs) |
| ChildPages, statuses and navigation | **Adapted:** configurable four-state lifecycle policy, live/destroyed records and state-preserving resurrection; select/next/prev/first/last/clear/set_items. Empty selection is `None` rather than Kotlin -1. | [upstream Pages factory][pages-factory], [helpers][pages-helpers]; [pages module](../src/router/pages/mod.rs), [navigation](../src/router/pages/navigation.rs), [tests](../tests/child_pages.rs) |
| ChildPanels: typed roles and Single/Dual/Triple | **Implemented:** three-role and no-extra adapters, typed factories, mode lifecycles, activation/dismissal/pop/mode changes and Single-only automatic Back. Kotlin navigate overloads map to named set_panels/set_details_extra/set_extra methods. | [upstream Panels factory][panels-factory], [helpers][panels-helpers]; [panels module](../src/router/panels/mod.rs), [navigation](../src/router/panels/navigation.rs), [tests](../tests/child_panels.rs) |
| ChildItems and LazyChildItems | **Adapted:** ordered unique Eq configurations, activity map and receiver-local helpers; `get_or_create(&C)` explicitly reports errors and preserves live identity. Linear `ItemMap` avoids mandatory Hash/Ord. No automatic model Back action. | [upstream Items factory][items-factory], [lazy interface][lazy-items]; [items module](../src/router/items/mod.rs), [map](../src/router/items/map.rs), [tests](../tests/child_items.rs) |
| Lazy initial state of all five families | **Adapted:** six `_with_initial` functions accept borrowed/owned FnOnce, including Panels without extra. Evaluation is synchronous during construction, zero times with valid restoration and once on the no-restoration path. Eager calls remain. | [upstream factories][stack-factory] / [Slot][slot-factory] / [Pages][pages-factory] / [Panels][panels-factory] / [Items][items-factory]; standard factory modules, [lazy regression suite](../tests/lazy_initial_state.rs), [example](../examples/lazy_initial_state.rs) |
| Navigation/Navigator event interfaces | **Adapted:** concrete constructors plus five generic navigator traits, public family event aliases and `NavigationEvent<S,R>`. Sources are object-safe; navigator traits use static dispatch for generic callbacks. | [upstream Stack source][stack-navigation]; [public event](../src/router/navigation.rs), each family navigation module, [extension tests](../tests/router_extension_points.rs) |
| Custom sources in standard factories | **Implemented:** every factory accepts borrowed/dyn `NavigationSource<FamilyEvent>`, allowing wrappers, interception or direct emission while keeping standard model policies. | [upstream source][navigation-source], all factory links above; [extension tests](../tests/router_extension_points.rs), [example](../examples/router_extension_points.rs) |
| Event delivery and completion | **Adapted:** shared Rc event identity, no replay, nested FIFO, receiver-specific state/results and post-publication completion. `NavigationEvent::new` is no-op completion; navigator conveniences panic on errors and explicit completion receives Result. | [upstream SimpleNavigation][simple-navigation]; [transport](../src/router/children/navigation.rs), [adapter](../src/router/navigation.rs), [extension tests](../tests/router_extension_points.rs) |
| Construction and teardown | **Implemented with documented boundaries:** initialization events are buffered; source tokens are cancelled on rollback/Destroy. Custom subscribe owns cleanup before returning its token. Parent owns live navigation even after view/source handles are dropped. | [engine](../src/router/engine.rs), [extension regressions](../tests/router_extension_points.rs), [ADR-0012](adr/0012-standard-router-extension-points.md) |

## Generic Navigation and observable values

| Upstream contract | Rust mapping and status | Evidence |
|---|---|---|
| children with arbitrary NavState | **Adapted:** immutable Clone whole-state, Eq-only configuration descriptors, arbitrary observable projection and four lifecycle statuses. Internal upstream navigator/controller classes remain private engine machinery. | [upstream children][children-factory]; [public adapter](../src/router/children/mod.rs), [types](../src/router/children/types.rs), [tests](../tests/generic_navigation.rs) |
| Generic hooks, conditional Back and lazy initial state | **Implemented:** initial state-change hook, per-event hook/errors, cached optional Back action, FnOnce initial state and initialization FIFO. | [upstream children][children-factory]; [generic tests](../tests/generic_navigation.rs), [example](../examples/generic_navigation.rs) |
| Value/MutableValue, immediate replay | **Adapted:** `Value<T>` is a read-only Rc snapshot view, `MutableValue<T>` owns writes; no payload Clone/Eq. Subscription is a RAII token. Reentrant writes conflate to latest snapshot after the current round. | [upstream Value][value], [MutableValue implementation][mutable-value]; [Rust value](../src/value.rs), unit tests and [observable regressions](../tests/observable_values.rs) |
| compareAndSet/update/updateAndGet/getAndUpdate | **Implemented for the synchronous Rust contract:** identity CAS uses Rc::ptr_eq; FnMut transforms retry stale reentrant snapshots and return exact committed identities. No multithreaded atomicity guarantee. | [upstream mutable extensions][mutable-ext]; [Rust value](../src/value.rs), [observable tests](../tests/observable_values.rs) |
| Value.map | **Implemented:** eager initial mapping, identity cache, shared derived snapshots, chained reads and panic/reentry recovery. Recursive evaluation of the same uncached input has a documented panic guard. | [upstream Map][value-map]; [mapped value](../src/value/mapped.rs), [observable tests](../tests/observable_values.rs) |
| Lifecycle-bound observation | **Adapted:** CreateDestroy/StartStop/ResumePause gates and latest replay on reopening. Token Drop cancels both registrations; detach lasts until explicit Destroy, retaining an Essenty lifecycle witness. | [upstream lifecycle subscription][value-ext]; [Rust binding](../src/value/lifecycle.rs), [observable tests](../tests/observable_values.rs), [ADR-0011](adr/0011-observable-values-completion.md) |
| Kotlin delegated-property Value access | **Adapted:** explicit `get()` returns Rc; Kotlin property-delegate syntax has no direct Rust API counterpart. | [upstream Value extensions][value-ext]; [Rust value](../src/value.rs) |

## Persistence, adaptations and remaining gaps

| Upstream capability | Rust status and practical consequence | Evidence |
|---|---|---|
| Configuration/state serializers | **Adapted:** caller-supplied codecs to bytes, no mandatory serde or Kotlin serialization dependency. Standard family metadata belongs to the library; Generic codec saves whole state. | [upstream NavStateSaver][state-saver]; standard codec modules and [generic codec](../src/router/children/state.rs) |
| Keys, nested child state and retained recreation | **Implemented:** stable identities/counter, strict framing, inactive child state, saved authoritative membership and guarded retries. | [stack](../tests/child_stack.rs), [slot](../tests/child_slot.rs), [pages](../tests/child_pages.rs), [panels](../tests/child_panels.rs), [items](../tests/child_items.rs), [generic tests](../tests/generic_navigation.rs) |
| Serialized bytes across languages | **Gap:** Rust DCS1/DST1/DSL1/DPG1/DPN1/DPI1/DCH1 are local versioned formats, not Kotlin SerializableContainer bytes. Existing Rust fixtures remain stable; moving Kotlin saved data requires an explicit migration. | [framing](../src/framing.rs), all router state.rs modules, [fixture regressions](../tests/child_pages.rs) |
| Declining valid saved state / skipping a particular save | **Partially adapted:** Generic NavStateCodec supports encode/decode None. Standard codecs do not expose equivalent per-save/per-restore decline; absent codec selects transient mode and decode errors propagate. Upstream Slot restoreConfiguration returning null means empty membership, not decline; Rust records empty Slot membership separately. | [upstream Stack callbacks][stack-factory] / [Pages][pages-factory] / [Panels][panels-factory] / [Slot][slot-factory], [NavStateSaver used by Items][state-saver]; [generic tests](../tests/generic_navigation.rs), standard public codec definitions |
| transientNavStateSaver: retain in memory but not on disk | **Gap:** no matching memory-only container/saver. `codec: None` means reset on recreation and is not equivalent to upstream's memory-only preservation. | [upstream transient saver][transient-saver]; [ADR-0009](adr/0009-lazy-child-items.md), [ADR-0010](adr/0010-public-generic-navigation.md) |
| DecomposeSettings duplicate policy | **Adapted:** per-router allow_duplicates for Stack/Pages/Generic; Items always rejects duplicates. No global mutable settings object. | [upstream settings][settings]; corresponding options/validation and [generic occurrence tests](../tests/generic_navigation.rs) |
| DecomposeSettings error handler and main-thread logging | **Gap/adaptation:** no global onDecomposeError callback or platform-main-thread check. The core is Rc-based single-threaded; fallible operations use Result and user panics propagate. This does not bind a router to an OS/UI main thread. | [upstream settings][settings]; [Rust errors](../src/error.rs), [navigation errors](../src/router/mod.rs), [ADR-0001](adr/0001-component-context-model.md) |
| Public decompose-test-utils | **Outside current delivery:** crate-internal TestContext supports tests; it is cfg(test)/private and no public test-utils crate is supplied. | [upstream TestComponentContext][test-context]; [internal harness](../src/testing.rs), [exports](../src/lib.rs) |
| WebNavigation / browser history | **Outside scope:** no browser history controller, URL serializer or host history integration. | [upstream WebNavigation][web-navigation]; [exports](../src/lib.rs), [roadmap](PORTING_ROADMAP.md) |
| Android/Jetpack/Compose/SwiftUI/Reaktive and host adapters | **Outside scope:** core compiles for applicable targets but supplies no UI framework adapters or automatic host lifecycle/gesture wiring. No dependency integrations are ported just to copy upstream modules. | [pinned upstream project tree][upstream-tree]; [Cargo.toml](../Cargo.toml), [exports](../src/lib.rs) |
| Thread safety, concurrency and no_std | **Outside the selected Rust architecture:** synchronous Rc/RefCell core, no Send/Sync promise, runtime or no_std support. A separate concurrent mirror would require its own design. | [Cargo.toml](../Cargo.toml), [Rust value](../src/value.rs), [architecture decisions](PORTING_ROADMAP.md) |

## Verification boundary

The controller independently confirmed **428 unit/integration tests and 1
doctest**, formatting, strict Clippy/rustdoc and all nine runnable examples.
The lazy suite adds 18 tests to the 410-test baseline. Library compilation passed
all 24 installed targets; full target names, commands and independent review
results are recorded in the [implementation plan](superpowers/plans/2026-10-10-lazy-initial-state-compatibility.md).
Cross-compilation confirms library type-checking for targets,
not runtime behavior on those operating systems or UI hosts.

Independent whole-stage specification and quality reviews both passed without
findings, with high confidence for the bounded Rust contracts. They independently
confirmed focused/full tests, doctest and the restoration example, and inspected
the actual implementation and compatibility evidence. This stage is delivered
on codex/lazy-initial-state-compatibility; main remains at the e129ab1 baseline.

The covered portable behavior includes the component model, five standard
routers, custom navigation, values, Back and explicit-codec restoration. The
gaps above prevent a claim that every upstream API/module has been ported. No
percentage is assigned: counting overloads, internal helpers and integrations
as equal units would obscure practical compatibility.

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
