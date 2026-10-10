# Porting Roadmap

> Current delivery (2026-10-10): the selected independent Decompose core is
> complete against pinned revision `00840bcfed6d55710c8fbb1a72503f2041eda490`.
> Contexts, values, five standard families and Generic Navigation now include
> selective/full-state persistence, owned memory snapshots, current-thread
> settings/diagnostics, public test utilities and both portable history algorithms.
> See [COMPATIBILITY.md](COMPATIBILITY.md), ADR-0014 through ADR-0018 and the
> [completion plan](superpowers/plans/2026-10-10-independent-core-completion.md)
> for source mappings, test evidence and remaining integration boundaries.
> This delivery extends `f3886cd` on `codex/lazy-initial-state-compatibility`;
> local commits only. The original phase research below is historical.
> DOM/wasm host wiring, Android/Jetpack, Compose, SwiftUI, Reaktive and other
> UI/platform adapters remain outside scope, as do thread-safe mirrors and no_std.
> Kotlin serialized-byte migration is explicitly outside the required contract.

## Research identity

```text
Upstream repository: https://github.com/arkivanov/Decompose
Upstream branch:     master
Pinned upstream SHA: 00840bcfed6d55710c8fbb1a72503f2041eda490 (verified HEAD, 2026-10-03)
Upstream date:       2026-10-03

Essenty-rs version:  0.1.0 (essenty-lifecycle/state-keeper/instance-keeper/back-handler)
Essenty release SHA: 2bf0e9108d8ae2dcd67ac3ab7ea7ca54cae16092 (tag v0.1.0, 2026-09-24)

Research workspace:  /Users/yet/development/rust/decompose (local git, branch
                     codex/phase-0-decompose-research; upstream-ref/ is a gitignored
                     reference clone, never committed)
Phase 0.5 remediation (2026-10-04): Clone bound removed; parent-held scope lifetime;
                     strong state provider; manual-destroy panic contract; &self
                     receiver; key validation removed. Prototypes: /tmp/dc-proto-a,
                     /tmp/dc-proto-b (throwaway, real essenty-* 0.1.0 deps).
Phase 0.6 remediation (2026-10-04): trait fifth member is context_factory
                     (from_parts off-trait); SharedBack reentrancy wrapper;
                     ContextScope removed as REDUNDANT; DCS1 framing; construction
                     order + rollback; root attachTo; merged driver rules; minimized
                     errors; ChildKey crate-private. Prototype: /tmp/dc-proto-c
                     (F-01 nesting, 9 back cases, framing, rollback — all executed).
Git mutation performed: NONE on any remote. Local-only repo init + research docs.
                        No pushes, no deletions, no rewrites of yet300/Decompose.
```

## Required architecture decisions

| Decision | Status | Outcome |
|---|---|---|
| ComponentContext trait vs struct | DECIDED (0.6: factory member, no `from_parts`) | Trait (`Sized` only): 4 accessors + `context_factory()` + provided `child_context` (ADR-0001) |
| Generic/custom context strategy | DECIDED | Single generic trait; custom contexts delegate + close over services; non-`Clone` allowed; factory computed per call (no cycles) |
| ComponentContextFactory representation | DECIDED (0.6: whole-parts) | `Rc<dyn Fn(ContextParts) -> C>`; no public fields; delegate-don't-destructure (ADR-0002) |
| Ownership model | DECIDED (0.6: no ContextScope) | Parent-held strong captures; lifecycle-driven teardown; `ContextScope` REDUNDANT (ADR-0003) |
| Child lifecycle representation | DECIDED (0.6: driver rules) | `MergedLifecycle`; init→destroyed emits `Created,Destroyed`; terminal idempotent (ADR-0003) |
| StateKeeper child namespace | DECIDED (0.6: DCS1 framing) | Adapter: consume-seed + strong-Rc framed provider + unregister-on-destroy |
| InstanceKeeper child namespace | DECIDED (0.6: `get_or_create`) | Adapter: retained scope via `get_or_create` (`ptr_eq` identity) + `destroy`-on-destroy; Drop-timing difference documented |
| BackHandler child bridge | DECIDED (0.6: SharedBack) | `SharedBack` busy-window + FIFO queue; single parent callback; `Weak` listener link (ADR-0004) |
| Error strategy | DECIDED (0.6: minimized) | 6-variant `DecomposeError` (+`BackDispatchInProgress`); manual-destroy panics (fires last), creation-destroyed is `Err` |
| Key strategy | DECIDED (0.6: crate-private) | `pub(crate) ChildKey`; `&str` boundaries; uniqueness only — `""` legal; deterministic |
| `child_context` receiver | DECIDED (0.5) | `&self` via `Rc<RefCell<…>>` short borrows; never across callbacks |
| Observable state abstraction | IMPLEMENTED | Rc-backed `Value<T>`/`MutableValue<T>`, cached map, lifecycle bindings, identity CAS/update (ADR-0011); no runtime or payload Clone/Eq bounds |
| Serialization boundary | IMPLEMENTED | Explicit byte codecs, selective and whole-state restoration, owned memory-only snapshots; no new serialization dependency (ADR-0014/0015/0018) |
| Crate layout | DECIDED | One `decompose-rs` crate through Phase 5; no premature splits |
| Threading model | DECIDED | `!Send + !Sync` core; thread-safe mirrors are future new types, never bounds |
| Essenty dependency style | DECIDED | Four primitive crates directly; no umbrella, no Tokio in core |
| Repository topology | DECIDED | Option A (see REPOSITORY_STRATEGY.md) |
| Crate naming | DECIDED | `decompose-rs` (`decompose` taken, `-rs`/`-core` free) |
| `no_std` | DECIDED | NOT A GOAL |

