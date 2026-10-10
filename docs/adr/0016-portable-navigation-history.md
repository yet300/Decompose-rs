# ADR-0016: Browser-independent navigation history

Status: Accepted. Reference Decompose revision:
`00840bcfed6d55710c8fbb1a72503f2041eda490`.

## Source contracts

CommonMain `router/webhistory/WebNavigation.kt` defines typed keys, observable
nonempty history, URL path/query contributions, optional child navigation,
a browser guard and restoration. Stack, Pages and Panels supply model bridges.
The portable synchronization algorithm is in webMain
`router/webhistory/WebHistoryNavigation.kt`, mixed with the concrete browser
adapter. We port its model/history behavior and leave DOM/JS/wasm wiring to hosts.

CommonMain also exposes the older stack-only `WebHistoryController`. Its webMain
`DefaultWebHistoryController` implements different initial-entry and suffix
replacement rules, a desired/current stack guard, stored path access before
attachment and Forward path-to-configuration reconstruction. Those are portable
behaviors, not a browser binding, and are implemented separately.

## Decision

The existing crate exposes `router::webhistory`. `WebNavigation` is object-safe
for heterogeneous recursive trees. `TypedWebNavigation<K>`, `Route<K>` and
`RouteCodec<K>` retain typed application keys internally and encode only at the
host boundary. Rc sharing imposes no configuration Clone/Hash/Send/Sync bounds.
Stored observers and callback captures require `'static` as in other router
bindings. `HistoryItem.child` directly holds an optional Rc WebNavigation;
this provides the upstream owner's navigation without an extra holder trait.
`None` supplies the useful no-child/fake component behavior. Kotlin NoOp's empty
history cannot itself be attached under upstream's nonempty history contract.

`HistoryBackend` supplies current, replace, push, asynchronous-capable go and
pop subscriptions; `MemoryHistoryBackend` implements the same operations with
owned entries and RAII listeners. Managed `HistoryState`/`HistoryNode` trees
contain explicit Rust key bytes and saved paths, not Kotlin JSON or
SerializableContainer. A real host must identify its own managed entries,
serialize them, wire browser pop events and choose startup/deep-link behavior.
The core never accesses DOM, an OS event loop or a wasm browser API.

`HistoryController::attach` keeps an existing managed startup entry without
automatically restoring the model. An absent/unmanaged entry is replaced by
one entry containing the entire initial tree, even for a multi-item Stack.
Push expands nested paths; pop uses their count and clamps to the managed
boundary; arbitrary membership changes rewrite from that boundary. Equal keys
update the URL. Active child emissions push/pop their own history, while a
parent emission replacing its child updates the current parent entry. Paths
are concatenated along the active chain, query parameters merge in insertion
order with children overriding parents, and UTF-8 bytes use encodeURIComponent
escaping. Only active child chains are observed; inactive state is snapshotted.

Stack history can include all configurations or the active configuration only.
Pages uses the whole Pages state as one key, including empty Pages. Panels uses
membership as one key and preserves the local lifecycle mode on restoration.
Each standard bridge requires a navigator connected to exactly one receiver.

`StackHistoryController` implements the older algorithm: attachment initializes
one entry per initial configuration; push appends; changing only the active
suffix replaces its first item and pushes the rest; changing a larger suffix
moves to the common prefix then appends. Full replacement retains old Forward
entries when no push truncates them, matching upstream. `history_paths()` is
available before attachment. Browser restoration decodes the target stack for
the guard, pops to the last existing equal active configuration, or reconstructs
missing Forward items through the saved path mapper. An already equal active
configuration preserves the current stack, even when lower target items differ.

## Ownership, completion and errors

The returned `HistoryController` owns model/host subscription tokens; Drop,
explicit cancel and lifecycle Destroy disconnect them. Weak callback links
avoid cycles. Cancellation stops subsequent recursive restoration and host
writes, including late completions. A callback already started in the application
cannot be rolled back by the controller. Guard traversal is child-first.

Host/model operations may complete synchronously or later. FIFO queues prevent
feedback from model publication during restoration; programmatic host movement
suppresses its own pop notification. Consecutive browser events are buffered
while a model restoration is pending, with later events continuing after a
veto. The committed target index and actual host position are tracked separately
so delayed events do not bounce by a stale index. Invalid indices, empty histories,
cycles, codec errors and host failures are typed HistoryError values.
Latest failures remain observable through `errors()` and an optional handler.
User callback panics propagate, with guards restoring synchronization state
for both synchronous dispatch and asynchronous restoration continuations.
No transactions can undo already successful host writes or application effects.

## Evidence

`tests/web_history.rs` exercises both algorithms, all three standard bridges,
nested histories, URL/query encoding, push/pop/rewrite, startup semantics,
Back/Forward, guards, queued completions, delayed host movement, malformed keys
and indices, cycles, cancellation, Destroy and ownership release.
`examples/web_history.rs` runs without a browser. Final validation is recorded
in COMPATIBILITY.md; cross compilation tests the portable code, not an absent
browser adapter. The Kotlin suite is inspected but not executed as a differential
oracle, and no Kotlin serialized-byte compatibility is promised.
