# Review of audit remediation

Date: 2026-10-11. Repository HEAD: `ddd8f69`; remediation is uncommitted.
Original audit and implementation report are preserved.

## Result

The original adapter-managed StateStore data-loss, rollback abort, and lifecycle-instance borrow failures have meaningful fixes and regressions. Items performance and pop_while changes are supported by executed checks. Published raw Essenty limitations are explicitly documented rather than silently claimed fixed.

However, three additional P2 defects remain. The claim that every actionable behavior is complete would be premature.

Confidence: **high** for all three reproduced failures; **moderate** for exhaustive Kotlin equivalence. Production code was not changed by this review.

## P2: cancelled accepted navigation reports success

Locations:
- [engine.rs:782](/Users/yet/development/rust/decompose/src/router/engine.rs:782), Deferred cancellation;
- [engine.rs:938](/Users/yet/development/rust/decompose/src/router/engine.rs:938), parent teardown;
- [navigation.rs:36](/Users/yet/development/rust/decompose/src/router/navigation.rs:36), receipt collection.

Both cancellation paths call `release_events(abandoned, false)`. This drops completion capabilities without delivering an error; the receipt becomes non-pending and collects an empty result vector into `Ok([])`.

Repro: inside a root Back callback, call `try_push(1)` and keep the returned Pending receipt. Destroy the parent before Back dispatch completes. Alternatively, panic in that Back callback and catch the unwind.

Observed:
```text
active config: 0, destroyed: true, receipt results: Ok(0)
```

The navigation was accepted but never executed. Its result must report cancellation/unavailability, rather than successful empty results. Immediate dispatch without receivers is a different contract.

The ordinary pump unwind guard already uses `release_events(..., true)`. Complete every accepted abandoned event with an error, retaining Cleanup isolation for panicking completions. Add regressions for both parent Destroy and Back callback panic, including multi-receiver completion semantics.

## P2: queued history recreation gets the wrong destination URL

Locations:
- [controller.rs:488](/Users/yet/development/rust/decompose/src/router/webhistory/controller.rs:488), building HistoryRecreation;
- [controller.rs:703](/Users/yet/development/rust/decompose/src/router/webhistory/controller.rs:703), queueing only saved HistoryState.

The recreation hook reads the URL from `backend.current()` when a queued event is processed. That is no longer necessarily the destination of the original Pop event.

Repro: history entries `old /1 (index0)`, `old /2 (index1)`, `new /3 (index2)`; start at index2. Back to /1 starts deferred recreation. Forward to /2 queues a second mismatch. Complete the first recreation with a /9 root.

Observed:
```text
before: requests=["/1"], host=1
after: requests=["/1", "/9"], host=0, urls=["/9", "/2", "/3"]
```

Expected recreation destinations are `["/1", "/2"]`. The host is moved by the first completion before the second event reads its URL, so a host creating roots from deep links receives the wrong route.

Preserve the destination metadata with the queued event and tie deferred completion to the same destination. Do not reconstruct the target from whichever entry happens to be current later.

## P2: late recreation overwrites an unrelated unmanaged entry

Locations:
- [controller.rs:658](/Users/yet/development/rust/decompose/src/router/webhistory/controller.rs:658), unmanaged state mapped to managed index0;
- [controller.rs:889](/Users/yet/development/rust/decompose/src/router/webhistory/controller.rs:889), replacement based only on numeric index equality.

Repro: physical entries `[None /external, managed old index0 /1, managed new index1 /3]`; start at the last entry. First Back starts deferred recreation for /1. Second Back reaches /external. Complete the first recreation with a /9 root.

Observed:
```text
before: requests=["/1"], host=0
after: requests=["/1"], host=0, urls=["/9", "/1", "/3"]
```

The external entry was overwritten, while the intended managed /1 entry stayed stale. Missing managed state is mapped to index0, which accidentally matches the original target's managed index0. The queued `Pop(None)` then does nothing.

Do not equate an unmanaged entry with managed index0. Track a destination identity/location sufficient for late completion and preserve subsequent host navigation. Add a regression explicitly asserting the external entry is untouched.

The two history defects share insufficient destination bookkeeping, but the correction must cover both original URL preservation and replacement of the correct host entry.

