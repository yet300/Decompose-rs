# ComponentContext and ChildStack: Rust port design

Date: 2026-10-09. Status: approved by the user and implemented.

## Scope and evidence

The user selected ComponentContext and ChildStack as the first delivery.
Keep the existing four published `essenty-* = "0.1.0"` dependencies.
Do not port Jetpack Compose, Android, SwiftUI, Reaktive, web history, Slot,
Pages, Panels, Items, or expose a public generic navigation engine in this delivery.
`Value` and navigation events are necessary parts of ChildStack, not foreign integrations.

Behavioral reference: local upstream checkout at
`00840bcfed6d55710c8fbb1a72503f2041eda490`; this is a pinned reference, not
a claim about the latest GitHub revision.
Existing port inspected at `c368f58`.

Verification before changes:

- `cargo test --all-features`: 86 unit tests and 1 doctest passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features`: passed.

These checks establish the baseline; they do not prove upstream parity.

## Audit of the existing foundation

The single-crate boundary, synchronous Rc-based threading model, detached
`ContextFactory`, support for non-Clone custom contexts, and `SharedBack`
adapter are useful foundations. A rewrite of all existing code is unnecessary.
The following source-level findings need reproducing tests before fixes.

### A. Restored state is lost on first failed save

`src/child.rs::build_child` consumes the parent's restored child frame, then
initializes `last_good` to `None`. If the child's first save fails or its keeper
is temporarily mutably borrowed, the provider returns `None`.
Essenty's `save` preserves only *unconsumed* restored entries; the child's frame
has already been consumed. Consequently the saved parent map omits the child.
Existing tests first perform a healthy save, or start without restored bytes;
neither covers this case.

Fix: initialize the fallback frame from the consumed, successfully decoded
original frame. A healthy save replaces that fallback. A fresh child with no
prior state may still omit its key on a failed first save.

### B. Construction rollback does not restore consumed input

`Rollback` tracks registrations, instance entries, subscriptions and bridges,
but does not track the consumed restored frame. A decode failure, instance
type mismatch, busy back dispatcher, or factory panic can therefore consume
restored input permanently. Existing duplicate-key protection happens before
consumption and does not cover these later failures.

Fix: keep a per-parent pending-restoration store in the Decompose adapter so a
failed attempt can make the same frame available to retry and parent save.
Essenty 0.1.0 exposes consume but no restore-entry insertion API; do not replace
the whole StateKeeper, erase unrelated providers, or pretend unregister undoes
consumption. The concrete adapter must preserve public handle identity and
isolate its bookkeeping from user keys. If this cannot be composed cleanly
with the current raw SharedKeeper alias, specify a narrow API correction
instead of introducing hidden global state.

### C. Fixed-child retention follows the wrong lifetime

The fixed-child path attaches a hook to the parent lifecycle that removes
the retained child entry and destroys its nested instance keeper. Upstream
`instancekeeper/ChildInstanceKeeper.kt` installs such a hook only for an
explicitly supplied child lifecycle, not for fixed children. A caller-supplied
parent InstanceKeeper can outlive the parent lifecycle during recreation.
The current hook destroys fixed-child retained instances even in that case.
`TestContext::recreate` currently keeps the old lifecycle alive, so its
identity test cannot detect this discrepancy.

Fix: distinguish component lifecycle end from retained scope end. Fixed-child
retention ends when its parent retained scope ends, not simply when the old
component lifecycle is destroyed. Strengthen recreation tests to save state,
destroy the old lifecycle, and build a new root with the same external keeper.
If parent-scope cascade requires a Decompose-owned retained wrapper, use that
wrapper; value Drop may still be deferred by external Rc handles.

### D. Public fixed-child helpers are unsuitable for navigation

`child_context_with_lifecycle` intentionally panics when its supplied lifecycle
is destroyed. Upstream `DefaultChildItemFactory` instead assembles navigation
contexts directly from a factory and navigator-owned scope parts.
ChildStack must do the same. Do not weaken the public manual-destroy contract.

## Architecture alternatives

1. **Recommended: one crate, focused modules.** Keep `decompose-rs`; add
   `value` and `router::stack`, with private scope/event machinery. This keeps
   ownership rules in one implementation and preserves existing imports.
2. Separate context, values, navigation and facade crates. This adds package
   releases, cross-crate internal APIs and dependency management without a
   demonstrated consumer needing those boundaries. Reconsider for independent
   optional UI adapters or independently released integrations.
3. Reproduce the Kotlin module/class hierarchy. This duplicates abstractions
   for factories, covariance and platform dispatch that Rust generics and
   ownership already provide. Preserve behavior, not the Kotlin class tree.

Implemented layout (existing module paths were retained):

```text
src/lib.rs                 exports and crate documentation
src/context.rs             ComponentContext, ContextParts, factories
src/child.rs        fixed-child construction and rollback
src/retention.rs    child retention adapter and end-of-scope rules
src/merged.rs            merged lifecycle internals/public facade
src/shared_back.rs                 SharedBack and private bridge machinery
src/state.rs                reversible state adapter
src/framing.rs              child-state byte framing
src/value.rs                Value, MutableValue, Subscription
src/router/mod.rs
src/router/stack/mod.rs     Child, ChildStack, StackOptions, construction
src/router/stack/navigation.rs  StackNavigation and operations
src/router/stack/engine.rs  private lifecycle/diff/event processor
src/router/stack/state.rs   save/restore envelope and codec boundary
tests/component_context.rs public API and recreation regression tests
tests/child_stack.rs        behavior, lifecycle order, identities
examples/child_stack.rs     runnable runtime-independent example
```

Move existing modules only as needed to keep responsibilities clear; retain
the existing root-level reexports. Tests may remain internal where they
need private observer counts. Do not split files merely to mirror upstream.

## Rust API and ownership

- `ComponentContext` remains generic and does not require Clone, Send or Sync.
- Configuration `C` is immutable through shared handles; require `C: Eq + 'static`.
  Use `Rc<C>` snapshots so custom configurations need not implement Clone.
