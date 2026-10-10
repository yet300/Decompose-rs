# ADR-0017: Thread-local settings and public test contexts

Status: Accepted. Upstream reference:
`00840bcfed6d55710c8fbb1a72503f2041eda490`.

## Upstream contracts

[DecomposeSettings](https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/DecomposeSettings.kt)
has a mutable global duplicate policy, main-thread check and nonfatal-error
handler. The policy excludes Child Items. Its experimental update helper
serializes mutation. The deprecated DecomposeExperimentFlags property forwards
to this stable settings property.

[TestComponentContext](https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose-test-utils/src/commonMain/kotlin/com/arkivanov/decompose/testutils/TestComponentContext.kt)
defaults to a Resumed lifecycle and independently controlled state, instance
and Back dispatchers. Recreation saves state before teardown, temporarily
creates an Initialized lifecycle so it can be destroyed, preserves the
previous lifecycle state for the new context and supplies a fresh Back
dispatcher. Configuration recreation retains the instance dispatcher;
process recreation explicitly destroys it.

## Decisions

`DecomposeSettings` lives in the existing crate. Global means current-thread
global: the core and its callbacks use `Rc`, and making callbacks `Send + Sync`
would change the architecture. `current` returns a snapshot, `replace` returns
the previous settings, `update` retries its transform if reentry replaced the
snapshot, and `with` restores previous settings after normal return or panic.
Transforms must tolerate repeated evaluation. No settings borrow crosses user
callbacks, and replaced callback captures are released outside mutable borrows.

StackOptions, PagesOptions and ChildrenOptions read the global duplicate policy
when `Default::default` executes. Their existing explicit `allow_duplicates`
booleans always take priority; no public struct field is added. A previously
constructed options value and an already running router retain their policy
when global settings change. Items remains unique-only. This snapshot contract
preserves complete existing struct literals and makes explicit `false` work
even with a global `true` default. Slot and typed Panels have no equal-
configuration occurrence policy to configure.

Typed `DecomposeDiagnostic` observations retain the original NavigationError or
DecomposeError and identify navigation operation and router key. The default
handler prints to stderr. Reporting supplements existing Result/completion
delivery and convenience panics; it never converts a failure to success.
Handlers may change settings and enqueue navigation. Construction diagnostics
are delivered after rollback, allowing same-key retries. A recursion guard
suppresses directly nested diagnostic delivery and resets on panic. Navigation
sources also tag events emitted by a handler, carrying suppression through
their FIFO and through the engine FIFO while preserving Result/completion
delivery. This prevents a handler that always emits another invalid event
from causing an unbounded diagnostic cycle. Unrelated later events report
normally, including after a nested completion panic. Arbitrary user handler panics propagate,
as other user callbacks already do; diagnostics cannot guarantee recovery
from arbitrary handler side effects. Existing SharedKeeper borrowing rules
still apply to callbacks invoked during a caller-held save borrow.

There is deliberately no `main_thread_check_enabled` switch: the independent
core has no OS/UI event-loop identity and can be created on any thread.
`Rc`/`RefCell` prevent moving its handles across threads at compile time.
Checking an invented "main" thread would reject supported usage while giving
no valid UI-host guarantee. A platform host adapter can check its own event
loop and report a host error separately.

`test_utils::TestComponentContext` is public without feature gating or a
separate crate because all collaborators are production dependencies already.
It exposes constructors for lifecycle state, supplied services and detached
factory parts. ComponentContext handles provide lifecycle transitions, byte
providers, retained-instance operations and normal/predictive Back controls.
Children preserve this concrete Rust context type through its factory, as
required by the existing ComponentContext contract.

`recreate(configuration_change)` returns typed save/lifecycle errors. A save
failure leaves the old lifecycle and keeper untouched. A destroyed lifecycle
is rejected before saving. Configuration recreation uses the Decompose
StateSnapshot and retains memory-only navigation; process recreation uses
durable bytes. Old lifecycles end on either path. Instance keepers are externally
controlled, so destroying just the test lifecycle does not destroy retained
instances: use process recreation or keeper `destroy_all` when ending that
ownership. This mirrors the upstream test harness, rather than the default
application root context's automatic instance attachment.

## Additional public-API audit

The graph inventory of pinned commonMain classes and function declarations
was checked beyond COMPATIBILITY.md. Cancellation maps to the public RAII
Subscription with explicit idempotent `cancel`; SimpleChildNavState maps to
the public concrete ChildNavState; factory-owner capability belongs to
ComponentContext; Child copy operations map to public Rust fields and Clone
handles; Value and router property extensions map to `get()` and public
snapshot fields. Kotlin annotations, variance, delegated properties and the
deprecated experiment flag add no missing behavior. `Ref`, `hashString` and
`keyHashString` are annotated InternalDecomposeApi; RefCell and exposed stable
child keys cover their relevant mechanism without reflection/hash requirements.
Internal controller, locking and child-service classes remain implementation
details. Host/main-thread implementations and third-party framework modules
remain platform/UI integrations. Browser-independent WebNavigation and its
family adapters are audited and implemented separately in this delivery.

## Evidence

`tests/settings.rs` exercises global defaults and complete explicit literals,
reentrant updates, scope unwind, navigation completion errors, handler reentry,
construction rollback retries, recursive-report suppression and existing
convenience panics. `tests/test_utils.rs` uses the crate as an external consumer
and covers nested state, Back, lifecycle-state preservation, retention,
process recreation, save failure/retry and memory navigation. Runnable examples
are `core_settings` and `test_context`. Overall check results are recorded in
the compatibility matrix and roadmap after independent integration validation.
