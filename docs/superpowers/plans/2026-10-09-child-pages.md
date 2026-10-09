# ChildPages implementation plan

> Use superpowers:subagent-driven-development, test-driven-development and verification-before-completion. One implementation owner for src/tests; controller owns documentation/examples. The user authorized continuation; do not add a further scope approval gate.

Design: `docs/superpowers/specs/2026-10-09-child-pages-design.md`.
Baseline: ChildSlot commit e293ab9; 181 passing tests. Branch codex/child-pages.

## 1. Implement the stateful common engine and Pages router

- Write failing behavioral tests before adding behavior. Adapt private source events to actual model state, preserving public Stack/Slot completion contracts.
- Extend model policy and managed items to live scopes and saved tombstones. Save before destruction, reconstruct with stable key and new keeper, preserve retention and panic/reentry cleanup.
- Add router/pages public types, policy, navigation operations, constructor and DPG1 codec. Keep one crate and old APIs/bytes.
- Add focused integration tests for the full design acceptance criteria, including hardcoded old-format fixtures and meaningful lifecycle/state failure scenarios.
- Run focused tests, then all existing tests; self-review changes and report uncertainties. Do not commit or edit root-owned docs/examples.

## 2. Integrate documentation and a runnable example

- Document Rust ownership, selected-index representation, default/custom statuses, tombstone recreation and codecs, and the pinned upstream reference.
- Add an example demonstrating lazy pages, selection away and back, saved state with a new instance and same key, and restored selection.
- Update roadmap and ADR to reflect the delivered behavior and next scope.

## 3. Independent reviews

- Dispatch a fresh read-only spec reviewer against the design and code, independently verify claims. Return concrete gaps to the sole implementer; repeat until approved.
- Then dispatch a fresh read-only quality reviewer for engine regressions, resource/lifecycle correctness, persistence and Rust API clarity. Fix and re-review substantive findings.

## 4. Final verification and delivery

- Run all-target and doc tests, fmt check, all-target all-feature strict Clippy, strict rustdoc, all router examples, and the repository's established cross-target checks.
- Record actual evidence and limitations in the plan. Deliver Slot commit hash, current branch, Pages behavior and validation; no automatic push/merge.

Progress: implementation, self-review, independent spec and quality reviews,
documentation, example and final verification complete.

## Verification evidence — 2026-10-09

Controller independently executed these commands after source/tests were frozen.
Rust 1.96.0, codex/child-pages, baseline e293ab9.

| Gate | Result |
|---|---|
| cargo test --all-targets --all-features | 220 passed: 115 unit, 39 Pages, 35 Slot, 27 Stack, 4 context integration |
| cargo test --doc --all-features | 1 passed; 221 total |
| cargo fmt --all -- --check | Passed |
| cargo clippy --all-targets --all-features -- -D warnings | Passed |
| RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features | Passed |
| cargo run --example child_pages | Lazy creation, same-key/new-instance counter recovery, Back and restored index 3 passed |
| cargo run --example child_stack / child_slot | Existing examples passed |
| cargo check --lib --target each installed target | All 24 passed; matrix is the same as the ChildSlot plan |

Added 39 Pages integration tests and an immutable pre-Pages DSL1 compatibility
fixture; all 181 baseline tests remain passing. The existing fixed DST1 fixture
also remains passing. No extra dependencies or Essenty source changes.

Independent SPEC PASS and QUALITY APPROVAL, each high confidence. The spec
reviewer independently ran the router integrations (101 passed), all-targets
tests (220) and doctest (1). The quality reviewer inspected callback ownership,
retention, parser, adapters and scope guards, and executed separate path-dependency
probes outside the project. A suspected cleanup gap around predictive Cancelled
panics was disproved: Essenty cancellation occurs before callback removal, so
old membership and Back survive and retries work. Failed tombstone resurrection
also preserved key/selection/state while ending provisional lifecycle and keeper.
No concrete correctness findings remained; no fixes followed the final gates.

git diff --check passed. ChildPages remains as completed local changes on
codex/child-pages; the user's commit request applied to the preceding completed
ChildSlot stage (e293ab9). No automatic merge, push or publication.