## Verification performed independently

- Full test suite built in clean `/tmp/decompose-remediation-review-20261011`: **575 unit/integration tests**, exit0.
- One doctest, exit0.
- Format, strict Clippy/all features, strict rustdoc, diff whitespace check: passed.
- All 15 examples executed successfully.
- Library checks for WASM, Android, and iOS: passed.
- `cargo package --offline --locked --allow-dirty`: package and extracted-crate verification passed against published dependencies.
- Focused Items measurements reproduced:
  - unchanged inactive default n=1024: 523776 Eq calls;
  - stable Eq-only opt-in: 1024;
  - unchanged active stable opt-in: 4096;
  - dispersed indexed active reverse reorder: 4609.
- Existing store/cleanup/pop_while regression suites passed.
- All three findings above were reproduced independently with external programs.

An initial default-target test invocation picked up an obsolete pre-remediation library artifact. Clean-target compilation passed; that artifact failure is not classified as a source defect. Remote GitHub Actions and Kotlin differential tests were not run.

## Reproduction programs

Each program uses an external Cargo project:
```toml
[package]
name = "decompose-review-repro"
version = "0.0.0"
edition = "2024"

[dependencies]
decompose-rs = { path = "/Users/yet/development/rust/decompose" }
```

Place one program at a time in `src/main.rs` and run:
```text
rtk proxy cargo run --offline --manifest-path /tmp/decompose-review-repro/Cargo.toml
```

These programs demonstrate the current failures; their output is not a success criterion for a corrected implementation.

### Cancelled receipt

```rust
use decompose_rs::{ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState};
use decompose_rs::router::{NavigationOutcome, stack::{StackNavigation, StackNavigator, StackOptions, child_stack}};
use std::{cell::RefCell, rc::Rc};
fn main() {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    let root = DefaultComponentContext::new(lifecycle);
    let navigation = StackNavigation::new();
    let stack = child_stack(&root, &navigation, vec![0u32], StackOptions::default(), |_, ctx| ctx).unwrap();
    let pending = Rc::new(RefCell::new(None));
    let out = pending.clone();
    let lifecycle = root.lifecycle();
    let nested = navigation.clone();
    root.back_handler().register(100, true, move |event, _| {
        if event.phase == decompose_rs::BackPhase::Invoked {
            match nested.try_push(1).unwrap() {
                NavigationOutcome::Pending(receipt) => *out.borrow_mut() = Some(receipt),
                NavigationOutcome::Completed(_) => panic!("expected deferred navigation"),
            }
            lifecycle.destroy().unwrap();
        }
    }).unwrap();
    root.back_handler().back();
    let receipt = pending.borrow_mut().take().unwrap();
    let result = receipt.take_results().unwrap();
    println!("active config: {}, destroyed: {}, receipt results: {:?}", *stack.get().active.configuration, root.lifecycle().state().is_destroyed(), result.as_ref().map(Vec::len));
    assert!(result.as_ref().is_ok_and(Vec::is_empty));
}
```

### Queued recreation URL