## Navigation prerequisites split

**Needed in Phase 1** (without these no navigation model can exist): context shape
(lifecycle + 3 keepers/dispatchers + detached factory); per-child scope construction
(fresh merged lifecycle, seeded state dispatcher, retained-or-new instance scope, forked
back handler); stable caller keys + duplicate rejection; synchronous reentrant execution
posture; teardown ordering guarantees; `DEFAULT_BACK_PRIORITY = 0` + LIFO-tiebreak contract.

**Needed later**: status→scope actuation matrix (Phase 2 engine); `Created` vs `Destroyed`
tombstones + same-key re-instantiation (Phase 2); positional save/restore + `NavStateSaver`
(Phase 2, refined Phase 6); `Value` observation (Phase 2); per-family policies — stack
top=RESUMED/back=CREATED (Phase 3), slot optional-child (Phase 4), pages neighbors/panels
modes/items active-maps (Phase 5); web history (Phases 7–8); UI bindings (Phase 9).

## Phased plan (dependency-ordered)

```text
Phase 1  ComponentContext foundation: trait + DefaultComponentContext + factories,
         MergedLifecycle, 3 child adapters, TestContext harness, 12 parity suites.
Phase 2  Generic children engine + observable Value/MutableValue/Relay/Cancellation.
Phase 3  ChildStack.   Phase 4  ChildSlot.
Phase 5  Other models (Pages, Panels, Items) in upstream-dependency order.
Phase 6  State-restoration + serialization refinement (codecs, NavStateSaver).
Phase 7  Predictive-back integration.   Phase 8  Web history.
Phase 9  UI/platform integrations.
```

Ordering rationale: each phase's contract is exercised by the next (context → engine →
families → persistence → gestures → platforms). Serialization is deliberately late: transient
navigation must work before persistence is refined.

## Phase 1 acceptance gate

```text
ComponentContext API implemented (5-method trait + provided child_context + default
  + non-Clone service-bearing custom-context support; 3-generation nesting test green)
child_context(&self) (+with_lifecycle, +priority) via context_factory; normative 9-step order
scope-survival + strong-provider ownership tests green (zero-external-refs save)
MergedLifecycle / child-StateKeeper (DCS1) / child-InstanceKeeper (get_or_create ptr_eq)
  / SharedBack (9 reentrancy cases) parity green
predictive-back delegation covered (cancel + invoke flows, mid-gesture immunity)
construction atomicity + factory-panic rollback green; root attachTo green
manual-destroy catch_unwind panic test green (fresh fixtures); creation-destroyed Err green
no mandatory Tokio; no mandatory Send + Sync; no platform types in core
registry-only Essenty dependencies (4 primitives)
21-target compile matrix if practical for core, else documented subset
cargo fmt --check / clippy -D warnings / test / doc all clean on Rust 1.96.0
```

## Risks / blockers

No architectural blockers. Concrete (non-blocking) risks owned by Phase 1:

