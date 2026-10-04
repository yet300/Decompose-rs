# Porting Roadmap

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
Git mutation performed: NONE on any remote. Local-only repo init + research docs.
                        No pushes, no deletions, no rewrites of yet300/Decompose.
```

## Required architecture decisions

| Decision | Status | Outcome |
|---|---|---|
| ComponentContext trait vs struct | DECIDED | Trait + `DefaultComponentContext` struct (ADR-0001) |
| Generic/custom context strategy | DECIDED | Single generic trait; custom contexts delegate + close over services |
| ComponentContextFactory representation | DECIDED | `Rc<dyn Fn(ContextParts) -> C>` closures first (ADR-0002) |
| Ownership model | DECIDED | `Rc` handles + `&mut`-at-birth + RAII guards + `Weak` in parent-registered closures (ADR-0003) |
| Child lifecycle representation | DECIDED | `MergedLifecycle` struct over Essenty primitives (min-state + sticky destroy) |
| StateKeeper child namespace | DECIDED | Adapter in Decompose: consume-seed + register-save + unregister-on-destroy |
| InstanceKeeper child namespace | DECIDED | Adapter: retained `ChildScope` + remove-on-destroy; Drop-timing difference documented |
| BackHandler child bridge | DECIDED | Adapter: single parent callback, `gate && can_handle`, 1:1 predictive forwarding |
| Error strategy | DECIDED | Typed `DecomposeError` + `Result`; panic only for async-path contract violations |
| Key strategy | DECIDED | Owned-`String` `ChildKey`; positional namespaces; deterministic, never random |
| Observable state abstraction | DEFERRED WITH REASON | Phase 2 with the engine; leading candidate `Rc`-backed `Value<T>`; no Tokio |
| Serialization boundary | DEFERRED WITH REASON | Caller-supplied codecs to opaque bytes; no `serde` in core; revisit Phase 6 |
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
ComponentContext API implemented (trait + default + custom-context support)
child_context (+with_lifecycle, +priority) implemented
MergedLifecycle / child-StateKeeper / child-InstanceKeeper / child-BackHandler parity green
predictive-back delegation covered
no mandatory Tokio; no mandatory Send + Sync; no platform types in core
registry-only Essenty dependencies (4 primitives)
21-target compile matrix if practical for core, else documented subset
cargo fmt --check / clippy -D warnings / test / doc all clean on Rust 1.96.0
```

## Risks / blockers

No architectural blockers. Concrete (non-blocking) risks owned by Phase 1:

1. `RefCell` discipline in reentrant dispatch (borrow across observer delivery panics) —
   mitigated by snapshot-before-notify + parity reentrancy tests.
2. `Rc` allocation/traffic on hot paths (observer lists, per-child scopes) — acceptable for
   UI-tree rates; API keeps handles cheap to clone, internals can pool later without breakage.
3. `BTreeMap<String, …>` key costs inherited from Essenty — deterministic ordering is worth it;
   keys are short and few per scope.

## Completion status

```text
PHASE 0 RESEARCH: PASSED
COMPONENT CONTEXT DESIGN: READY FOR IMPLEMENTATION
NAVIGATION IMPLEMENTATION: NOT STARTED
ESSENTY-RS CHANGES: NONE REQUIRED
```
