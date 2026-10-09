# ChildItems Rust design

Status: implemented; independent specification and quality reviews passed.
Authorized by the user's instruction to commit ChildPanels and continue
with the proposed ChildItems stage. Existing scope: one crate above published
Rust Essenty, no UI/platform adapters or extra runtime dependencies.
Baseline: 0336a19, 263 passing tests including doctests.
Reference: pinned Decompose 00840bcfed6d55710c8fbb1a72503f2041eda490,
router/items production code and lazy/navigation/state/retention/Back tests.

## Architecture and alternatives

Reuse the existing private Model/transaction/scope engine with Destroyed entries.
A separate Items controller would duplicate lifecycle, retained scope, queue and
rollback invariants. Implementing Items through a public Pages wrapper would
expose irrelevant selection semantics and cannot provide receiver-local lazy
activation correctly. A small private weak receiver control extends the engine;
all existing builders keep their Value return and behavior. A default identity
normalization hook applies Items pruning/canonicalization before validation;
other models leave state unchanged.

Items membership is ordered and unique by equality, always including inactive
configurations. Active membership is keyed by configuration, not index: reorders
preserve activity and instance identity. Use a small module-local public
ItemMap<C, V> backed by unique Rc-key/value entries. This keeps Eq-only configs,
without imposing Hash/Ord/Clone or a third-party map dependency. Lookups are
linear; document the tradeoff. It has new/default, len/is_empty, iter/keys/values,
get/contains_key, insert/remove/retain/clear, from_owned and FromIterator for
(Rc<C>, V). Insert replaces equal keys, like a map. retain receives (&C, &mut V), allowing
value updates while filtering, as in standard Rust maps. Clone requires only V: Clone.

## Public API

router::items exports Items<C> with public items: Vec<Rc<C>> and
active_items: ItemMap<C, ActiveLifecycleState>. ActiveLifecycleState is Created,
Started or Resumed, Copy/Eq. Absent configurations are Destroyed. Items::new(Vec<C>)
returns Result and rejects duplicate membership; Default is empty. Manual Clone
never requires configuration Clone. Callers may transform public fields.

ChildItems<C,T> contains ordered configuration handles and active_items:
ItemMap<C, ActiveChild<C,T>>. ActiveChild contains child: Child<C,T> and
lifecycle_state: ActiveLifecycleState (the requested, parent-bounded target).
Snapshots expose only live components in the active map. Manual Clone requires
neither C nor T: Clone. Requested state remains visible if parent bounds actual
lifecycle. Equality matches config values; active snapshots use the canonical
membership Rc handle. Deactivation ends the previous scope despite old snapshots.

ItemsNavigation<C> is a shared synchronous source with manual Clone/default/new.
NavigationResult<C> has old_items/new_items. Public ItemsNavigator<C> provides
navigate/_with plus transformer-based set_items/_with over Vec<Rc<C>> and
set_active_items/_with over ItemMap<C,ActiveLifecycleState>. Both source and lazy
handle implement the trait and expose equivalent inherent convenience methods.
Completion receives Result once per connected receiver for the source, once for
local lazy navigation. Convenience failures panic, explicit completion receives
errors. Helpers capture transformers once and are reusable for broadcasts.

child_items(context, navigation, initial: Items, options: ItemsOptions, factory)
returns Result<LazyChildItems<C,T>, NavigationError>. Options contain hosting key
(DefaultChildItems) and optional ItemsCodec<C>; there is no allow_duplicates,
automatic Back setting, page-status policy or state-change hook.

LazyChildItems<C,T> is Clone without C/T bounds, with get() snapshot,
value() observable Value, subscribe(observer), receiver-local navigation helpers
and get_or_create(&C)->Result<Rc<T>, NavigationError>. Its control holds only a
Weak engine, so views do not extend navigation ownership. The parent owns it.
get_or_create returns an already active live component preserving its requested
state. Check actual scope availability through weak control, not just the
published snapshot: destructive lifecycle panics can leave a snapshot referring
to an ended scope. Such active entries return ChildUnavailable without queuing
work, as do scopes whose keeper cannot be established as available. Snapshots
may still expose ended instances by design; the lazy getter must not return them.
For an inactive member it synchronously activates Created and returns the live
component. It never adds absent membership. UnknownConfiguration is returned
for an absent config; NavigationInProgress for an inactive fetch while engine
construction/navigation or Back dispatch is in progress; ChildUnavailable if
queued factory/lifecycle/observer work removed/deactivated the requested child
before the synchronous fetch finished. The final synchronous lookup also verifies
live scope availability. An inactive busy fetch queues no activation
side effect. Existing active reads during navigation are allowed. Destroyed
handles return Destroyed, including when an old snapshot still has the component.
This makes upstream's illegal recursive lazy fetch a recoverable Rust error.

