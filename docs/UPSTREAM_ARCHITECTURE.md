# Upstream Architecture (Decompose @ `00840bc`)

Research identity (see final report in `PORTING_ROADMAP.md` for full pins):

- Upstream: `https://github.com/arkivanov/Decompose`, branch `master`
- SHA: `00840bcfed6d55710c8fbb1a72503f2041eda490` (2026-10-03, still HEAD at research time)
- Upstream versions: Decompose `3.6.0-beta01`, Essenty `2.7.0-alpha01`, Kotlin `2.3.21`,
  kotlinx-serialization `1.6.3`, kotlinx-coroutines `1.9.0` (test-only in core)

Evidence priority followed: tests > implementation > API dumps > docs > samples > README.
No docs/tests disagreement was found on load-bearing semantics.

## 1. Module graph

Gradle modules (from `settings.gradle.kts`):

```text
:decompose                        ← core + all navigation models (the port target)
:decompose-test-utils              ← TestComponentContext + test helpers (TEST ONLY)
:jetpack-component-context         ← Android/Jetpack bridge (PLATFORM INTEGRATION)
:extensions-compose                ← Compose UI extensions (UI EXTENSION)
:extensions-compose-experimental   ← experimental Compose extensions (UI EXTENSION)
:extensions-android                ← Android View extensions (PLATFORM INTEGRATION)
:sample:*                          ← SAMPLE ONLY (app-android/desktop/js/ios, shared, dynamic-features)
:tools:check-publication           ← build tooling only
```

`decompose/build.gradle.kts` core facts:

- `common.main` `api()` deps: `essenty-lifecycle`, `essenty-state-keeper`,
  `essenty-instance-keeper`, `essenty-back-handler`, `kotlinx-serialization-core`.
  (`api` = these types appear in public signatures; the Rust port must mirror this
  boundary with the four `essenty-* 0.1.0` crates.)
- `common.test`: `decompose-test-utils`, `kotlinx-coroutines-core` (test-only).
- `android.main`: `androidx.fragment:fragment-ktx` (integration only).
- `web.main`: `kotlinx-serialization-json`; `wasmJs.main`: `kotlinx-browser`.
- Source sets: `commonMain` holds ~all logic; `androidMain` (7 files), `darwinMain` (3),
  `linuxMain` (3), `jvmMain` (5), `jsMain`/`wasmJsMain`/`webMain` hold only platform
  actuals (`Lock`, `CheckMainThread`, `PrintError`, `Utils.uniqueName`) plus the
  browser-history bindings and `RetainedComponent`/deep-link Android bridges.

## 2. Module classification

| Module | Class | Rust consequence |
|---|---|---|
| `decompose` `commonMain` context + child-scope files | CORE | Port in Phase 1 |
| `decompose` `router/children` engine | NAVIGATION CORE | Port in Phase 2 (generic engine first) |
| `decompose` `router/stack`, `slot` | NAVIGATION CORE | Phases 3–4 |
| `decompose` `router/pages`, `panels`, `items` (`pages` stable, `panels`/`items` experimental) | NAVIGATION CORE | Phase 5 |
| `decompose` `router/webhistory` + `webMain/jsMain/wasmJsMain` history | NAVIGATION CORE + platform binding | Phases 7–8; browser binding is a future adapter |
| `decompose` `value/` (`Value`, `MutableValue`, `map`) | CORE | Port in Phase 2 (navigation needs it); nothing else in core needs it |
| `decompose` `Relay`, `Cancellation`, `Ref`, `Lock`, `Utils`, `GettingList` | CORE / KOTLIN-SPECIFIC | `Relay`+`Cancellation` port in Phase 2; `Ref`/`Lock`/`GettingList` are Kotlin-specific, not applicable |
| `decompose` `errorhandler`, `DecomposeSettings`, `DecomposeExperimentFlags`, `mainthread` | CORE (policy) | Port a minimal settings/error-hook subset in Phase 1; thread check is platform-adapter concern |
| `decompose` `androidMain` bridges, `RetainedComponent`, deep links | PLATFORM INTEGRATION | Not applicable to core; future adapters |
| `decompose-test-utils` | TEST ONLY | One `decompose` crate + `#[cfg(test)]` helpers first; separate crate only if evidence later demands it |
| `jetpack-component-context` | PLATFORM INTEGRATION | Do not port; only the delegate-and-rewrap factory *pattern* transfers (see §5) |
| `extensions-compose*`, `extensions-android` | UI EXTENSION / PLATFORM INTEGRATION | Out of scope until Phase 9 |
| `sample:*` | SAMPLE ONLY | Reference material only |
| `kotlin-js-store`, `tools`, `docs`, `gradle-setup-plugin` | build/tooling | Ignore |

## 3. Core dependencies (`decompose` `commonMain`)

