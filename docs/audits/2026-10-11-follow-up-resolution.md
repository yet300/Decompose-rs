# Follow-up remediation resolution

Date: 2026-10-11. Confidence: high for the executed Rust regressions.
The original audit, remediation report and follow-up review remain historical evidence.

## Reviewable baseline

`439af8d17e948ee7324f3626bc6cdb27d21bc905` —
`fix: implement Decompose audit remediation and regressions`.
Committed before modifying any follow-up defect. Fresh-target verification in
`/tmp/decompose-followup-20261011` passed 575 unit/integration tests, one doctest,
formatting, strict Clippy and strict rustdoc. The baseline includes the review
documenting all three unresolved findings; it does not resolve them.
The two historical reports retain their original trailing blank lines.

## Accepted navigation cancellation

Every engine cancellation path now reports `NavigationError::ChildUnavailable`
for accepted abandoned events. Removing the optional silent-release mode makes
Back unwind, parent teardown and ordinary pump unwind share one contract.
Completion, transformer destruction and completion-capture destruction remain
independent Cleanup actions. The first cleanup panic propagates after cleanup;
an original unwind remains authoritative.

`tests/navigation_cancellation.rs`: four regressions failed against the committed
baseline for the expected missing failure completion; all five tests pass after
the correction. Parent Destroy and Back panic leave the active configuration at
0 and resolve the retained Pending receipt to `Err(ChildUnavailable)`, exactly
once. A second receiver's successful transition remains committed while the
receipt reports the canceled receiver's error. Navigation retries successfully
after Back unwind and on the surviving receiver. Three abandoned callbacks all
run and release their captures even when two callbacks panic; the original Back
panic is preserved. A disconnected source still legitimately returns `Ok([])`.

No public API changed for navigation cancellation.

The navigation fix is committed as
`a7e998d9fd9e6d59786be3307eacdd92a0c106ac` —
`fix: report failure for accepted cancelled navigation`. The updated existing
Panels construction regression now observes failure completion explicitly
instead of relying on silent cancellation from a panic-on-error helper.
Its verified snapshot passed 580 unit/integration tests, one doctest, formatting,
strict Clippy and strict rustdoc. The two subsequently added failing history
reproductions were excluded from this navigation-only verification.

## Queued history destination URL

The original reproduction failed before correction with requests `["/1", "/9"]`.
`queued_recreation_uses_original_pop_url_and_preserves_latest_host_position` now
asserts `["/1", "/2"]`, host position1 throughout completion, exact replacement
URLs `["/9", "/8", "/3"]`, and subsequent Forward restoring key3.

Pop events capture their complete destination when notified. Both FIFO queues
and one-shot recreation completions retain that snapshot; recreation requests
never read a later `backend.current()` URL. Several queued destinations and
synchronous completion followed by reentrant Forward receive their original
URLs in order. Failure drains later queued targets while preserving host position.

## Late unmanaged history overwrite

The original reproduction failed before correction because `/external` became
`/9`. `late_recreation_preserves_unmanaged_entry_and_updates_actual_managed_destination`
now asserts the entire unmanaged entry is unchanged, the intended managed
physical entry becomes `/9`, `/3` remains intact and host position remains0.
Subsequent Forward reaches the corrected entry and restores key9.

An absent managed state now produces an explicit error for model writes, never
managed index0. Destination replacement validates stable host identity and the
original entry snapshot without moving the host. Tests cover duplicate/gapped
managed indices, failure and model changes while unmanaged, changed and discarded
destinations, a new branch with identical bytes/URL in a reused physical slot,
reentrant listener writes, cancellation, controller drop and late root release.
Existing panic, lifecycle, recursive and legacy history regressions remain active.

Independent review additionally reproduced repeated `/1 → /2 → /1` visits and
reentrant error-handler navigation overtaking queued work. Both new tests failed
before their corrections. A separate expected snapshot now advances only after
a matching successful controller write, preserving original request metadata.
Older pending Pops return to the front of the event queue before newer callback
events. Repeated visits finish with entries `/7, /8, /3`, host position0 and no
error; error-handler Forward finishes with model key3 at host `/3`.
The reviewer independently reran both external reproductions and all 65 focused
navigation/history tests in a fresh target; no remaining concrete defect was found.

## Public API and compatibility

`HistoryLocation` is an opaque identity allocated per host entry;
`HistoryDestination` contains that identity and the original `HistoryEntry`.
`HistoryBackend::current_destination` and `replace_destination` are required;
`subscribe` now delivers `HistoryDestination`. `current()` remains available with
a default implementation. Custom backend implementations must migrate, including
stable identity across replacement and fresh identity on a new branch.
See [host migration](../history-versioning.md#host-destination-api-migration).

An adapter unable to replace a noncurrent destination safely must report a typed
backend error. The explicit unsupported-backend regression verifies no unintended
write and reconnects the usable old model. No DOM, JavaScript, UI framework,
async runtime or persistence-format change is introduced. Recursive
`HistoryController` and legacy `StackHistoryController` remain distinct.

## Final executed verification

All commands used RTK and pinned Rust 1.96.0. Final repository builds used fresh
`/tmp/decompose-followup-review-new`. Package verification used separate
`/tmp/decompose-followup-package-20261011`: reusing a target after package
verification reproduced the obsolete-artifact issue documented in the review.

| Check | Actual result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | Passed |
| `cargo test --all-targets --locked` | **593 unit/integration tests passed**, zero failed; all 15 example test binaries built |
| `cargo test --doc --locked` | **One doctest passed** |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked` | Passed |
| `cargo run --locked --example NAME`, every `examples/*.rs` | **All 15 examples executed successfully** |
| `cargo check --lib --locked --target wasm32-unknown-unknown` | Passed |
| Same check for `aarch64-linux-android` | Passed |
| Same check for `aarch64-apple-ios` | Passed |
| `cargo package --offline --locked --allow-dirty` | Packaged crate compiled against published dependencies; no publication |
| `cargo test --test panic_cleanup --locked` | All 12 passed, including subprocess panic regressions |
| `cargo test --test items_performance --locked -- --nocapture` | All 12 passed; n=1024 counts remain 523776 default, 1024 stable inactive, 4096 stable active, 4609 indexed active reorder |
| `git diff --check` and staged whitespace check | Passed for follow-up changes |
| Historical audit/review diff against baseline commit | Empty; all three original reports unchanged |

593 = 575 baseline + five new navigation tests + thirteen new history tests.
Logs and command/exit-code inventory are in `/tmp/decompose-followup-checks/`.
Cross-target verification establishes library compilation. Kotlin differential
execution and remote GitHub Actions were not run. Existing raw Essenty ownership
and panic=abort limitations documented in the historical remediation report
remain unchanged. Graph discovery was refreshed initially; later graph calls
and final refresh failed with `Transport closed`, so source reads used fallback.