1. `RefCell` discipline in reentrant dispatch — mitigated structurally: keepers use
   snapshot-before-notify with short borrows (executed, no panics); back dispatch goes
   exclusively through `SharedBack`'s busy window (nine executed cases, no panics).
2. `Rc` allocation/traffic on hot paths (observer lists, per-child scopes, per-call factory
   `Rc`, queued back ops) — acceptable for UI-tree rates; API keeps handles cheap to clone,
   internals can pool later without breakage.
3. `BTreeMap<String, …>` key costs inherited from Essenty — deterministic ordering is worth it;
   keys are short and few per scope.

## Phase-2 forward compatibility re-check (Phase 0.6, against pinned sources)

Re-inspected `DefaultChildItemFactory` (builds each child exclusively through
`contextFactory(lifecycle, stateKeeper, instanceKeeper, backHandler)` and stores the
resulting `Ctx` by value in `ChildItem.Created`), `ChildItem` (`Created` holds the
instance; `Destroyed` holds configuration + key + saved state, no instance),
`ChildrenFactory`/`ChildrenNavigator` (generic over the context type; positional
save/restore; tombstone re-instantiation under the same key), and `ChildController`
(keyed create/start/resume with per-item back priorities). Proven consequences:

- `C: Clone` still unnecessary: the engine moves owned `C` values into items and reuses
  `Created` items across navigations; nothing clones a context.
- `ContextFactory<C>` is sufficient: it is exactly the seam the engine already calls.
- Service-bearing custom `C` creates arbitrary descendants: construction flows through
  `context_factory()`, which closes over services per generation (executed: 3 generations).
- Inactive children remain representable: `Destroyed`-style tombstones hold no `C` and no
  live scope — only key + saved `DCS1`-framed bytes.
- CREATED/STARTED/RESUMED/DESTROYED actuation works through the owned lifecycle handles;
  same-key recreation replays consume-seed + `get_or_create` identity.
- No new public API redesign is knowingly required for Phase 2.

## Completion status

Independent core completion supersedes earlier deferred research scope: portable
state/settings/test-utils/history are delivered; concrete platform adapters remain
excluded. Verification commands and results are in the completion plan.

```text
PHASE 0 RESEARCH: PASSED
COMPONENT CONTEXT: IMPLEMENTED WITH STATE/RETENTION CORRECTIONS (ADR-0005)
CHILD STACK: IMPLEMENTED (operations, Back, reentrancy, codecs)
CHILD SLOT: IMPLEMENTED (optional membership, Back, reentrancy, empty/populated persistence)
CHILD PAGES: IMPLEMENTED (selected-index state, live/destroyed instances, lazy recreation, Back, DPG1 persistence)
CHILD PANELS: IMPLEMENTED (typed roles, lifecycle modes, independent dismissal, Back and DPN1 persistence)
CHILD ITEMS: IMPLEMENTED (configuration-keyed activity, lazy instances, receiver-local navigation, DPI1 persistence)
GENERIC NAVIGATION: IMPLEMENTED (custom NavState/events, neutral child types, DCH1 persistence)
OBSERVABLE VALUES: IMPLEMENTED (cached map, lifecycle observation, identity CAS/update)
STANDARD ROUTER EXTENSION POINTS: IMPLEMENTED AND MERGED (typed events, custom sources, navigator traits)
LAZY STANDARD INITIAL STATE: IMPLEMENTED AND VERIFIED (six FnOnce factory variants; eager calls preserved)
COMPATIBILITY MATRIX: COMPLETE FOR PINNED REVISION (source/test evidence, Rust adaptations, excluded integrations)
SELECTIVE / WHOLE-STATE PERSISTENCE: IMPLEMENTED (ADR-0014/0018; old formats preserved)
MEMORY NAVIGATION: IMPLEMENTED (ADR-0015; owned nested snapshots, no disk storage)
SETTINGS / PUBLIC TEST UTILITIES: IMPLEMENTED (ADR-0017)
PORTABLE RECURSIVE AND LEGACY HISTORY: IMPLEMENTED (ADR-0016; explicit host backend)
VERIFICATION: 509 unit/integration tests, 1 doctest, 15 examples, 24 compile targets; fmt/Clippy/rustdoc pass
PLATFORM INTEGRATIONS: OUTSIDE APPROVED SCOPE
ESSENTY-RS CHANGES: NONE REQUIRED
```
