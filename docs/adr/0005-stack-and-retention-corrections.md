# ADR-0005: ChildStack and retention lifetime corrections

Date: 2026-10-09. Status: accepted and implemented.
Reference: Decompose `00840bcfed6d55710c8fbb1a72503f2041eda490` and Essenty 0.1.0 registry sources.
Supersedes conflicting Phase 1 assumptions in earlier ADRs and research documents.

Keep one `decompose-rs` crate. Public `router::stack` and `value` modules expose
stack navigation and observable snapshots; scope/event/persistence machinery
remains private. There is no separate runtime, UI dependency or public generic
navigation engine in this delivery.

Configurations require Eq and are shared as Rc; contexts, configurations and
components do not require Clone. Components exposed by old snapshots can remain
alive after removal, while their lifecycle and retained scope are already ended.

Retained scopes belong to a parent InstanceKeeper, not to the old parent
component lifecycle. Decompose stores a private RetainedScope owner in the
parent keeper; releasing that owner marks the nested keeper destroyed even
when public handles remain. Fixed children and explicit-lifecycle children
preserve retained identity across parent lifecycle recreation when an external
keeper survives. Explicit child-lifecycle destruction retains its public panic
contract and ends that child's retained scope. Default roots still attach
their newly created keeper to their lifecycle.

The public StateKeeper name now denotes a small Decompose adapter around
EssentyStateKeeper. SharedKeeper remains Rc<RefCell<StateKeeper>>. Existing
Essenty keepers can be converted with StateKeeper::from; EssentyStateKeeper is
also reexported. The adapter owns returned restored frames and fallible byte
providers. Failed child/stack construction restores consumed input for retry;
the original child frame initializes the last-good fallback for a failing
first save. Provider failures in stack codecs propagate as StateKeeperError::Encode.

Stack contexts are assembled through ContextFactory and ContextParts directly.
Navigation never uses the fixed-child helper whose manual lifecycle cannot be
destroyed. Local lifecycle targets are Created for back-stack children and
Resumed for the active child, bounded by the parent through MergedLifecycle.

Recursive events are queued; scopes are prepared before membership commit.
Back work runs after its dispatcher releases its borrow. Canceling deferred
work during unwinding resets the engine's deferred flag and drops the abandoned
events. Enabled state is ready before observable publication. Child Back has
priority over automatic stack pop; inactive children are gated explicitly.

StackCodec supplies configuration encoding/decoding without a serde format.
DST1 stores configuration bytes, positional stable child keys/state, and a
checked allocation counter. Restore validates all structure and key identities
before factories. A previous retained map is reconciled with the restored keys
so old snapshots cannot bind an unrelated new configuration to an old scope.

Convenience navigation methods panic on rejected transformations, matching
upstream contract violations; navigate_with supplies a typed Result callback.
User panics propagate. Construction guards clean up incomplete resources and
restore input, but successful user callback/navigation side effects are not
reversed. Destroyed retained owners are replaced with fresh scopes on retry.
Dropping a view never ends parent-owned navigation. Subscription Drop cancels
observation; callback captures remain the caller's ownership responsibility.

Validation: regression tests exercise the original state-loss and retention
errors, lifecycle and Back reentrancy, rollback, non-Clone custom nesting,
state consistency under provider navigation, and source/scope teardown.
The runnable example and compile-target evidence are recorded in README.
