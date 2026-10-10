# Standard-router extension points

Status: implemented and verified; independent spec and quality reviews passed.
Baseline/main 9a70ae0; branch
codex/router-extension-points. The user approved this next stage after completing
observable values and merging accumulated work into main. Both prerequisites
are complete: observable commit 9a70ae0, fast-forward main, 389 tests + 1 doctest
passed again on main. No remote is configured.

Reference: pinned Decompose 00840bc, standard family Navigation/Navigator interfaces
and factories. The Rust adaptation keeps concrete Navigation constructors and
adds composable traits, rather than replacing existing structs with interfaces.

## Architecture decision

Reuse the public object-safe router::children::NavigationSource<E> registration
protocol and SimpleNavigation<E> broadcast transport. Add typed standard events,
generalize standard factories, and expose generic navigator traits. This is the
selected approach: it makes wrappers, interceptors and custom sources usable
without reimplementing lifecycle policies. Separate source traits per family
would duplicate registration/cancellation machinery; using only the generic
children factory would make callers recreate standard policies unnecessarily.

One crate, existing dependencies, synchronous Rc ownership. No runtime, serde,
platform/Web integration, Essenty changes, mandatory Send/Sync, configuration
Clone/Hash bounds, or new observable abstraction. Keep standard codecs and model
policies. Lazy initial-state closures are a separate later stage.

## Public events and source connection

Add router::NavigationEvent<S, R>, an immutable event whose callbacks are shared
without S/R Clone bounds. Provide:

```rust
pub fn new(transformer: impl Fn(S) -> S + 'static) -> Self;
pub fn with_completion(
    transformer: impl Fn(S) -> S + 'static,
    completion: impl Fn(Result<R, NavigationError>) + 'static,
) -> Self;
pub fn transform(&self, state: S) -> S;
pub fn complete(&self, result: Result<R, NavigationError>);
```

new uses a no-op completion, matching upstream Event's default. Existing
navigate convenience methods still panic on transition errors; navigate_with
still reports typed errors. Public event Clone shares callbacks, not payloads.
No new mutable public event fields. Completion is per connected receiver after
publication; rejected transitions report the typed error and preserve state.

Export family aliases alongside each NavigationResult:

- stack::StackEvent<C>: Vec<Rc<C>> -> stack::NavigationResult<C>.
- slot::SlotEvent<C>: Option<Rc<C>> -> slot::NavigationResult<C>.
- pages::PagesEvent<C>: Pages<C> -> pages::NavigationResult<C>.
- panels::PanelsEvent<MC, DC, EC = Infallible>: Panels<...> -> panels result.
- items::ItemsEvent<C>: Items<C> -> items::NavigationResult<C>.

All five concrete Navigation types implement NavigationSource<their event>.
Standard child_stack/slot/pages/panels/items factories accept
&(impl NavigationSource<FamilyEvent> + ?Sized); child_panels_without_extra does
likewise for EC=Infallible. Preserve named generic parameter counts, existing
inherent Navigation APIs, constructor names, inference, and callers that never
import a navigator trait. Factories need only subscription, not a navigator.
The passed source need not be Clone or itself 'static: only its observer and
returned Subscription own their registrations. dyn NavigationSource is supported.

## Navigator traits

Add StackNavigator<C>, SlotNavigator<C>, PagesNavigator<C>,
PanelsNavigator<MC, DC, EC = Infallible>. Match the existing ItemsNavigator pattern:
navigate_with is the one required generic callback method; navigate and all
existing family helpers are provided methods. Existing concrete inherent helpers
delegate to these methods, so custom implementations inherit identical behavior.
Keep method-specific Eq bounds where needed, rather than imposing them on all
navigation. Preserve ItemsNavigator's required signature and LazyChildItems
receiver-local operations. These generic navigator traits use static dispatch;
the source protocol is object-safe. Do not claim dyn navigator support.

## Transport and engine boundary

Use the existing SimpleNavigation broadcast for standard sources too. Avoid a
second public source protocol or duplicate FIFO loop. Shared Rc events preserve
identity across connected public receivers; separate receivers apply callbacks
to their own states and invoke completion separately. Disconnected events are
discarded; registration order, nested FIFO delivery, cancellation during an
already captured round, panic queue clearing and recovery remain covered.

Adapt public event snapshots/results into private engine Event at construction,
using the engine's existing build_with_subscriber setup and cleanup guards. Slot
adapts optional configuration to private Vec state at this boundary. Preserve
typed Panels roles and existing lazy Items control. Buffer emissions during
subscribe, factories and restoration until the initial publication is complete;
drain through the normal engine queue. Construction error/panic cancels the
source token, unregisters providers/Back, destroys new scopes, and restores
consumed saved input according to existing Generic Navigation guarantees. An
arbitrary custom source is responsible for registrations it creates before its
subscribe returns if that method itself panics without yielding a token.

Parent destruction cancels custom-source observation. Back remains a receiver
operation and must not require broadcasting another event through the source.
No public source reference is retained after subscription; no user callback or
capture/payload Drop runs under an obstructing new borrow.

## Acceptance

Existing 389 tests + doctest continue passing. Test public non-Clone events and
Rc identity/multiple receivers/FIFO/no replay/cancel/panic recovery. Test generic
custom navigator helpers and old inherent APIs, including ItemsNavigator custom
implementations and local lazy Items. Connect custom sources and dyn sources to
all five families and two-role panels, non-Clone configurations/components,
subscription-time events, per-receiver completion, invalid transition errors,
Back, teardown and constructor failure/panic rollback. Preserve all old saved
format fixture bytes and retention/lifecycle behavior.

One source owner implements all connected changes; controller owns docs/example.
Independent spec review precedes whole-stage quality review. Controller runs full
test/doc/fmt/strict Clippy/rustdoc, eight examples and the installed target matrix.
The completed stage stays on its new branch unless a subsequent user request
authorizes its merge. No remote push or branch deletion.
