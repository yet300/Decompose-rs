# Repository & Naming Strategy (no remote action taken in Phase 0)

## Recommendation: Option A

**Keep `yet300/Decompose` as the upstream Kotlin fork; create `yet300/Decompose-rs` as a
new non-fork Rust repository.** Rationale:

1. The fork relationship to `arkivanov/Decompose` is load-bearing: it enables upstream
   fetch/compare and signals provenance. Freeing the name (Option B) destroys history
   linkage for zero technical benefit.
2. Rust and Kotlin trees must never share a history: force-pushing Rust into `master` would
   orphan every fork/pull reference and poison the crates.io ↔ repo mapping.
3. `Decompose-rs` matches the `Essenty` → `Essenty-rs` precedent already set by this
   organization (`yet300/Essenty`), keeping discoverability symmetric.
4. Migration/naming remains a reversible user decision; Phase 0 executes nothing
   destructive either way.

Explicitly **not done** in this phase: no deletion, no overwrite, no force-push, no Rust
replacement commit, no new remote repository creation, no renames. The research workspace
(`/Users/yet/development/rust/decompose`, local branch
`codex/phase-0-decompose-research`) is unpushed and contains only docs + a gitignored
`upstream-ref/` reference clone.

## crates.io naming research (checked 2026-10-04 via `cargo info`/`cargo search`)

| Candidate | Status | Evidence |
|---|---|---|
| `decompose` | TAKEN | v0.5.1, unrelated scheduler/orchestrator (`github.com/sciyoshi/decompose`, MIT/Apache) |
| `decompose-rs` | AVAILABLE | `cargo info` → "could not find in registry" |
| `decompose-core` | AVAILABLE | same |
| `decompose-stack` (and by extension per-family names) | AVAILABLE | same |

Package name: `decompose-rs` (single core crate through Phase 5; family crates, if ever
split, take `decompose-rs-*` prefixes to avoid the taken root). Nothing is published in
Phase 0. Before first publish, re-check availability and reserve early with a minimal
`0.1.0-alpha` if squatting risk appears.

## GitHub naming

New repository `yet300/Decompose-rs` (non-fork, Apache-2.0, Edition 2024 workspace).
Keep `yet300/Decompose` fork intact for upstream tracking (`git fetch upstream` against
`arkivanov/Decompose`). Research docs migrate into `Decompose-rs/docs/` at creation;
the `upstream-ref/` clone does not migrate (re-clone by SHA when needed).

## Licensing / attribution plan

- License: **Apache-2.0** (same as upstream; Essenty-rs is Apache-2.0; compatible throughout).
- Every crate ships `LICENSE-APACHE` + `NOTICE` attributing Ark Ivanov / upstream Decompose,
  stating the Rust port is an independent implementation preserving behavioral semantics,
  with no claim of official affiliation.
- Behavioral parity does not copy expression: where implementation is substantially adapted
  from upstream source (e.g. transition tables), retain comments citing the upstream file +
  SHA (`00840bc`) rather than copying code verbatim.
- Essenty-rs dependency is a registry dependency (`essenty-*= "0.1.0"`), satisfying the
  "registry-only" acceptance gate; no path dependencies in published artifacts.
