# ChildPages Rust design

Status: authorized by the user's continuation after ChildSlot; implemented and
independently reviewed in `codex/child-pages`.

## Scope and reference

Implement independent ChildPages navigation in the existing `decompose-rs` crate, above Rust Essenty. No UI, browser, Android, SwiftUI or reactive-library adapters. Reference: ignored local `upstream-ref` at `00840bcfed6d55710c8fbb1a72503f2041eda490`, particularly `router/pages` factories, operations and tests. Existing ComponentContext, ChildStack and ChildSlot APIs and DST1/DSL1 persistence must continue working.

## Public contract

`Pages<C>` contains `items: Vec<Rc<C>>` and `selected_index: Option<usize>`. Empty pages use `None`; nonempty pages require an in-range selection. `Pages::new(Vec<C>, usize)` is fallible and normalizes empty input to `None`; `Default` is empty. Manual Clone implementations avoid `C: Clone` or `T: Clone` bounds. Configuration matching uses equality and occurrence order; duplicates are rejected unless explicitly enabled.

`ChildPages<C,T>` exposes items and selection. `PageChild<C,T>` distinguishes `Created(Child<C,T>)` from `Destroyed { configuration: Rc<C>, key: String }`, with configuration/key/optional-instance accessors. Preserve the existing public `Child` struct for Stack and Slot.

`PagesNavigation` supplies `navigate`, `navigate_with`, `select`, `select_with`, `select_next(circular)`, `select_prev(circular)`, `select_first`, `select_last`, `clear`, and `set_items`. Completion results contain old/new Pages state for each receiver. Invalid selection is a typed navigation error in fallible completion paths; convenience methods follow the existing panic-on-error convention. Selection of an empty collection is a no-op. Noncircular movement clamps at the boundary. `set_items` clamps the old numeric index to the new range; it does not track selected configuration identity. Empty output always selects None.

`PagesOptions` contains the parent key (default ChildPages), optional configuration-list codec, handle_back_button (false), allow_duplicates (false), and optional page_status callback. Public `PageStatus` has Created, Started, Resumed and Destroyed. Export the default policy: selected page Resumed, immediate neighbors Created, all other pages Destroyed. The callback permits other policies including circular adjacency. Recompute statuses after navigation and restoration.

`child_pages` returns `Result<Value<ChildPages<C,T>>, NavigationError>`, using the same context factory and ownership conventions as the existing routers. No Clone, Send or Sync requirement on configurations or component instances.

## Lifecycle and ownership

Each page has a stable generated key. Destroyed pages retain configuration, key and saved child state, but have no live component or keeper. Only materialize pages whose desired status is live. Going live -> Destroyed saves state before ending the child scope and its retained instances; returning reconstructs a new instance and keeper with the same key and saved state. Reorders retain occurrence-matched identities. Removed pages release all scope resources. Old observable snapshots may retain component objects, but their lifecycles are ended.

Live statuses are bounded by the parent lifecycle. Child Back handlers work only when the target status and merged parent lifecycle allow Started/Resumed. Automatic Back is enabled for a nonempty collection with selected index > 0, and selects the preceding page. Preserve deferred processing during Back dispatch, cancellation cleanup and navigation FIFO during reentrant factory/lifecycle/value/completion callbacks.

Prepare new live scopes and save prospective tombstones before destructive transitions so typed failures preserve the current state. Panic guards restore queue usability and release prepared resources; arbitrary user callback side effects cannot be rolled back. Never reuse a live scope whose local lifecycle or keeper has ended after a panic. Deliberate tombstones remain reusable identities.

External parent InstanceKeeper retains desired live child scopes across context recreation. A desired destroyed page must not keep a retained live scope. Transient routers discard obsolete retained navigation. Internal ownership must have no Rc cycles; dropping a Value alone must not tear down a context-owned router.

## Shared implementation

Generalize private navigation events and engine policy to carry actual state (including selected index), rather than hiding selection in configurations or a mutable side channel. Model policy determines configurations, statuses, snapshots, Back and serialization. Generalize managed items to live scopes/tombstones. Stack and Slot wrappers adapt their existing public results; all existing source, queue, lifetime, state and panic tests remain applicable. Extract private helpers if this improves clarity; do not duplicate an entire engine for Pages or split the crate.

## Persistence

Provide a user list codec, matching Stack's ownership conventions. Add a separate versioned DPG1 envelope carrying configurations, selected index, stable keys, per-child state and monotonic next-key counter. Empty Pages state is authoritative over initial nonempty state. Validate framing, lengths, selection, count, canonical key/counter consistency, duplicates and nested state before component factories. Codec failures are typed errors; constructor failures return restored input for a retry. Reserve provider keys before user callbacks. Preserve DST1 and DSL1 byte compatibility with fixed fixtures.

Take one coherent state/key/counter snapshot before saving live child providers, which may navigate. Tombstones contribute stored state directly. Without a codec do not call providers merely to save a transient router. Recompute live statuses from the current policy after restore; do not persist status values.

## Acceptance

Cover empty/boundary/circular navigation, numeric index clamping, duplicates, default lazy construction, custom statuses, stable identity and state across destruction/resurrection/reorder, parent-bounded lifecycle ordering, child and automatic Back, reentrancy, failure/panic cleanup and retry, malformed persistence, external retention, mixed/nested routers and non-Clone types. Keep all 181 baseline tests passing. Deliver runnable ChildPages example, README/roadmap and an ADR. Finish with independent spec review, then quality review, cargo test for all targets and doc tests, fmt, strict Clippy and rustdoc, example execution and existing target compilation coverage.