## Membership, lifecycle, queues and Back

Duplicates are always rejected before factories, both initially and in navigation.
For transformations and construction, prune active entries absent from membership
and canonicalize equal map keys to membership handles. Upstream prunes such keys
on list changes and leaves other invalid membership undefined; Rust consistently
normalizes them rather than maintaining inaccessible live scopes. The saved
format never contains an active index outside membership.

Only active entries invoke factories. Inactive entries retain key and saved child
state, with no component/InstanceKeeper. Activity changes between live targets
retain component/key/keeper; deactivation saves state then destroys the scope and
retained services. Reactivation creates fresh component/keeper under the same key
with saved state. Removal discards state/key; later re-add uses a fresh key/state.
Equal membership replacement preserves a currently live instance. If equality
ignores additional configuration fields, a later fresh activation receives the
current member data rather than the obsolete equal configuration. This follows
pinned ChildController activation of a destroyed entry. Existing models keep
their configuration-handle behavior through a default private engine policy.
Factories follow membership order, destruction is reverse order; demotions precede
promotions. Parent lifecycle bounds each live entry.

Use the existing FIFO engine queue for source and receiver-local navigation.
Factories, lifecycle, observers/completions and Back may enqueue recursive calls.
Callbacks run outside navigation-state RefCell borrows. Panics propagate, clear
abandoned queued work, and allow usable retry. Existing prepare-before-destruct,
reservations, provisional-scope cleanup and ended-scope recovery remain intact.
No-op events publish and complete. Local lazy navigation never broadcasts to
other receivers sharing ItemsNavigation. A dropped source does not disable lazy
navigation, and dropped views do not stop the parent-owned router.

There is no automatic Back operation. Child bridges are enabled only for effective
Started/Resumed children, bounded by parent lifecycle. Created/inactive children
do not intercept Back. Preserve priority/LIFO, predictive cancellation, dispatch
borrowing/deferred navigation and panic recovery. Membership is unchanged by
unhandled Back.

## Persistence

ItemsCodec<C> uses ordered configuration-list callbacks like Pages/Stack:
encode(&[Rc<C>])->Result<Vec<u8>,String>, decode(&[u8])->Result<Vec<C>,String>.
Manual Clone has no C bound. Active lifecycle targets are library metadata.
DPI1 saves the next-key counter, list payload, item count and each item's target
status (0 Destroyed, 1 Created, 2 Started, 3 Resumed), stable key and DCS1 state.
Concrete frame: magic[4], counter u64LE, config-payload u32 length+bytes, child
count u32LE, then per child status u8, key u32 length+UTF8 bytes, DCS1 u32
length+bytes. Structural framing/status/key uniqueness/canonical identity/counter,
all nested DCS1 and trailing bytes validate BEFORE invoking the config codec.
Decoded config count and uniqueness validate before any component factory.
Restored order/active targets replace initial state, including authoritative empty
and all-inactive lists. Both live and tombstone state persists over repeated saves.

Without a codec navigation is transient: parent saves skip child providers and
recreation uses initial state, discarding prior retained scopes. External retained
services survive parent lifecycle recreation for restored active entries; inactive
entries never retain an old keeper. Key reservation, keeper-type conflict,
retryable original input, codec failures and coherent save snapshotting before
provider reentry use the existing engine. Existing DST1/DSL1/DPG1/DPN1 bytes and
public model APIs remain unchanged.

## Acceptance

Test empty/all-inactive construction with zero factories; non-Clone service-bearing
custom contexts/components/configs, Eq-only map and exact three-target lifecycles;
identity across reorder/status changes; fresh factories receive updated non-Eq
payload after equal membership replacement; destruction/state/keeper recreation;
removal/readd discards state; duplicate rejection; active key pruning; navigation
helpers, shared per-receiver completions, receiver-local isolation; lazy lookup,
busy recursive fetch, self-removal, active fetch during navigation, dead handles,
dropped source/views; initial/factory/lifecycle/observer/completion panic recovery; immediate lazy
fetch after a caught destructive lifecycle panic rejects the stale ended entry;
child Back Created gating/Started/Resumed, parent limits, predictive cancellation
and navigation deferral; live/inactive/empty persistence, strict DPI1 malformed
fields, pre-codec structural checks, decode count/duplicate/errors/save failure,
reservations/type conflict/input rollback, external retention/transient reset,
coherent save under provider navigation and nested/mixed routers. Preserve all263
baseline tests and immutable old-format fixtures. Provide README/example/ADR/
roadmap and independent spec then quality reviews. Verify strict fmt/Clippy/
rustdoc, all-target/doc tests, five runnable examples and 24 installed compile
platforms. Do not add integrations or make Essenty source changes.
