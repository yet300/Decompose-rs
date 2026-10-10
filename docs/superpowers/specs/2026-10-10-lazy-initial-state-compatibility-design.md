# Lazy initial state and compatibility matrix

Date: 2026-10-10. Status: implemented and verified; independent spec and quality
reviews passed without findings.
Rust baseline: e129ab1 on main, clean after the previous local merge.
Reference: Decompose 00840bcfed6d55710c8fbb1a72503f2041eda490,
checked against the local pinned upstream sources. Essenty dependencies remain
the four published 0.1.0 crates. One decompose-rs crate, Rust 1.96/edition 2024.

## Goal and scope

Add caller-supplied lazy initial-state factories for all five standard navigation
families, including both Panels factory variants. Produce a compatibility matrix
that distinguishes verified behavior, deliberate Rust adaptations, remaining
core API gaps and excluded integrations. Existing eager calls must keep compiling.
This stage does not add Web/UI/platform bindings, serialization dependencies,
thread-safe variants, public test utilities or unrelated convenience APIs.

## API choice

Recommended: six explicit additive functions ending in `_with_initial`.
They avoid breaking existing eager calls and require no conversion traits or
coherence workaround. Existing eager functions delegate to their new counterpart
with `|| initial`; the lazy path owns the shared implementation.

Alternatives considered:

1. Replace existing initial arguments with closures: a smaller API but breaks
   every existing eager call and Rust cannot overload functions like Kotlin.
2. Accept values and closures through a conversion trait: adds wrappers or
   coherence restrictions, complicates inference and public generics for little
   benefit compared with named functions.

The names and initializer results are:

| New function | Initializer | Named generic parameters |
|---|---|---|
| stack::child_stack_with_initial | impl FnOnce() -> Vec<C> | Ctx, C, T |
| slot::child_slot_with_initial | impl FnOnce() -> Option<C> | Ctx, C, T |
| pages::child_pages_with_initial | impl FnOnce() -> Pages<C> | Ctx, C, T |
| panels::child_panels_with_initial | impl FnOnce() -> Panels<MC, DC, EC> | Ctx, MC, MT, DC, DT, EC, ET |
| panels::child_panels_without_extra_with_initial | impl FnOnce() -> Panels<MC, DC> | Ctx, MC, MT, DC, DT |
| items::child_items_with_initial | impl FnOnce() -> Items<C> | Ctx, C, T |

Every other argument, return type and configuration/context/component bound
matches the existing counterpart. Source accepts the existing borrowed
`&(impl NavigationSource<FamilyEvent> + ?Sized)`, including dyn sources. The
initializer is argument-position impl Trait, has no Clone or 'static bound,
may borrow local data, consume non-Clone captures and mutate borrowed captures.
It is not stored in a router or re-invoked on later navigation/recreation.

Example of the proposed API:

```rust
let stack = child_stack_with_initial(
    &context,
    &navigation,
    || vec![load_initial_route()],
    StackOptions::default(),
    |configuration, context| build_component(configuration, context),
)?;
```

## Evaluation and restoration contract

Lazy means evaluation during construction only when no usable state is restored;
it does not delay router initialization until get/subscribe or alter lazy Items
component activation. The shared engine already accepts FnOnce; thin standard
adapters pass the initializer through without evaluating it beforehand.

The initializer is invoked exactly once on the successful no-restoration path.
It is never invoked if the parent is already destroyed, the key is already
reserved, a restoration frame fails validation/decoding, or valid saved state
supplies the model. A valid empty Slot, empty Pages, empty Items or all-inactive
Items state is authoritative and must suppress initial evaluation. Saved Panels
membership and mode are authoritative, including the two-role adapter.

No codec keeps the existing transient behavior, using the initializer once.
Standard codec errors propagate; this stage does not add a decoder-decline
protocol to standard codecs. Generic children already has optional decode and
lazy initialization; its semantics remain unchanged. DST1, DSL1, DPG1, DPN1,
DPI1, DCH1 and DCS1 encodings remain unchanged.

