# Portable navigation schema versions

## Reference and reproducible comparison

On 2026-10-11, `git ls-remote https://github.com/arkivanov/Decompose HEAD`
returned `0824a183f7fe3862a1409553e66346c999de7084`. Thus the current
reference for this changed contract is exactly the audit reference; the former
porting reference remains `00840bcfed6d55710c8fbb1a72503f2041eda490`.

Inspect both immutable revisions without changing the reference checkout:

```sh
rtk git -C upstream-ref fetch origin 0824a183f7fe3862a1409553e66346c999de7084
rtk git -C upstream-ref diff 00840bcfed6d55710c8fbb1a72503f2041eda490 0824a183f7fe3862a1409553e66346c999de7084 -- decompose/src/webMain/kotlin/com/arkivanov/decompose/router/webhistory decompose/src/jsMain/kotlin/com/arkivanov/decompose/router/webhistory decompose/src/wasmJsMain/kotlin/com/arkivanov/decompose/router/webhistory decompose/src/webTest/kotlin/com/arkivanov/decompose/router/webhistory
rtk git -C upstream-ref show 0824a183:decompose/src/webTest/kotlin/com/arkivanov/decompose/router/webhistory/WebHistoryNavigationTest.kt
```

The exact `webMain`, JS, wasmJs implementations, `BrowserHistory`, and history
regressions were read at this revision. JS and wasmJs wrappers expose an optional
schema version, discard incompatible saved root state before restoration, recreate
fresh state keepers, and cancel the previous model binding when publishing a new
root. Shared `enableWebHistory` always rewrites the current host entry, computes a
base index offset from host index versus root history length, checks recursive
guards before recreating incompatible history, and compares versions before
consuming route nodes. Source inspection verified these changes. **Kotlin tests
were not executed**; the Rust tests are executed regressions and do not establish
serialized-byte equivalence.

## Rust contract

`HistoryController::attach_with_options` accepts `HistoryOptions { version,
recovery }`. `version: Option<String>` compares exactly. All managed entries
written by a recursive controller carry that version. Matching versions allow
normal route restoration; a mismatch invokes general recursive guards without
passing incompatible target bytes to application target-dependent guards or
route codecs. Rejected navigation returns to the last committed host index.
This index records the same destination as upstream's initial base-offset formula,
including different root stack lengths at attachment, and continues tracking
successful deferred restorations.

Recursive attachment replaces the current entry with the supplied current model
and URL while retaining its saved managed index. It does not restore that old
entry or append entries for an initial multi-item stack. This intentionally
updates the previous Rust startup behavior, which kept existing entries intact.
`StackHistoryController` retains its separately documented legacy stack semantics,
including initial expansion, target guards, and path mapping on Forward.

The default recovery policy is `Reject`: return to the committed entry and report
`HistoryError::VersionMismatch(HistoryVersionMismatch)`. The optional `Recreate`
hook receives only typed mismatch metadata and the original Pop destination URL, never obsolete
route bytes. Its one-shot completion can return immediately or later; no async
runtime is needed. Return `HistoryRoot::new(new_navigation)` and optionally
`.with_lifecycle(&new_lifecycle)`. The application owns its real root component,
creates fresh context/state stores, publishes that root, and destroys the old root
lifecycle. Old model and lifecycle subscriptions detach before the hook runs;
a new lifecycle binding replaces them after successful completion. A host-history
listener remains during deferred recreation to queue Back/Forward targets.
Cancellation removes this listener, every model/lifecycle observer, factory
captures, and queued work. Late completion is ignored and its returned root is
released. Deferred recreation rewrites the originally requested entry, preserving
later queued host targets. New model changes then synchronize normally.

A typed factory failure or invalid replacement history reconnects the old model
and returns to the committed entry when the failed destination is still current
and no later host navigation is queued, permitting retry if the old root is still
usable. A factory panic preserves its original panic and reconnects old observers
before propagation. A host that has already destroyed its old root should either
return a usable replacement or cancel the controller on failure. The core cannot
reconstruct application-owned components or undo application lifecycle destruction.

