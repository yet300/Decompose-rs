# Rust API audit and migration plan

> **Completed.** All groups below are implemented and merged (commits `78ee8d6`
> and `2c45df6`; details in [ADR-0019](adr/0019-explicit-rust-api-surface.md)).
> Because the crate is pre-public, deprecated aliases were **deleted**, not kept:
> no `#[deprecated]` remains in `src/`, `tests/` or `examples/`.
> The audit table is preserved as the historical proposal record.

Baseline (2026-10-10): `cargo test --all-targets` passed **509 tests** before edits.
No commits are part of this work. Dependency set and persistent formats are fixed.

## Audit

Locations below refer to the baseline, before additive methods shift line numbers.

| Location | Kotlin pattern / example | Rust problem | Proposal |
|---|---|---|---|
| `src/value.rs:54,74,124,189` | `get() -> Rc<T>`, `subscribe(Fn(Rc<T>) + 'static)`, cached `map` | Rc dereference/clone ceremony even for Copy values; immediate reads cannot accept local captures | Add `get_cloned`, `with`, `subscribe_ref`, `subscribe_cloned`, `modify(&mut T)`; retain snapshots and retained observers |
| `src/context.rs:78,199` | `ContextFactory<C> = Rc<dyn Fn(ContextParts) -> C>` | Direct context assembly allocates a closure and excludes borrowed services | Add `make_context(&self, parts)`; keep owned detached factories for routers that create future children |
| `src/router/stack/mod.rs:59,82`; `slot/mod.rs`, `pages/mod.rs`, `panels/mod.rs`, `items/mod.rs` factory functions | `factory: impl Fn(&C, Ctx) -> T + 'static` | Stack borrowing impossible for long-lived callbacks | Document ownership requirement; borrowed FnOnce initializers already exist and should be used for synchronous setup |
| `src/router/stack/navigation.rs:20,38,45,78,86`; analogous four families | `push`, `pop`, `replace_all`, `navigate` panic on rejection | Routine errors unwind | Add `try_* -> Result<NavigationOutcome<_>, NavigationError>` and explicitly named `*_or_panic`; deprecate ambiguous panicking names |
| `src/settings.rs:90`; `stack/mod.rs:38`, `pages/mod.rs`, `children/mod.rs:39` | Thread-local SETTINGS; Default snapshots global duplicate policy | Hidden input and diagnostics | Explicit `DecomposeConfig`, options `with_config`, context configuration; deprecated thread-local compatibility methods |
| `src/context.rs:41,52,188` | Public `Rc<RefCell<StateKeeper/InstanceKeeper>>`, MUST NOT hold borrow | Runtime borrowing is a public protocol | Opaque state/instance services with short operations returning owned data; keep deprecated aliases/accessors until major release |
| `src/value.rs:212,226,232,238` | `compare_and_set`, `update`, `update_and_get`, `get_and_update` | Java-style identity CAS and transforms obscure synchronous mutation | `compare_exchange`, `modify` and explicitly named snapshot variants; keep reentrant retries and exact identities |
| `src/router/navigation.rs:27`; `webhistory/mod.rs:42,110` | `with_completion`, `HistoryCompletion = Box<dyn FnOnce>` | Result delivered by retained callbacks | Result adapters, explicit pending receipts for accepted deferred operations; retain extension callbacks for FIFO/custom backends |
| `src/router/stack/navigation.rs:93` | `bring_to_front_by(configuration, kind_selector)` | Actually removes every matching kind before appending | Add `replace_kind_and_push`/fallible equivalent; preserve existing stable occurrence policy |
| `src/router/panels/navigation.rs:8`; `panels/mod.rs` | No Extra encoded using Infallible generic arguments | Verbose public spellings | Named two-panel aliases; uninhabited Extra remains a useful Rust guarantee |
| `src/router/items/map.rs:11,47` | Eq-only linear insertion-ordered ItemMap | Map complexity differs from std maps | Document O(n), add owned insertion/reference iteration; retain Eq-only compatibility and stable ordering |
| `src/router/mod.rs:62`; `webhistory/mod.rs:31,34` | `State(String)`, `Codec(String)`, `Backend(String)` | Typed source lost | Source-preserving additive error variants/constructors; retain legacy variants and wire codecs |
| `src/value.rs:337` | Subscription.detach | No cancellation token; retained lifecycle binding requires Destroy | Explicit leak-by-design docs; RAII recommended |
| `src/settings.rs:76` | Default eprintln | Library produces unsolicited stderr | Silent default, opt-in owned diagnostic hook |

## Sequence and verification

- [x] Baseline and source audit.
- [x] Value conveniences, immutable snapshots and reentrant clone-and-edit regression tests.
- [x] Fallible navigation families, explicit panic wrappers and FIFO receipts.
- [x] Explicit configuration and isolated diagnostics.
- [x] Opaque keeper services and direct context assembly.
- [x] History results, source errors, names/aliases/map docs.
- [x] Module docs, compatibility matrix, runnable migration example, final review.

After each implementation group run `cargo fmt --all -- --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test --all-targets`.
Final checks additionally run doctests and strict rustdoc. Existing tests remain
legacy-behavior oracles; new tests use the additive APIs without deprecation allowances.

