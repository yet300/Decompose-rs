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