- Component `T` is exposed as `Rc<T>` in `Child<C, T>` snapshots; no `T: Clone`.
  A removed child can remain reachable from an old snapshot, but its lifecycle
  must already be Destroyed and its retained scope must already be ended.
- Child keys are stable opaque identifiers allocated per stack, persisted in
  saved state; do not derive them from Debug, memory addresses or hashes.
- `ChildStack` has one active child and an ordered back stack, with an iterator
  over all items from bottom to top. An empty stack is not representable.
- `Value<T>` exposes current immutable `Rc<T>` and synchronous observation.
  Subscription guard Drop cancels observation; explicit detach is available
  for a lifecycle-owned subscription. Do not require Tokio or a GUI framework.
- A `StackNavigation<C>` is a cheap shared event source separate from the
  observable stack. It can broadcast to multiple stacks like upstream.
- Nested navigation is queued FIFO and processed after the current event,
  including factory, lifecycle, observer and completion callback reentry.
- Completion reports `Result<NavigationResult<C>, NavigationError>` for every
  processed event. A queued call cannot truthfully report its final result
  synchronously; its completion callback is authoritative.
- Duplicate configurations are rejected by default; allow duplicates through
  per-stack options, with occurrence-based identity matching. Avoid mutable
  process-global settings.

## Lifecycle and transactions

Construct navigation scopes with the detached context factory and private
scope adapters. Do not use the public fixed-child helper for stack elements.
The navigator owns each child's local lifecycle; the component observes its
merge with the parent. Back-stack children are CREATED, the top child targets
RESUMED, and the merged lifecycle limits both by the parent state.

Validate nonempty/duplicate constraints before creating or destroying any
child. Preserve the instance and key of each surviving configuration.
Stop or pause the old active child before resuming the new one; never expose
two resumed children at once. Removing a child stops its back bridge, destroys
its lifecycle and ends its retained scope. Publish the final stack and run
completion only after lifecycle operations finish.

Prepare newly required children before committing membership changes.
Rollback prepared scopes on a recoverable construction error. User panic is
propagated; unwind guards release temporary resources and reset event pumps,
but do not promise rollback of arbitrary user callback side effects.

Parent lifecycle destruction ends live components and source subscriptions.
Externally retained instance scopes survive recreation when still owned by
the external InstanceKeeper. A default root's attached keeper ends with its
lifecycle. Recreated components are fresh; retained service instances may
keep their Rc identity.

## Back handling

