# Independent core completion

Reference: Decompose `00840bcfed6d55710c8fbb1a72503f2041eda490`.
Baseline: Rust `f3886cd`; user has explicitly authorized design and local commits.

Keep one synchronous Rc/RefCell crate and the existing Essenty versions and byte
formats. Additive codec predicates gate save before nested providers and restore
after full validity checks. Absence/decline use the lazy initializer; valid empty
membership remains distinct, and corruption fails with rollback.

An owned `StateSnapshot` carries persistent bytes and type-erased Rc memory
values. A keeper opts a router key into memory navigation before construction.
Snapshot restoration retains configuration identities, child keys and nested
snapshots; persistent byte extraction excludes every memory value. No global
store, disk, new configuration bounds or changes to public option literals.

Settings use synchronous scoped thread-local values; existing explicit duplicate
booleans win over defaults. Typed diagnostics run outside engine borrows, retain
Result/panic behavior and suppress recursive reporting. Public test utilities
live in the same crate and distinguish in-memory recreation from process death.

Web history uses a backend abstraction and deterministic memory backend. The
portable controller supports route codecs, Back/Forward, restoration, nested
navigation, completion-aware feedback suppression and lifecycle/token cleanup.
DOM/browser bindings and UI integrations are excluded.

Compare pinned public core sources beyond the baseline matrix. Each difference
must be implemented, an equivalent Rust contract, or a concrete excluded host/UI
integration. Kotlin byte interchange is outside the required compatibility.
