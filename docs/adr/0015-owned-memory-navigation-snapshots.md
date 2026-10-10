# ADR-0015: Owned memory navigation snapshots

Status: Accepted. Reference Decompose revision:
`00840bcfed6d55710c8fbb1a72503f2041eda490`.

## Source contract and dependency boundary

Pinned `router/children/TransientNavStateSaver.kt` stores the actual NavState in
an in-memory SerializableContainer whose serializer writes no restorable state.
Recreating from the in-memory container preserves navigation; serializing and
reading it back does not. An absent saver resets navigation and is different.
Rust Essenty 0.1.0 stores `Vec<u8>` providers and restored maps only, so it cannot
hold this typed in-memory container. Its retained InstanceKeeper is a separate
service lifetime, not a navigation-state saver. No Essenty modification or
version upgrade is necessary.

## Decision

The existing Decompose StateKeeper adapter adds `enable_memory_navigation(key)`
for a router before construction, `save_snapshot()` and `from_snapshot()`.
`StateSnapshot` owns byte maps and private `Rc<dyn Any>` entries plus selected
router keys. There is no global registry, numeric side-channel, serializer or
disk write. Snapshot Clone shares immutable memory values. Dropping the last
snapshot/keeper owner releases them; `into_persistent()` extracts only bytes.

Selection works for Generic Navigation and all five standard models. It takes
priority over a supplied byte codec and its save/restore predicates, which are
not invoked. `codec: None` retains its existing meaning: without explicit memory
selection, parent saves omit navigation and recreation initializes afresh.
The selected key travels with the snapshot; hosts do not need to select it again
when restoring. Byte-only restoration does not carry that selection.

A memory entry contains the immutable whole model state, allocation counter,
stable child identities and each child's complete StateSnapshot. Standard states
share Rc configurations without Clone, Hash, Send or Sync bounds. Generic state
retains its existing whole-state Clone contract. Stored typed values require
`'static`, already required by the observer-owning navigation engine. Family
identity is checked separately from the Rust state type: Stack and Slot can
share an internal Vec state type but cannot restore each other's state.

Live children and destroyed tombstones preserve nested memory. Fixed child
contexts save their complete namespaced snapshot. Persistent outer routers keep
byte membership authoritative and carry supplemental child snapshots through
in-process recreation; byte-only restoration has no supplemental memory.
An outer saver that declines the save does not query children, including nested
memory providers. Memory-only outer state keeps nested byte values in memory,
but none of that router's subtree is emitted into its parent's durable bytes.

Snapshot providers produce bytes and memory in one traversal. A provider is
called once per snapshot, preventing byte and memory values from diverging when
callbacks are stateful. Ordinary byte saves retain their existing family framing
and fixed-child last-good fallback behavior. Snapshot-save failures are typed
errors rather than silently inventing a successful memory snapshot.

Construction reserves the key before application callbacks, validates saved
membership, identities and model type, skips the lazy initializer on valid
restoration and returns both consumed representations on any failure. A snapshot
does not retain controllers, observers, factories or lifecycles. Retained service
identity still depends on independently preserving the InstanceKeeper; saving
navigation alone does not keep live component instances running.

## Consequences and evidence

Applications explicitly own the recreation snapshot and choose its lifetime.
`TestComponentContext::recreate(true)` uses this path and preserves retained
services. `recreate(false)` saves only bytes and destroys retained services.
Existing Rust DCS1/DST1/DSL1/DPG1/DPN1/DPI1/DCH1 formats remain compatible;
Kotlin SerializableContainer bytes require explicit application migration.

`tests/memory_navigation.rs` covers every model, non-Clone configurations,
stable Rc/key identity, nested byte data, retained services, fixed contexts,
destroyed-page resurrection, type mismatch/retry, factory panic rollback,
one provider traversal, predicate precedence and ownership release.
`examples/memory_navigation.rs` is runnable and distinguishes configuration
recreation from process recreation. Final checks are in COMPATIBILITY.md.