## Host destination API migration

The follow-up correction changes the public `HistoryBackend` implementation
contract. Existing callers of `current()` keep the same API; it now defaults to
the entry from `current_destination()`. Backend implementations must provide:

- `current_destination() -> Option<HistoryDestination>`: an atomic entry snapshot
  and its stable `HistoryLocation` identity, including for unmanaged entries.
- `subscribe(Rc<dyn Fn(HistoryDestination)>)`: capture the destination before
  delivering observers, rather than passing only `Option<HistoryState>`. Preserve
  that original URL/state if an earlier observer reenters or replaces an entry.
- `replace_destination(&HistoryDestination, HistoryEntry)`: replace that exact
  entry without host movement or Pop notification. Require its original contents
  to still match; reject changed/discarded targets without any write or movement.

Allocate `HistoryLocation::new()` once per physical host entry, clone it for
snapshots, and retain it across replacement. Allocate a fresh identity for each
new entry, including a new forward branch that reuses a physical slot. Identities
are opaque in-process tokens, not managed indices, physical offsets or persisted
router state. Hosts keep their own mapping to actual entries.

`MemoryHistoryBackend` implements replacement of noncurrent destinations. A host
unable to perform that operation safely must return `HistoryError::Backend`;
the controller reports it and reconnects the old model. It must never fall back
to replacing whichever entry is current. This is a source-breaking trait change
for custom host implementations; silently supplying approximate identities would
retain the defect. Router persistence envelopes are unchanged.

Both the pending Pop queue and recreation completion own their original
destination snapshot. Successful recreation updates that entry directly and
leaves later host navigation in place. A repeated queued visit keeps its original
URL/schema while its expected snapshot advances after a matching successful
controller write; external writes still fail snapshot validation. Pending Pops
run before newer events from reentrant completion or error callbacks.
Unmanaged entries never stand for managed
index0; model changes while the host is unmanaged report an error without writing
there. A later managed Pop resumes synchronization. Managed indices remain model
metadata; deferred replacement uses only host identity and snapshot validation.

## Persistence migration

`HistoryState` gains public `version: Option<String>`; struct literals must specify
it. Host adapters reading older stored entries must assign `None` when this field
is absent. `None` matches `None`; starting a versioned application makes old
unversioned Back/Forward entries incompatible and subject to the explicit policy.
The current entry is rewritten with the new version at attachment. Route key bytes,
node trees and existing state-store persistence formats remain unchanged. No
Kotlin/Rust JSON or binary compatibility is claimed. Root-state envelope decoding,
URL selection, storage and actual root construction belong to the host adapter.
Call `HistoryOptions::validate_saved_version(envelope_version)` before invoking
the root payload decoder or factory with restored data. This typed portable gate
is the same comparison used for Back/Forward, accepts matching legacy versions,
and rejects mismatches so the host can supply fresh state before root construction.

## Semantic scenarios and evidence

`tests/fixtures/history-versioning.tsv` is a portable trace specification consumed
by `portable_semantic_trace_fixture_matches_history_contract`. Its matching,
legacy, mismatch, and guard/stack-length scenarios correspond to the inspected
Kotlin source and `GIVEN_previous_history...` regressions at lines 803–976 of the
reference history tests. A Kotlin harness can consume the same fixture, but none
was run for this fix. Additional Rust tests cover replacement observers, lifecycle
replacement, factory failure/panic, reentrant recreation, deferred Forward,
cancellation and late completion. Existing recursive and legacy history tests
continue to exercise normal Back/Forward and deferred host/model completions.

Focused verification:

```sh
rtk cargo test --test web_history --test web_history_versioning --locked
```

Intentional exclusions: DOM/sessionStorage adapters, JS/wasm host wrappers, UI root
publication, and Kotlin execution. They are unnecessary to the portable core.
