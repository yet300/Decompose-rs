# Generic Navigation design

Status: implemented and verified; independent spec and quality reviews passed, 2026-10-10.
Continuation authorized by the user's repeated “commit and continue” within the independent core port.
Baseline: ChildItems commit b1a7d56; branch codex/generic-navigation.
Reference: upstream 00840bcfed6d55710c8fbb1a72503f2041eda490, router/children Kotlin APIs and tests.
Official overview: https://arkivanov.github.io/Decompose/navigation/children/overview/

## Decision and alternatives

Expose a public custom-model adapter to the existing private engine in the same decompose-rs crate.
A separate engine would duplicate lifecycle, retention, queue and rollback code. Making the internal
Model trait public would expose private persistence internals and family-specific implementation hooks.
A focused public adapter provides upstream capabilities through Rust ownership and explicit callbacks.

## Public contracts

Module router::children has the following API:
- ChildStatus: Created, Started, Resumed, Destroyed. Move the existing PageStatus definition to neutral
  types and retain pages::PageStatus as a public re-export alias.
- ChildItem<C,T>: Created(Child<C,T>) or Destroyed { configuration: Rc<C>, key: String }.
  Move existing PageChild implementation to neutral types; preserve pages::PageChild as an alias.
  Clone never requires C/T: Clone; configuration(), key(), instance() retain existing semantics.
- ChildNavState<C> { configuration: Rc<C>, status: ChildStatus }, manual Clone without C: Clone.
  new(C,status) owns a configuration. This concrete Rust record replaces the Kotlin interface plus
  SimpleChildNavState class; callers keep custom per-child metadata in their whole navigation state.
- NavState<C>: Clone + 'static, children(&self) -> Vec<ChildNavState<C>>. Implement for
  Vec<ChildNavState<C>> for simple cases. Caller-owned state/projections must be immutable and
  deterministic. C requires Eq, not Clone/Hash/Ord; T and E require no Clone.
- NavigationSource<E> object-safe trait:
  subscribe(&self, observer: Rc<dyn Fn(Rc<E>)>) -> Subscription.
  Existing public Subscription::from_cancel permits custom source implementations.
  SimpleNavigation<E> implements it; new/default/manual Clone, navigate(E), navigate_rc(Rc<E>).
  No replay; no receivers means discarded; synchronous receiver registration order; FIFO nested
  broadcasts after current event reaches every active receiver; short borrows; cancellation during
  dispatch honored; panic abandons queued events and propagates, source reusable. Public events are
  shared Rc handles, allowing event payloads without Clone.
- NavigationResult<N> { old_state: N, new_state: N }, Clone only N: Clone.
- ChildrenOptions<E,N>: key (default DefaultChildren), allow_duplicates (default false),
  codec: Option<NavStateCodec<N>>, on_state_changed: Option<Rc<dyn Fn(&N,Option<&N>)>>,
  on_event_complete: Option<Rc<dyn Fn(&E,Result<NavigationResult<N>,NavigationError>)>>,
  back_transformer: Option<Rc<dyn Fn(&N)->Option<Rc<dyn Fn()->N>>>>.
- children(context, source, initial_state, options, nav_transformer, state_mapper, child_factory)
  -> Result<Value<S>,NavigationError>, with Ctx: ComponentContext, C: Eq, N: NavState<C>,
  E/T/S: 'static. source accepts a custom or dyn NavigationSource<E>; initial_state is FnOnce()->N
  and only called on absent or explicitly declined restoration.
  nav_transformer: Fn(N,&E)->N; state_mapper: Fn(&N,Vec<ChildItem<C,T>>)->S;
  factory: Fn(&C,Ctx)->T.

Subscription must begin before initial-state factory, child factories and hooks; synchronously emitted
subscription/initialization events are buffered, then processed in order after initialization.
Use one receiver queue and weak event callbacks, with parent state provider owning runtime. No
additional public controller needed. Dropping Value or source handles does not terminate child scopes.
Parent destruction cancels subscription, removes Back/provider and ends children; held snapshots
may retain ended component Rc values (existing contract).

## Runtime

