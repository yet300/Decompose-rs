# ChildSlot design

Date: 2026-10-09. Status: implemented and verified; authorized by the user after accepting
the proposed ChildSlot scope and creating codex/child-slot from main@e96eaf1.

## Scope

One decompose-rs crate; add router::slot with ChildSlot, SlotNavigation,
SlotOptions, SlotCodec and child_slot. No UI adapters, extra navigation families,
async runtime, mandatory serde or Essenty modifications. Behavior reference:
local Decompose revision 00840bcfed6d55710c8fbb1a72503f2041eda490, specifically
router/slot/ChildSlotFactory.kt, SlotNavigatorExt.kt and integration tests.
Official overview: https://arkivanov.github.io/Decompose/navigation/slot/overview/
Navigation: https://arkivanov.github.io/Decompose/navigation/slot/navigation/

## Alternatives and choice

Recommended: share the private navigation transaction/event/scope machinery
between Stack and Slot, with small model policies defining cardinality,
snapshot shape, Back transformation and persistence. Preserve Stack's public
imports, signatures and DST1 bytes; the new model exercises empty membership.

A standalone Slot engine could be implemented quickly, but duplicating Stack's
rollback, deferred Back and lifecycle machinery would create two places for the
same fixes. A public fully generic children engine would broaden the API and
freeze untested abstractions before Pages exists. Neither is needed here.

## Public API

- ChildSlot<C,T> has pub child: Option<Child<C,T>>.
- SlotNavigation<C> is a shared source supporting new/Default/Clone without
  C: Clone; navigate and navigate_with take Option<Rc<C>> transformations.
- NavigationResult<C> in the slot module has old_configuration and
  new_configuration, both Option<Rc<C>>. navigate_with completion receives
  Result<NavigationResult<C>, NavigationError>, once per connected slot.
- activate(C) and dismiss() are conveniences; activate_with and dismiss_with
  provide result completions (dismiss success reports whether a child existed).
  Convenience calls propagate construction/transition failures as panics,
  consistent with Stack; explicit callbacks expose typed results.
- child_slot(&Ctx, &SlotNavigation<C>, Option<C>, SlotOptions<C>, factory)
  returns Result<Value<ChildSlot<C,T>>, NavigationError>.
- SlotOptions defaults: key DefaultChildSlot, handle_back_button false,
  codec None. SlotCodec encodes one &C and decodes &[u8] to C; empty persisted
  membership is represented by the envelope without invoking the codec.
- C: Eq + 'static, Ctx: ComponentContext + 'static, T: 'static; no Clone,
  Send or Sync bounds. Reuse Child and NavigationError with compatibility
  reexports under router::stack and reexports under router::slot.

## Transactions, lifecycle and ownership

No child is a valid initialized state and requires no child factory call.
Equal active configurations preserve the child key, instance and lifecycle;
empty-to-empty does not create scopes. Completion still runs for no-op events.
A changed configuration prepares its scope before ending old membership;
old active lifecycle ends before the new child resumes. A child targets Resumed,
bounded by the parent. Dismiss destroys lifecycle and retained child scope even
if old snapshots retain Rc component instances. Parent-owned navigation survives
dropped views, and parent destruction detaches sources and Back callbacks.
External retained keeper survives parent lifecycle recreation; default root
keeper ends with its lifecycle. Scope creation uses ContextFactory/ContextParts.

Factory, lifecycle, observer and completion reentry is FIFO. Back-triggered work
waits until its dispatcher borrow ends. Panics propagate, clean incomplete scopes
and abandoned events, and reset processing flags. Successful callback side effects
cannot be reversed. Failed initialization returns restored bytes and registrations
for retry. Parent destruction from callbacks aborts uncommitted construction.

## Back and composition

When enabled and populated, automatic Back dismisses the slot. Enabled child
handlers have precedence. Empty slots do not consume Back. Predictive events
forward to the child; cancellation leaves membership unchanged. Stack and Slot
can coexist under different keys and nested custom contexts; same-key conflicts
are rejected before factories even when recursively attempted during construction.
Sibling callback priority follows existing SharedBack priority/LIFO semantics.

## Persistence

A codec enables a distinct versioned slot envelope (DSL1). It stores explicit
zero-or-one membership, configuration bytes when populated, stable child key,
nested child state and monotonic key allocation counter. Saving an empty slot
must overwrite an earlier populated save; restoration of empty state overrides
a nonempty initial configuration. Empty state never invokes configuration codecs.
Validate framing, version, count 0/1, complete consumption, nested child frames,
canonical child identity and counter before calling factories. Errors are typed;
input remains retryable. Restore reconciles retained keys with saved membership,
including empty saves, preventing stale retained scope reuse. Snapshot configs,
keys/counter and child state coherently when providers trigger navigation.

## Acceptance

Keep all existing 144 tests green. Add tests for empty initialization; activate,
equal-config identity/no lifecycle churn, replacement and dismiss; completion
success/empty results; lifecycle bounds and old snapshot teardown; all reentrant
callback origins including Back/panic recovery; child/predictive Back precedence;
Stack/Slot composition and nested non-Clone custom contexts; populated and empty
persistence, child state and retained/process recreation; malformed frames/codec
failures and constructor rollback; duplicate key reservation; dropped view/source
ownership and deterministic cleanup. Run fmt, clippy -D warnings, all tests,
rustdoc -D warnings, a runnable Slot example, and installed-target library checks.

## Delivery evidence

Implemented shared machinery in src/router/engine.rs and src/router/navigation.rs,
with Stack policies/wrappers preserving existing APIs, and Slot policy/API/DSL1
codec under src/router/slot. Common Child/NavigationError have compatibility
reexports. Scope matching additionally excludes ended lifecycles/keepers so
navigation after a lifecycle panic reconstructs a fresh child instead of
reporting success with a destroyed component. Independent spec and quality
reviews passed; final commands/counts/target matrix are in the implementation plan.