`handle_back_button` defaults to false like upstream. When enabled and depth
exceeds one, Back pops the top. A resumed child's own enabled handler has
precedence over the stack-pop callback, following upstream's nested bridge
priorities. Inactive children do not consume Back.

`SharedBack` currently rejects new registrations during dispatch. Navigation
requested from a Back callback must therefore defer scope preparation until
the relevant dispatcher is idle. Add a private after-dispatch work queue that
runs only after the Essenty borrow and busy window end, preserves FIFO,
and is cleared on panic. Parent-to-child forwarding must account for all
busy dispatchers involved; a local relay alone does not release those borrows.
Reentrant direct calls to the same back dispatcher retain their existing
false/no-op contract.

Predictive event forwarding remains supported by SharedBack. UI animations
and browser integration are outside scope.

## Stack operations

Provide `navigate`, `push`, `push_new`, `push_to_front`, `pop`, `pop_while`,
`pop_to`, `pop_to_first`, `replace_current`, and `replace_all`.
`pop` on one child is a no-op. `push_new` on the already active configuration
is a no-op. `push_to_front` matches equality and preserves a surviving child.
`bring_to_front_by` accepts an explicit kind selector or same-kind predicate:
Kotlin runtime-class comparison does not model Rust enum variants.
Document this adaptation rather than using TypeId for every enum variant.

## Persistence

Persistence is part of this ChildStack delivery. Offer explicit user codecs
for configuration lists to/from bytes, with a transient default that does
not restore a stack. Keep serde optional/outside core.
A versioned envelope stores the encoded configuration stack, each child key,
and each child's state map in positional correspondence.
Validate framing, version, counts, unique keys, nonempty configuration stack,
duplicate policy, and complete input consumption before constructing scopes.
Malformed saved state returns a typed error and does not silently reset data.
This delivery exposes no automatic corruption fallback. A caller that chooses
to discard incompatible state can assemble a fresh context/keeper explicitly.

## Acceptance tests and quality gates

Tests must exercise observable contracts, not only private implementation.

1. First failing save after restoration retains the original child frame;
   later healthy save refreshes it.
2. Failure after consume preserves retryable state and unrelated providers.
3. Destroy old lifecycle, recreate with external keeper: nested retained
   instance identity survives; process-death recreation gets fresh instances.
4. Custom non-Clone context services survive at least three nested generations.
5. Push/pop/replace/reorder preserve surviving instances and stable keys.
6. All stack operations match boundary behavior, including duplicate options.
7. Parent pause/stop/resume bounds children; old active stops before new resume.
8. Removed children are destroyed even while old UI snapshots hold Rc handles.
9. Navigation from factory, lifecycle, observer, completion and Back callback
   runs FIFO without RefCell borrow panic or a stale final state.
10. Back prioritizes a child's callback, then pop, then the parent fallback.
11. Save/restore round-trip includes stack order and nested child state;
    corruption and codec failures are explicit.
12. Source/observer subscriptions detach on scope end; dropped views do not
    prematurely destroy a parent-owned stack; no internal Rc cycle survives
    deterministic teardown.

Run format, Clippy with warnings denied, all tests/doctests, rustdoc with
warnings denied, and the runnable example. Check installed host/WASM targets
when available; do not claim unexecuted cross-platform checks.

## Implementation sequence

1. Reproduce and correct ComponentContext state/retention discrepancies.
2. Add observable values and navigation events with reentrancy tests.
3. Add private navigation scopes and stack diff/lifecycle machinery.
4. Add stack operations, Back routing and deferred work.
5. Add configuration codecs, restoration and recreation parity tests.
6. Update README, roadmap, public documentation and the example; run gates.

Use failing behavioral tests before each change. No Essenty fork, new runtime,
new UI dependency, public generic engine, or new crate is required by this design.

## Implemented API decisions

The StateKeeper adapter is public; SharedKeeper wraps that adapter, and the raw
primitive is reexported as EssentyStateKeeper. This is the narrow API correction
required by finding B. StateKeeper::from converts owned primitives.
Fixed and explicit child retention survive old parent lifecycle destruction
when an external keeper owns them. Public explicit-child manual destruction
still ends that scope and panics.
Convenience navigation methods panic on rejected transformations; navigate_with
reports typed results through completion. Successful user callback side effects
are outside construction rollback. The acceptance evidence and final target
matrix are recorded in the implementation plan; ADR-0005 explains these decisions.