## Baseline panic inventory

Every production `panic!`, `expect` and `unwrap` below is reachable through a
public operation or guards an internal invariant used by one. Unit-test-only
`src/testing.rs` is excluded. User callbacks and Essenty may also propagate
panics; this inventory is for this crate's explicit sites. `assert!` panic guards
are listed separately below. Checked fixed-width conversions are safe because
`take(N)?` either returns exactly N bytes or returns an error.

| Baseline location | Expression | Classification |
|---|---|---|
| `src/router/children/mod.rs:114` | `panic!("{error}");` | Legacy Generic completion/Back error delivery |
| `src/router/children/mod.rs:203` | `panic!("{error}");` | Legacy Generic completion/Back error delivery |
| `src/router/children/state.rs:101` | `Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))` | Checked fixed-width conversion |
| `src/router/children/state.rs:116` | `let next_key = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());` | Checked fixed-width conversion |
| `src/router/engine.rs:1184` | `.expect("initialized stack")` | Validated internal invariant |
| `src/router/items/mod.rs:185` | `lifecycle_state: *s.active_items.get(c).expect("live target"),` | Validated internal invariant |
| `src/router/items/navigation.rs:78` | `panic!("items navigation failed: {e:?}")` | Legacy convenience error delivery |
| `src/router/items/state.rs:194` | `Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))` | Checked fixed-width conversion |
| `src/router/items/state.rs:209` | `let next_key = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());` | Checked fixed-width conversion |
| `src/router/pages/navigation.rs:34` | `panic!("pages navigation failed: {e:?}")` | Legacy convenience error delivery |
| `src/router/pages/navigation.rs:321` | `panic!("pages navigation failed: {e:?}")` | Legacy convenience error delivery |
| `src/router/pages/state.rs:194` | `Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))` | Checked fixed-width conversion |
| `src/router/pages/state.rs:209` | `let next_key = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());` | Checked fixed-width conversion |
| `src/router/pages/state.rs:210` | `let selection = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());` | Checked fixed-width conversion |
| `src/router/panels/engine.rs:89` | `unreachable!("panels are always live")` | Programmer/reentrancy guard |
| `src/router/panels/engine.rs:113` | `_ => unreachable!("typed panel factory role mismatch"),` | Programmer/reentrancy guard |
| `src/router/panels/engine.rs:117` | `main: main.expect("required main"),` | Validated internal invariant |
| `src/router/panels/navigation.rs:424` | `panic!("panels navigation failed: {e:?}")` | Legacy convenience error delivery |
| `src/router/panels/state.rs:220` | `Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))` | Checked fixed-width conversion |
| `src/router/panels/state.rs:235` | `let next_key = u64::from_le_bytes(r.take(8)?.try_into().unwrap());` | Checked fixed-width conversion |
| `src/router/slot/mod.rs:83` | `_ => unreachable!("slot children always live"),` | Programmer/reentrancy guard |
| `src/router/slot/navigation.rs:19` | `panic!("slot navigation failed: {error:?}");` | Legacy convenience error delivery |
| `src/router/slot/state.rs:161` | `Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))` | Checked fixed-width conversion |
| `src/router/slot/state.rs:176` | `let next_key = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());` | Checked fixed-width conversion |
| `src/router/stack/engine.rs:60` | `_ => unreachable!("stack children always live"),` | Programmer/reentrancy guard |
| `src/router/stack/engine.rs:64` | `active: children.pop().expect("validated nonempty stack"),` | Validated internal invariant |
| `src/router/stack/navigation.rs:23` | `panic!("stack navigation failed: {error:?}");` | Legacy convenience error delivery |
| `src/router/stack/navigation.rs:53` | `while stack.len() > 1 && predicate(stack.last().unwrap()) {` | Length-guarded access (removed by this change) |
| `src/router/stack/state.rs:137` | `Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))` | Checked fixed-width conversion |
| `src/router/stack/state.rs:152` | `let next_key = u64::from_le_bytes(reader.take(8)?.try_into().unwrap());` | Checked fixed-width conversion |
| `src/router/webhistory/controller.rs:64` | `write!(output, "%{byte:02X}").unwrap();` | Validated internal invariant |
| `src/router/webhistory/controller.rs:111` | `url: nodes.last().unwrap().url(),` | Validated internal invariant |
| `src/router/webhistory/controller.rs:153` | `outer.last_mut().unwrap().children = children;` | Validated internal invariant |
| `src/router/webhistory/controller.rs:160` | `new_active.child_identity.unwrap(),` | Validated internal invariant |
| `src/router/webhistory/controller.rs:168` | `Change::LegacyRewrite(..) => unreachable!(),` | Programmer/reentrancy guard |
| `src/test_utils.rs:55` | `.expect("fresh lifecycle accepts every state");` | Fresh lifecycle invariant |
| `src/value/lifecycle.rs:108` | `let value = self.value.borrow().as_ref().unwrap().clone();` | Validated internal invariant |
| `src/value/mapped.rs:51` | `assert!(` | Programmer/reentrancy guard |
| `src/value/mapped.rs:68` | `.unwrap();` | Validated internal invariant |