```rust
use decompose_rs::router::webhistory::*;
use decompose_rs::value::MutableValue;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn route(key: u8) -> Route<u8> {
    Route::from_key(key, key.to_string())
}
fn nav(source: &MutableValue<Vec<Route<u8>>>, decodes: &Rc<Cell<usize>>) -> Rc<dyn WebNavigation> {
    let target = source.clone();
    let count = decodes.clone();
    Rc::new(TypedWebNavigation::new(
        source.value(),
        RouteCodec::new(
            |key| Ok(vec![*key]),
            move |bytes| {
                count.set(count.get() + 1);
                bytes
                    .first()
                    .copied()
                    .ok_or_else(|| HistoryError::Codec("obsolete".into()))
            },
        ),
        move |keys, done| {
            target.set(keys.into_iter().map(|key| route(*key)).collect());
            done(Ok(()));
        },
    ))
}
fn state(index: usize, version: Option<&str>, key: u8) -> HistoryEntry {
    HistoryEntry {
        state: Some(HistoryState {
            index,
            version: version.map(str::to_owned),
            nodes: vec![HistoryNode {
                key: vec![key],
                path: key.to_string(),
                children: vec![],
            }],
        }),
        url: format!("/{key}"),
    }
}


fn main() {
 let source=MutableValue::new(vec![route(3)]); let replacement=MutableValue::new(vec![route(9)]); let decodes=Rc::new(Cell::new(0)); let backend=Rc::new(MemoryHistoryBackend::with_entries(vec![state(0,Some("old"),1),state(1,Some("old"),2),state(2,Some("new"),3)],2).unwrap());
 let pending=Rc::new(RefCell::new(Vec::<HistoryRecreationCompletion>::new())); let out=pending.clone(); let requests=Rc::new(RefCell::new(Vec::new())); let req=requests.clone();
 let _binding=HistoryController::attach_with_options(nav(&source,&decodes),backend.clone(),HistoryOptions{version:Some("new".into()),recovery:HistoryRecoveryPolicy::Recreate(Rc::new(move |r,done| {req.borrow_mut().push(r.url);out.borrow_mut().push(done);})),}).unwrap();
 backend.go(-2,Box::new(|r|r.unwrap())); backend.go(1,Box::new(|r|r.unwrap())); println!("before: requests={:?}, host={}", requests.borrow(),backend.position());
 let done=pending.borrow_mut().remove(0); done(Ok(HistoryRoot::new(nav(&replacement,&decodes)))); println!("after: requests={:?}, host={}, urls={:?}", requests.borrow(),backend.position(),backend.entries().iter().map(|e|e.url.clone()).collect::<Vec<_>>());
}
```

### Unmanaged history entry overwritten

```rust
use decompose_rs::router::webhistory::*;
use decompose_rs::value::MutableValue;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn route(key: u8) -> Route<u8> {
    Route::from_key(key, key.to_string())
}
fn nav(source: &MutableValue<Vec<Route<u8>>>, decodes: &Rc<Cell<usize>>) -> Rc<dyn WebNavigation> {
    let target = source.clone();
    let count = decodes.clone();
    Rc::new(TypedWebNavigation::new(
        source.value(),
        RouteCodec::new(
            |key| Ok(vec![*key]),
            move |bytes| {
                count.set(count.get() + 1);
                bytes
                    .first()
                    .copied()
                    .ok_or_else(|| HistoryError::Codec("obsolete".into()))
            },
        ),
        move |keys, done| {
            target.set(keys.into_iter().map(|key| route(*key)).collect());
            done(Ok(()));
        },
    ))
}
fn state(index: usize, version: Option<&str>, key: u8) -> HistoryEntry {
    HistoryEntry {
        state: Some(HistoryState {
            index,
            version: version.map(str::to_owned),
            nodes: vec![HistoryNode {
                key: vec![key],
                path: key.to_string(),
                children: vec![],
            }],
        }),
        url: format!("/{key}"),
    }
}


fn main() {
 let source=MutableValue::new(vec![route(3)]); let replacement=MutableValue::new(vec![route(9)]); let decodes=Rc::new(Cell::new(0)); let backend=Rc::new(MemoryHistoryBackend::with_entries(vec![HistoryEntry{state:None,url:"/external".into()},state(0,Some("old"),1),state(1,Some("new"),3)],2).unwrap());
 let pending=Rc::new(RefCell::new(Vec::<HistoryRecreationCompletion>::new())); let out=pending.clone(); let requests=Rc::new(RefCell::new(Vec::new())); let req=requests.clone();
 let _binding=HistoryController::attach_with_options(nav(&source,&decodes),backend.clone(),HistoryOptions{version:Some("new".into()),recovery:HistoryRecoveryPolicy::Recreate(Rc::new(move |r,done| {req.borrow_mut().push(r.url);out.borrow_mut().push(done);})),}).unwrap();
 backend.go(-1,Box::new(|r|r.unwrap())); backend.go(-1,Box::new(|r|r.unwrap())); println!("before: requests={:?}, host={}", requests.borrow(),backend.position());
 let done=pending.borrow_mut().remove(0); done(Ok(HistoryRoot::new(nav(&replacement,&decodes)))); println!("after: requests={:?}, host={}, urls={:?}", requests.borrow(),backend.position(),backend.entries().iter().map(|e|e.url.clone()).collect::<Vec<_>>());
}
```

