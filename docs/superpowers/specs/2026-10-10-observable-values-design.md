# Observable values completion

Status: implemented and verified. Baseline d27d289, branch codex/observable-values.
The user approved completion, commit and merge of accumulated work into main,
followed by standard-router extension points. That is a separate subsequent stage.
Reference: Decompose 00840bc, value/{ValueExt,MutableValue,MutableValueExt,operator/Map}.

## Architecture

Complete the existing public value module in the same crate. Keep synchronous
single-threaded Rc ownership, no runtime/serde/dependency additions, and existing
direct-value replay, conflation, cancellation, detach and panic behavior. Private
derived-source machinery may replace the concrete-only representation of Value;
do not expose a second public observable abstraction or duplicate the event pump.
Keep mapping and lifecycle helpers in focused private submodules as needed.
No payload requires Clone, Eq, Hash, Send or Sync. Existing public constructors,
get/set/set_rc/subscribe/value/clone must remain usable without new payload bounds.

## Identity updates

MutableValue::compare_and_set(&self, expected: &Rc<T>, new: Rc<T>) -> bool
compares Rc::ptr_eq. Mismatch leaves value/observers untouched; success commits
and emits with the existing set_rc semantics, including same-reference assignment.
No user code or payload Drop under a source borrow.

update, update_and_get and get_and_update take impl FnMut(Rc<T>) -> Rc<T>.
Each snapshots previous, computes new without a borrow, retries CAS on a stale
snapshot caused by reentrant mutation. A transformer may run more than once.
update returns (); update_and_get returns the exact successfully installed Rc,
get_and_update returns its predecessor. Later observer reentry may replace the
current value but must not change these return identities. Panic propagates;
successful callback effects stay, failed candidate isn't committed. No lock-free
or multithreaded atomicity claim; Rc snapshot identity is the Rust adaptation.

## Cached mapping

Value::map<R: 'static>(&self, mapper: impl Fn(&T) -> R + 'static) -> Value<R>.
Mapping establishes its initial cache immediately, as upstream. get works without
subscribers and refreshes after source changes. Cache by upstream Rc identity;
equal payload in a fresh Rc maps again, repeated reads/observers of the same Rc
reuse the exact mapped Rc and don't duplicate mapper work. Each subscription
replays synchronously and tracks upstream changes, cancellation and detach follow
existing rules; chaining and non-Clone input/output work. No permanent upstream
subscription is required for a derived value with no observers.

Mapper executes outside obstructing RefCell borrows. Compute then install a
coherent input/output pair; a panic leaves cache retryable. Input is the snapshot
being mapped, not whatever a mapper's reentrant mutation subsequently installs.
Newer cache entries created during reentry must not be overwritten by an older
in-flight calculation. Recursive get for the very same uncached input may be
rejected with a documented panic instead of unbounded recursion. This guard
must reset during unwinding; safe mapped get/subscribe/update reentry remains
supported. Drop of old cached payloads occurs after cache borrows are released.

## Lifecycle observation

ObserveLifecycleMode { CreateDestroy, StartStop, ResumePause }, Default StartStop.
Value::subscribe_with_lifecycle(&self, lifecycle: &LifecycleRegistry,
mode: ObserveLifecycleMode, observer: impl Fn(Rc<T>) + 'static) -> Subscription.
This explicit RAII return is the Rust adaptation of Kotlin's lifecycle-owned helper.
The token cancels both source and lifecycle listener; detach keeps the binding
until terminal destruction. Ordinary RAII ownership must release captures even
when all external lifecycle handles disappear without Destroy. Essenty exposes
no weak registry handle; detached bindings therefore intentionally retain their
registry witness and require explicit Destroy to release lifecycle ownership.
Source is observed at Created+, Started+, or Resumed
according to mode, never Initialized/Destroyed. Already-active lifecycle replays
immediately; inactive source changes are skipped, reopening replays latest.
Stop/Pause close the proper gate, Destroy releases source/listener/captures.
Already-Destroyed registration is inert and releases observer captures.
Synchronous replay may stop/destroy lifecycle, cancel binding, or update source;
prepare guards first, never invoke callbacks under obstructing borrows, cancel a
subscription returned after its replay closed the gate. Panic on setup rolls back
both registrations and preserves the original panic. Ordinary observer panic
propagates with existing Essenty dispatch limits, binding must remain cancellable.

## Verification and integration

Use real published Essenty. Test identity CAS, stale update retries and exact
return values, callback/transformer panics and Drop reentry. Test mapping cache
counts/identity across readers/subscribers, no-subscriber changes, chain/cancel/
detach, reentrant updates/reads and panic recovery. Test every lifecycle gate,
initial states, transitions, replay shutdown, ownership/capture release and panic.
All previous navigation and value tests must pass. Independent spec then quality
reviews precede commit/merge. Controller runs tests/doc/fmt/strict Clippy/rustdoc,
all seven examples and installed target matrix. Merge only clean verified state;
preserve all earlier branch history, no remote push or branch deletion.