Support arbitrary ordered children including empty and all-destroyed states. Validate uniqueness
before factories unless allow_duplicates; duplicate matching is stable by equal occurrence like Pages.
All four statuses are parent bounded. Destroyed children own no live component/InstanceKeeper, retain
saved state and key; resurrection builds a fresh component and keeper under the same key. Removal
forgets both. Live equality reuse preserves the existing component and its configuration handle.
Refresh managed configuration to the current descriptor for inactive snapshots and future factories:
upstream resurrects using state.configuration, so an Eq-ignored payload update must reach a fresh
factory. Use the same opt-in canonicalization hook as Items; old families stay unchanged.
Custom contexts and non-Clone components work.
Forward factory/reverse outgoing teardown order, demotions before promotions, preparation/rollback,
retention and panic contracts remain shared.

on_state_changed runs initially with None after provider/Back registration, then after publication
on each successful event (including equal state and Back), before completion. on_event_complete
runs once per connected receiver for explicit events; reports errors, has original event identity;
Back has no event completion. Without on_event_complete, a rejected explicit event panics with its
navigation error, matching existing convenience operations and upstream; successful events need no
completion action. With the hook installed, it receives typed errors without an automatic panic.
A rejected automatic Back panics with its navigation error; there is no fabricated E completion.
A private model error hook can preserve previous families' handling while the generic adapter
reports arbitrary user-action rejection. A validation/save/preparation error leaves committed state untouched.
Reentrant source/factory/lifecycle/mapper/hook/completion events are FIFO and not processed mid-switch.
Panic propagates and abandons queued work; destructive lifecycle panic cannot promise undo and old
snapshots may reference ended children. Subsequent explicit navigation can repair ended scopes.

back_transformer is evaluated once at initialization and once per successful transition. Cache its
optional action and invoke that action on unhandled Back. Child Back has higher priority, only
Started/Resumed scopes participate; predictive cancellation changes no state. Back navigation uses
the same queue and is deferred until SharedBack dispatch borrow is released. No user callback under
RefCell borrow that prevents reentrant observation, save, source delivery or cancellation.

## Persistence

NavStateCodec<N>::new takes encode(&N)->Result<Option<Vec<u8>>,String> and
decode(&[u8])->Result<Option<N>,String>, stored Rc/manual Clone without N: Clone.
None codec means transient (no save providers queried); encode None skips navigation state and
child provider queries, matching upstream's saveState-before-saveChildState ordering. Encode the
whole-state payload exactly once per save before collecting child bytes. Existing families keep
their save order; a private default lazy-child-save hook can support this custom adapter.
decode None explicitly declines a structurally valid frame: run initial factory and discard saved
children/retention. Decoder errors propagate; never silently fall back.
Use DCH1 frame: magic[4], next_key:u64 LE, payload:u32 length+bytes, child_count:u32 LE,
then each child key:u32 length UTF-8 and DCS1 state:u32 length+bytes. Statuses/metadata belong to
user N payload; restored statuses may differ. Validate the entire envelope including keys canonical
child/n, unique and n<counter, nested framing, bounds and trailing bytes BEFORE user decoder.
Check restored N child count and uniqueness BEFORE factories. Caller codec must preserve child
configuration identity/order; inability to infer arbitrary configuration serialization is documented.
Persist all live/destroyed children; snapshot before providers for coherent reentrant save.
Preserve DST1/DSL1/DPG1/DPN1/DPI1 framing and behavior.

## Boundaries and verification

No dependencies, new crates, UI/platform/web history integrations, Send/Sync bounds or Essenty changes.
Source/types/runtime/persistence files have separate responsibilities; minimal shared engine hook for
custom event subscription + lazy initial factory; don't duplicate a lifecycle engine or refactor
unrelated modules. Existing public Pages aliases remain source compatible.

TDD integration cases cover custom state metadata/mapper/event non-Clone, custom synchronous source,
initial factory buffering/restore skip, empty/all four statuses, stable keys/recreation/state/retention,
duplicate policy and occurrence matching, observer-hook-completion order/reentrancy/errors/panics,
conditional cached Back and predictive cancel/child priority/deferred navigation, transient/codec
skip+decline+errors, valid and malformed DCH1 predecoder guards and rollback, custom contexts/drop/
parent ownership. Run all baseline tests plus new tests, doctest, fmt, strict Clippy, warning-free
rustdoc, six examples and installed target matrix. Fresh independent spec then quality review.