- **Essenty** (`lifecycle`, `state-keeper`, `instance-keeper`, `back-handler`): the four
  owner interfaces composed by `GenericComponentContext`; child scoping is implemented
  in Decompose (`MergedLifecycle`, `ChildStateKeeper`, `ChildInstanceKeeper`,
  `ChildBackHandler`), not in Essenty. See `ESSENTY_MAPPING.md`.
- **kotlinx.serialization-core**: pervasive in navigation *only* (`ChildStackFactory`,
  `ChildrenFactory`, `ChildrenNavigator`, `NavStateSaver`, `Pages`/`Panels`/`Items`
  state models, web-navigation adapters). Pattern is uniform: optional `KSerializer<C>`
  (or `NavStateSaver` / save-restore fn pair) per `children()` call; `null` = transient
  only. Core context/child-scope code does not touch serialization except via the opaque
  `SerializableContainer` wrapper.
- **kotlinx.coroutines**: zero imports in main source sets (only `nonWebTest` harness).
  Deliberate — the `Value` docstring states it exists "to avoid direct dependency on
  coroutines/Reaktive".
- **Reaktive**: zero imports anywhere. `Value`/`Relay`/`Cancellation` are the hand-rolled
  replacements.
- **Compose / AndroidX**: zero imports in `decompose` main source except the documented
  `androidMain` AndroidX bridges. All animation/predictive-back controllers live in
  `extensions-compose*`.
- **Other Ark Ivanov libraries**: only Essenty (above) and the Gradle setup plugin
  (build-time only).

## 4. Navigation families at HEAD (lookahead only, not ported)

| Family | State | Navigator | Notes |
|---|---|---|---|
| Generic `children` engine (`router/children`) | `NavState` + `ChildNavState` (`DESTROYED/CREATED/STARTED/RESUMED`) | `SimpleNavigation` + internal `ChildrenNavigator` | Foundation for stack/slot/pages/panels; `ChildController` is a parallel keyed engine used only by `items` |
| Stack | `ChildStack` (active + backStack) | `DefaultStackNavigation` + `StackNavigatorExt` (push/pop/replace/bringToFront/…) | `Child.Created` vs `Child.Destroyed` tombstones; `StackWebNavigation` adapter |
| Slot | `ChildSlot` (optional child) | `DefaultSlotNavigation` (`activate`/`dismiss`) | No web adapter at HEAD |
| Pages | `Pages`/`ChildPages` (items + selectedIndex) | `DefaultPagesNavigation` | Neighbor `pageStatus` policy; `PagesWebNavigation` |
| Panels | `Panels`/`ChildPanels` + mode SINGLE/DUAL/TRIPLE | `DefaultPanelsNavigation` | All `@ExperimentalDecomposeApi`; `PanelsWebNavigation` |
| Items (lazy) | `Items`/`ChildItems`/`LazyChildItems` | `DefaultItemsNavigation` + `ItemsController` | All experimental; per-item lifecycle map; no web adapter |
| Web history | `WebNavigation`/`WebNavigationOwner` | `enableWebHistory` tree sync (`webMain`) | Experimental; browser binding is platform code |
| Predictive back | core hook-forwarding only | gesture controllers in `extensions-compose*` | Core contribution = `DefaultChildBackHandler` forwarding |

Status→scope actuation the engine will later require: RESUMED ⇒ resume + back start;
STARTED ⇒ start + back start; CREATED ⇒ create/step-down + back stop; DESTROYED ⇒ save,
destroy lifecycle + instance scope, keep re-instantiable tombstone under the same key.
Keys are minted once per child and preserved across navigations and process restore.

## 5. Jetpack bridge (not ported)

`jetpack-component-context` wraps a plain `ComponentContext`, delegates the four Essenty
owners, mirrors the Essenty lifecycle into a Jetpack `LifecycleRegistry`, persists the
Jetpack `SavedStateRegistry` through the Essenty `StateKeeper` under one stable key, and
retains the `ViewModelStore` via `retainedInstance`. The transferable insight is the
**delegate-and-rewrap factory pattern** (extra capabilities closed over in the factory
lambda) — which is also the documented custom-context recipe (`docs/component/custom-component-context.md`).
Everything else (ViewModels, Parcel/Bundle `SavedStateSerializer`, `expect/actual`) is
Android/Jetpack- or Kotlin-specific and must not enter the Rust core.

## 6. Test utilities

`decompose-test-utils` provides `TestComponentContext` (defaults: RESUMED lifecycle, fresh
dispatchers, `DefaultComponentContext` factory) with `recreate(isConfigurationChange)`
simulating process death (JSON round-trip) vs config change (retain `InstanceKeeper`),
plus reified `StateKeeper` helpers. The Rust port starts with `#[cfg(test)]` helpers
inside the single crate; a separate crate is unjustified until reuse evidence appears.
