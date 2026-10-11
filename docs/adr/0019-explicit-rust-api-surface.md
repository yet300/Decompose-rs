# ADR-0019: Explicit Rust API surface without deprecated aliases

Status: Accepted. Upstream reference:
`00840bcfed6d55710c8fbb1a72503f2041eda490`.
Supersedes the compatibility shims of [ADR-0017](0017-settings-and-public-test-context.md)
(thread-local settings) and the Java-style value aliases of
[ADR-0011](0011-observable-values-completion.md). The crate is pre-public, so the
old spellings are removed rather than deprecated.

## Upstream contracts

Kotlin exposes panicking navigation conveniences (`push`, `pop`, `select`, ...),
overloaded `navigate` helpers, a mutable `DecomposeSettings` global, an
`AtomicReference`-style `MutableValue` extension API (`compareAndSet`, `update`,
`updateAndGet`, `getAndUpdate`) and direct keeper handles on contexts. These
spellings are convenient in Kotlin but unidiomatic in Rust: panics hide routine
failures, globals hide inputs, `Rc`-identity CAS obscures mutation, and exposed
`RefCell` guards turn borrow discipline into a runtime protocol.

## Decisions

- Navigation families expose exactly three operation kinds: `try_*` returning
  `Result<NavigationOutcome<R>, NavigationError>` (a `Pending` receipt for
  accepted nested work), `*_or_panic` panicking with `#[track_caller]` location,
  and `*_with`/`navigate_with` taking explicit typed completions. The 72
  short panicking aliases (`push`, `pop`, `select`, `activate`, `dismiss`,
  `navigate`, `set_items`, ...) are deleted from traits and inherent impls.
  Abandoned queued work reports `ChildUnavailable` through pending receipts
  instead of an empty success.
- `DecomposeSettings` and its thread-local global are deleted. `DecomposeConfig`
  (duplicate default plus opt-in `with_error_handler`, silent by default) is
  attached explicitly via `with_config` on contexts and
  `StackOptions`/`PagesOptions`/`ChildrenOptions`. `Default` for options rejects
  duplicates. Diagnostics flow only through the owning context's config.
- `ComponentContext` exposes only opaque `StateStore`/`InstanceStore` services
  returning owned data; `SharedKeeper`/`SharedInstances` are crate-internal and
  no public signature leaks `RefCell` guards. `DefaultComponentContext` offers
  `with_state_store`/`with_instance_store`/`with_stores`/`with_back_handler`;
  `TestComponentContext` offers `with_stores`. Provider captures drop and
  factories run outside keeper borrows, so `Drop`/reentrant callbacks observe a
  usable store.
- `MutableValue` keeps `compare_exchange` (`Result<Rc<T>, Rc<T>>`), `update_rc`,
  `update_rc_and_get`, `get_and_update_rc`, and adds `modify` (`&mut T` edits),
  `get_cloned`, `with`, `subscribe_ref` and `subscribe_cloned`. The
  `compare_and_set`/`update`/`update_and_get`/`get_and_update` aliases are
  deleted.
- `StateStore` gains `register_fallible`, `provider_count` and `ptr_eq`;
  `InstanceStore` gains `contains` and `ptr_eq` so external tests need no raw
  handles. White-box borrow-conflict tests that required holding a `RefCell`
  guard across an operation are removed or rewritten to public-API assertions;
  borrow discipline is now unobservable externally by construction.

## Consequences

Callers migrate mechanically: `nav.push(x)` becomes `nav.push_or_panic(x)` (or
`nav.try_push(x)` for a `Result`); `value.update(f)` becomes
`value.update_rc(f)` (or `value.modify` for `&mut T` edits);
`DecomposeSettings::with(...)` becomes an explicit `DecomposeConfig` passed to
contexts and options; `ctx.state_keeper().borrow_mut()` becomes
`ctx.state_store()` operations. `Vec::push` and other std methods are
unaffected. The compatibility matrix records the new spellings; the pre-removal
behavior remains described by the ADR history.

Migrating `State(String)` failures to `StateSource(ErrorSource)` changes `PartialEq` from message equality to source identity and changes `Debug`; `Display` is preserved (`navigation state: …`).

Verification: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test --all-targets` (519 tests), `cargo test --doc` (1 doctest), strict
`cargo doc`, all 15 examples, and `cargo check --lib` on all 24 installed
targets. `grep -rn deprecated src/ tests/ examples/` is empty.
