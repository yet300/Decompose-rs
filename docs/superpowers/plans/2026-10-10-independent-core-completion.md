# Independent Core Completion Implementation Plan

> For agentic workers: use superpowers:subagent-driven-development and
> superpowers:dispatching-parallel-agents for independent work, with root
> integration and verification. Design is already authorized by the user.

**Goal:** Close every functional gap in the pinned independent Decompose core.

**Architecture:** Preserve the shared navigation engine and byte formats; add
memory snapshots in the existing state adapter, additive codec predicates,
scoped settings, public test context and host-independent history.

**Tech Stack:** Rust 2024, Rc/RefCell, existing essenty 0.1.0 crates.

- [ ] Selective persistence: standard codec/state/model files; integration tests
  first (decline, skip, corruption, empty state, nested provider gating,
  initializer, retention and construction rollback); runnable example; ADR 0014.
- [ ] Memory state: src/state.rs, src/state/memory.rs, src/router/engine.rs,
  src/child.rs; tests/memory_navigation.rs first; owned snapshots with nested
  memory values, key preservation, cleanup/type mismatch/rollback; ADR 0015.
- [ ] Settings/test utils: src/settings.rs, src/test_utils.rs, duplicate defaults,
  shared diagnostics; tests/settings.rs and tests/test_utils.rs first; examples;
  ADR 0017. Preserve explicit bool overrides and error propagation.
- [ ] History: src/router/webhistory, recursive source/backend interfaces,
  standard model adapters; tests/web_history.rs first; example; ADR 0016.
- [ ] Audit every portable public declaration in pinned upstream against exports
  and tests, including model/Value/context helpers beyond existing matrix.
- [ ] Integrate and review shared changes, regressions and cleanup/reentry tests.
- [ ] Run `rtk cargo test --all-targets --all-features`, `rtk cargo test --doc`,
  `rtk cargo fmt --check`, strict Clippy and rustdoc; run examples and applicable
  installed cross targets. Record actual outcomes and practical limits.
- [ ] Update README, COMPATIBILITY and PORTING_ROADMAP, create local commits;
  verify clean final status. No push or main merge.