Construction subscribes and reserves the state key before running user
initialization. Source subscribe-time, initializer-time, decoder-time and
component-factory events share the existing deferred FIFO, and completion occurs
after initial state has been published and normal event publication completes.
Panels initial on_state_changed precedes pumping those queued events.

Invalid initial models keep existing typed errors. Initializer panic propagates
the original panic. Existing guards cancel the source token, release reserved
providers, restore any consumed bytes and clean provisional retained scopes.
Source cancellation panic must not replace an existing initializer panic.
Successful external user callback side effects are not reversed. Captured
initializer resources are dropped when construction returns or unwinds, including
when restoration skips evaluation. Initializers may trigger parent destruction;
construction must return Destroyed and remove its incomplete registrations.

Custom-source cleanup before subscribe returns its token remains the source's
responsibility. Existing Essenty lifecycle panic delivery limitations remain.

## Implementation boundaries

Modify the five standard public factory modules and the private Stack/Panels
builders. No state-machine rewrite, new transport or public conversion trait.
One source owner avoids concurrent edits to shared adapters. Add a focused
tests/lazy_initial_state.rs suite and an executable example demonstrating a
non-Clone owned initializer and restoration suppressing it. Existing router,
extension-point and observable regressions must pass unchanged.

## Compatibility matrix

Create docs/COMPATIBILITY.md with the pinned upstream and exact Rust baseline,
row-level upstream source links, Rust source/test evidence and explicit status.
Cover ComponentContext/default/custom contexts, fixed/explicit child scopes,
lifecycle, state/retention, Back/predictive dispatch, all family models and helper
operations, lazy initial state, custom sources/navigator traits, Generic Navigation,
observable values and persistence. Separate runtime tests from compile targets.

Record Rust adaptations: Rc snapshots and identity CAS, linear Eq reconciliation,
typed Result completions, RAII cancellation, static navigator/object-safe source,
explicit bring_to_front_by grouping, per-router duplicates, caller codecs and
local wire formats. Do not claim binary compatibility with Kotlin serialization,
cross-target runtime execution or the entire upstream project being ported.

Record remaining API/scope gaps: standard restore-decline parity where absent,
memory-only transientNavStateSaver behavior, global DecomposeSettings/error hook,
public test-utils and platform/main-thread/UI/Web bindings. Classification must
be justified from source; a deliberately absent API cannot be marked implemented.
Internal upstream implementation classes need behavior coverage, not name copies.
Update README, roadmap and ADR-0013 with the matrix and actual verification.

## Acceptance and verification

1. All six lazy factory signatures compile with current named generic counts,
   dyn sources, borrowed FnOnce and non-Clone configuration/component/captures.
2. Every family evaluates once without restored state and zero times with valid
   restored state. Include authoritative empty/inactive cases and two-role Panels.
3. Destroyed/duplicate/invalid-restoration paths do not run the initializer;
   invalid models release keys and allow retry. Decoder errors preserve bytes.
4. Subscribe/initializer/factory events complete in FIFO after initialization;
   per-receiver completions and Panels initial hook order remain intact.
5. Initializer panic, parent destruction and combined cancellation panic leave
   no incomplete registration or lost retry input. Exercise capture Drop outside
   obstructing borrows, with isolated subprocesses for combined panic paths.
6. Existing eager calls and codecs behave unchanged; no new dependencies/bounds.
7. Fresh full all-target/all-feature tests, doctest, fmt, strict Clippy/rustdoc,
   all examples and all 24 installed library compile targets pass. Record actual
   counts rather than predict them. Independent spec review precedes quality review.
8. Every matrix status has source/behavior evidence or a clear gap/scope reason.
   Source links use the pinned revision; matrix conclusions are bounded to it.

The user reviewed and approved this design. Feature branch:
codex/lazy-initial-state-compatibility. No commit/merge of the implementation or
remote push is included unless subsequently requested by the user.
