# Audit remediation implementation plan

**Goal:** Correct every actionable finding against baseline `ddd8f69`, preserving unrelated audit files.

**Architecture:** Keep synchronous Rc ownership and published Essenty primitives. Add adapter ownership indirection and one cleanup protocol; keep history policy portable and indexed Items opt-in.

**Execution:** Independent history and Items domains run concurrently with ownership cleanup. Integrate and review in this workspace. No publish, merge or dependency source modifications.

- [x] State ownership (`src/state.rs`, `src/context/stores.rs`, `tests/store_unregister.rs`): reproduce lost replacement, incorrect reentrant reads, restored consumption, nested unregister and panic. Detach only the provider ownership token while preserving Essenty registration semantics. Run `rtk cargo test --test store_unregister --locked` before and after.
- [x] Cleanup (`src/cleanup.rs`, `src/router/engine.rs`, `src/retention.rs`): reproduce factory plus Destroy double panic in a subprocess. Catch independent cleanup steps, preserve active unwind and propagate first panic after cleanup otherwise. Verify retryable input, reservations and retained identity with `rtk cargo test --test panic_cleanup --locked`.
- [x] Instance ownership (`src/context.rs`, `src/context/stores.rs`, lifecycle regressions): make root, removal, deactivation, rollback and retention release share detach-before-drop behavior. Verify terminal state in destructors and independent release after panic. Record primitive API limits for pre-existing imported values.
- [x] History (`src/router/webhistory`, history tests): inspect exact upstream revisions and current HEAD; version-check before codecs; add portable recreation, cancellation and attachment index tracking. Test versions, legacy entries, guards, deferred/reentrant cancellation and failed recreation. Preserve legacy controller semantics.
- [x] Items (`src/router/items`, engine matching, deterministic count tests): measure baseline 256/512/1024 unchanged inactive entries. Add explicit stable-equality unchanged fast path preserving mutable default Eq semantics and optional collision-safe fingerprint indexing reaching engine paths. Verify identity, uniqueness, lifecycle, state and lazy creation.
- [x] Stack predicate (`src/router/stack/navigation.rs`, parity tests): call predicate top-to-root, preserve root, test stop order and root panic.
- [x] CI and documentation (`.github/workflows/ci.yml`, `rust-toolchain.toml`, README, compatibility matrix, ADR): pin declared MSRV 1.96.0, add strict checks, representative targets and package verification. Add portable semantic trace fixtures; distinguish Kotlin source review from execution.
- [x] Final verification: format, strict Clippy, all-target tests, doctests, strict rustdoc, all runnable examples, installed representative targets, package using published dependencies, subprocess/performance regressions, final diff and finding-by-finding review. Record commands and actual outcomes in `docs/audits/2026-10-11-remediation-results.md`.
