# Audit remediation results

Date: 2026-10-11. Audited and implementation starting HEAD: `ddd8f69`.
The original audit report is preserved. Changes remain in the working tree;
no crate publication, commit, push or merge was performed.

## Finding-by-finding review

| Finding | Implemented result | Observable regression evidence | Boundary |
| --- | --- | --- | --- |
| 1. StateStore unregister loses state | Detach only the selected provider ownership; keep unrelated registrations and restored input in the actual keeper. Drop detached captures outside the shared borrow. | Nine store regressions cover reentrant reads, same-key replacement, nested unregister, restored consumption, callback families, duplicate semantics and panic preservation. Original failing cases were reproduced before the fix. | Applies to Decompose adapter registrations. Pre-existing raw Essenty providers cannot be detached through the published primitive API. |
| 2. Rollback can abort on a second panic | Shared Cleanup protocol independently completes preparation, scope ending, retention release, rollback, queued-event cancellation and teardown; preserve an active unwind, otherwise propagate the first cleanup panic. | Twelve cleanup regressions, including subprocesses for factory plus Destroy panic and multiple retained destructor panics, verify catchability, terminal stores, remaining cleanup, restored retries and survivor identity. Existing generic navigation panic tests also pass. | Arbitrary Essenty lifecycle listeners can stop primitive dispatch; opaque imported instance values cannot be independently drained. |
| 3. Lifecycle release drops instances under a borrow | Internal instance ownership guards allow terminal-store replacement under a short borrow followed by independent payload release. Root, removal, deactivation, retention and rollback use this boundary. | Actual lifecycle tests let destructors read is_destroyed successfully; supplied external ownership and configuration-change retention tests pass. | Imported raw payloads are dropped outside the borrow but lack per-payload panic isolation; externally held Rc objects drop when their final owner releases them. |
| 4. Current portable history | Optional schema version; pre-decoder saved-root gate; typed mismatch rejection/recreation; replace model and lifecycle bindings; rewrite current entry on attachment while preserving its managed index; track committed Back/Forward position. | Thirty history tests and seventeen versioning tests cover matching/missing versions, guards, different initial stack lengths, deferred and reentrant recreation, failure/panic recovery, late completion and observer cleanup. | Kotlin source was inspected, not executed. Root publication, fresh stores, storage and DOM bindings belong to the host. Legacy StackHistoryController remains distinct. |
| 5. Items quadratic work | Explicit stable-equality opt-in and collision-safe optional fingerprints reach validation, maps, normalization, snapshots and engine matching. Preserve default mutable Eq semantics and earliest-match behavior. | Twelve comparison/behavior regressions cover non-Hash/non-Clone Eq-only configurations, fresh equal allocations, collisions, mutable identity, component identity, lazy state, restoration, lifecycle and panic-invalidated indexes. | Default Eq-only remains quadratic. Constant fingerprints and collisions retain quadratic changed-list worst cases. |
| 6. pop_while root callbacks | Evaluate top through root, stop at first false, then clamp to a nonempty result. | Four tests cover shared trace fixtures, singleton/order, early termination and root predicate panic with retry. | Kotlin predicate source inspected; no differential Kotlin execution. |
| 7. Maintainability and automated checks | Central cleanup and detached ownership helpers, pinned MSRV/tools/targets, CI, consumed semantic fixtures, updated README/compatibility/ownership ADR and migration/performance documents. | All checks below executed locally; final diff reviewed against each finding. | Workflow added but GitHub Actions was not remotely run in this task. Cross-target checks establish compilation only. |

## Actual verification

All commands used RTK. Cargo ran on pinned Rust **1.96.0**, matching Cargo.toml
rust-version, using the four published Essenty 0.1.0 crates and thiserror 2.
The fresh verification target directory was `/tmp/decompose-audit-verification`.

| Check | Result |
| --- | --- |
| `rtk proxy cargo fmt --all -- --check` | Passed |
| `rtk proxy cargo clippy --all-targets --all-features --locked -- -D warnings` | Passed |
| `rtk proxy cargo test --all-targets --locked` | **575 unit/integration tests passed**, zero failures; all fifteen example test binaries compiled |
| `rtk proxy cargo test --doc --locked` | **One doctest passed** |
| `RUSTDOCFLAGS="-D warnings" rtk proxy cargo doc --no-deps --locked` | Passed |
| Run every `examples/*.rs` with `rtk proxy cargo run --locked --example NAME` | **All fifteen executed successfully** |
| `rtk proxy cargo check --lib --locked --target wasm32-unknown-unknown` | Passed |
| Same library check for `aarch64-linux-android` | Passed |
| Same library check for `aarch64-apple-ios` | Passed |
| `rtk proxy cargo package --locked --allow-dirty` | Package built and extracted crate verified against published dependencies; no publication |
| `rtk proxy git diff --check` | Passed |

