# Independent Core Completion Implementation Plan

> For agentic workers: use superpowers:subagent-driven-development and
> superpowers:dispatching-parallel-agents for independent work, with root
> integration and verification. Design is already authorized by the user.

**Goal:** Close every functional gap in the pinned independent Decompose core.

**Architecture:** Preserve the shared navigation engine and byte formats; add
memory snapshots in the existing state adapter, additive codec predicates,
scoped settings, public test context and host-independent history.

**Tech Stack:** Rust 2024, Rc/RefCell, existing essenty 0.1.0 crates.

- [x] Selective persistence: standard codec/state/model files; integration tests
  first (decline, skip, corruption, empty state, nested provider gating,
  initializer, retention and construction rollback); runnable example; ADR 0014.
- [x] Memory state: src/state.rs, src/state/memory.rs, src/router/engine.rs,
  src/child.rs; tests/memory_navigation.rs first; owned snapshots with nested
  memory values, key preservation, cleanup/type mismatch/rollback; ADR 0015.
- [x] Settings/test utils: src/settings.rs, src/test_utils.rs, duplicate defaults,
  shared diagnostics; tests/settings.rs and tests/test_utils.rs first; examples;
  ADR 0017. Preserve explicit bool overrides and error propagation.
- [x] History: src/router/webhistory, recursive source/backend interfaces,
  standard model adapters; tests/web_history.rs first; example; ADR 0016.
- [x] Audit every portable public declaration in pinned upstream against exports
  and tests, including model/Value/context helpers beyond existing matrix.
- [x] Integrate and review shared changes, regressions and cleanup/reentry tests.
- [x] Run `rtk cargo test --all-targets --all-features`, `rtk cargo test --doc`,
  `rtk cargo fmt --check`, strict Clippy and rustdoc; run examples and applicable
  installed cross targets. Record actual outcomes and practical limits.
- [x] Update README, COMPATIBILITY and PORTING_ROADMAP, create local commits;
  verify clean final status. No push or main merge.


## Final implementation and independent review

The source audit covered commonMain contexts, Child/keys, each model/factory/
navigator/source/helper group, Generic state/savers, settings/error handlers,
values/operators and public test utilities, plus both webMain history algorithms.
It found additional true omissions beyond the starting matrix: whole-state
Pages/Items/Panels callbacks, nullable Slot restoration and the older public
Stack WebHistoryController. They are implemented, with ADR-0018 and the separate
StackHistoryController behavior documented in ADR-0016. Kotlin overloads and
syntax-only conveniences are mapped to Rust APIs; actual OS/UI integrations
are excluded in COMPATIBILITY.md.

Independent core and history reviews found and verified fixes for snapshot
last-good byte cache loss, queued Back vetoes using stale host positions,
asynchronous restoration panic recovery, cancellation of recursive late
restoration and replacement of a child under unchanged parent keys. Root
regressions additionally verified provisional memory input during construction,
several custom cancellation panics without process abort, and nested memory
under a persistent outer router. The full-state codec review found no actionable
issues after these changes. No Kotlin differential test suite was executed.

## Final validation (2026-10-10)

Reference clone HEAD independently verified:
`00840bcfed6d55710c8fbb1a72503f2041eda490`.
Rust toolchain: 1.96.0. No Cargo.toml/Cargo.lock or Essenty version changes;
no new feature gates, runtime or direct serialization/UI dependency.

| Command / check | Actual outcome |
|---|---|
| `rtk cargo test --all-targets --all-features` | 509 unit/integration tests passed; no failed tests |
| `rtk cargo test --all-targets --no-default-features` | Same 509 passed; all public new facilities available without feature flags |
| `rtk cargo test --doc --all-features` | 1 doctest passed |
| `rtk cargo fmt --all -- --check` | Passed |
| `rtk cargo clippy --all-targets --all-features -- -D warnings` | Passed, no warnings |
| `rtk proxy env RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --all-features` | Passed |
| `rtk cargo run --offline --example NAME` for every example | All 15 passed |
| `rtk cargo check --lib --all-features --offline --target TARGET` | All 24 installed targets passed |
| `rtk git diff --check` | Passed |

New suites contribute 81 tests beyond baseline 428: selective_persistence 11,
memory_navigation 15, settings 8, test_utils 5, web_history 29 and
whole_state_codecs 13. Old byte fixtures and all existing API/behavior suites
remain in the complete run. Some panic regressions deliberately spawn a child
test process; its successful exit verifies preservation of the original unwind
instead of a double-panic abort.

Runnable examples:
- `child_items` — passed
- `child_pages` — passed
- `child_panels` — passed
- `child_slot` — passed
- `child_stack` — passed
- `core_settings` — passed
- `generic_navigation` — passed
- `lazy_initial_state` — passed
- `memory_navigation` — passed
- `observable_values` — passed
- `router_extension_points` — passed
- `selective_persistence` — passed
- `test_context` — passed
- `web_history` — passed
- `whole_state_persistence` — passed

Installed library compile targets:

- `aarch64-apple-darwin` — passed
- `aarch64-apple-ios` — passed
- `aarch64-apple-ios-macabi` — passed
- `aarch64-apple-ios-sim` — passed
- `aarch64-apple-tvos` — passed
- `aarch64-apple-tvos-sim` — passed
- `aarch64-apple-visionos` — passed
- `aarch64-apple-visionos-sim` — passed
- `aarch64-apple-watchos` — passed
- `aarch64-apple-watchos-sim` — passed
- `aarch64-linux-android` — passed
- `aarch64-pc-windows-msvc` — passed
- `aarch64-unknown-linux-gnu` — passed
- `aarch64-unknown-linux-musl` — passed
- `armv7-linux-androideabi` — passed
- `i686-linux-android` — passed
- `wasm32-unknown-unknown` — passed
- `x86_64-apple-darwin` — passed
- `x86_64-apple-ios` — passed
- `x86_64-apple-ios-macabi` — passed
- `x86_64-linux-android` — passed
- `x86_64-pc-windows-msvc` — passed
- `x86_64-unknown-linux-gnu` — passed
- `x86_64-unknown-linux-musl` — passed

These are library compile checks, not runtime validation on each OS/UI/browser.
The native test backend verifies history behavior; no DOM/browser adapter is
present. Kotlin serialization compatibility is intentionally not promised.

All delivered changes are local commits on codex/lazy-initial-state-compatibility.
No push or merge was performed. Confidence is high for verified bounded Rust
contracts and moderate for exhaustive cross-language behavioral equivalence.