Cargo commands additionally used `--target-dir /tmp/decompose-audit-verification`.
Local package verification needs `--allow-dirty` because implementation remains
uncommitted; clean-checkout CI uses `cargo package --locked`. Initial sandbox DNS
failure was followed by authorized network access for package verification.
The test inventory confirms 575 tests versus the audit baseline's 520: 55 added.
Subprocess helper tests are included in that total; the parent regressions
actually launch children and assert successful catch/recovery rather than merely
compiling their cases.

The final full run caught two existing generic-navigation regressions: retaining
a validation Rc kept displaced state alive until after publication. Releasing
that temporary ownership before preparation restored the existing destructor
ordering. Both focused tests and the complete suite then passed.
Independent review also exposed mutable-identity Items semantics and a replacement
history root left unobserved after the old root destructor panicked. Regressions
failed before those corrections and pass afterward.

## Measured equality work

One update; construction excluded. Actual values printed by
`rtk proxy cargo test --test items_performance --locked -- --nocapture`:

| n | Original/current default, inactive unchanged | Stable Eq-only opt-in, inactive unchanged | Stable Eq-only opt-in, all active unchanged | Dispersed index, all active reversed with fresh equal allocations |
| ---: | ---: | ---: | ---: | ---: |
| 256 | 32,640 | 256 | 1,024 | 1,153 |
| 512 | 130,816 | 512 | 2,048 | 2,305 |
| 1,024 | 523,776 | 1,024 | 4,096 | 4,609 |

No mandatory Hash, Ord or configuration Clone bounds were added. A default
configuration may mutate equality identity through shared interior state, so
caching its prior uniqueness would be unsound. Explicit stable identity permits
the fast path. Dispersed fingerprints provide expected linear equality work;
ordered lifecycle/retention bookkeeping adds O(n log n). Collision-heavy and
constant-fingerprint changed membership can still require O(n²). See
[the detailed contract](../items-performance.md).

## Reference and remaining limits

Upstream HEAD was checked with git ls-remote and was exactly
`0824a183f7fe3862a1409553e66346c999de7084`. History comparison covers immutable
webMain, JS, wasmJs and test sources at that revision; the general core reference
remains `00840bcfed6d55710c8fbb1a72503f2041eda490`. The shared TSV scenarios are
consumed by Rust tests and available to a future Kotlin harness. They do not
constitute executed Kotlin equivalence evidence.

Essenty 0.1.0 exposes neither provider detachment nor instance enumeration/drain.
Providers registered directly in raw Essenty before conversion or by explicit
primitive dereferencing retain primitive destructor behavior; Decompose cannot
extract their captures. Unregister still preserves unrelated state, but those
captures may observe a borrowed store. Multiple panicking opaque raw instance
payloads can still abort inside the primitive's own release. All adapter-managed
providers and instance entries use the new ownership mechanism. These residual
primitive cases are **not claimed fixed**; see [ADR-0020](../adr/0020-detached-ownership-and-panic-cleanup.md).

Secondary panic payloads are disposed under a catch boundary. If disposal itself
panics, its replacement payload is deliberately forgotten to prevent another
destructor unwind. This exceptional leak is confined to recursively panicking
panic payload destruction. Cleanup isolation assumes unwind panics; panic=abort
cannot be intercepted by catch_unwind.

Portable history gains a public HistoryState version field, so struct literals
must specify it; older host envelopes map absence to None. Router persistence
formats remain unchanged. The host must use the saved-version gate before
decoding root payloads, and owns component recreation/publication. No DOM,
sessionStorage, Android/Jetpack/Compose/SwiftUI adapter, async runtime, Send/Sync,
no_std or Kotlin/Rust serialized-byte compatibility was introduced or claimed.

Confidence: **high** for the explicitly executed Rust regressions and checks;
**moderate** for exhaustive behavioral equivalence because Kotlin was not run.
