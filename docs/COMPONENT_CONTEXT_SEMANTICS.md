# ComponentContext Semantics (upstream behavioral contract)

Source: `upstream-ref` @ `00840bc`. All statements verified against implementation
*and* `commonTest` suites. This document states the contract the Rust port must honor;
for the Rust mapping see `RUST_OWNERSHIP_MODEL.md` and `PUBLIC_API_PROPOSAL.md`.

## 1. Shape

```kotlin
interface GenericComponentContext<out T : Any> :
    LifecycleOwner, StateKeeperOwner, InstanceKeeperOwner, BackHandlerOwner,
    ComponentContextFactoryOwner<T>

interface ComponentContext : GenericComponentContext<ComponentContext>
```

A context is exactly five things: `lifecycle`, `stateKeeper`, `instanceKeeper`,
`backHandler`, plus `componentContextFactory` — a *detached* factory
`(lifecycle, stateKeeper, instanceKeeper, backHandler) -> T` that mints siblings of
the same type with no parent link. Attachment (child scoping) is always the caller's job
(`childContext`, navigation engine). `ComponentContext` itself is the default
instantiation (`T = ComponentContext`).

Why generic over its own type: navigation (`children()`, `childStack`, …) is written
against `Ctx : GenericComponentContext<Ctx>` and builds every child scope exclusively
through `componentContextFactory`, so a custom user context flows through navigation
untouched and `childContext` returns the *same* custom type with zero casts.
Covariance (`out T`) lets factories compose (`Factory<AppCtx> <: Factory<Any>`).
Contexts are not clonable; "cloning" means invoking the factory with four dependencies.
The canonical custom-context recipe closes extra services over the delegate factory:

```kotlin
override val componentContextFactory = ComponentContextFactory { l, s, i, b ->
    DefaultAppComponentContext(componentContext.componentContextFactory(l, s, i, b), logger)
}
```

## 2. Defaults (`DefaultComponentContext`)

Only `lifecycle` is required. When omitted:

- `stateKeeper` → fresh `StateKeeperDispatcher()`;
- `instanceKeeper` → fresh `InstanceKeeperDispatcher().attachTo(lifecycle)` (destroyed with it);
- `backHandler` → fresh `BackDispatcher()`;
- `componentContextFactory` → bound `::DefaultComponentContext` constructor reference.

## 3. `childContext(key, lifecycle?, backHandlerPriority?)`

Fixed-child helper ("lives as long as the parent"). `key` must be unique within the parent
(enforced by the state-keeper `check`, see §5). Assembly:

```kotlin
lifecycle?.doOnDestroy { error("The lifecycle of a child ComponentContext must never be destroyed manually.") }
componentContextFactory(
    lifecycle      = if (lifecycle == null) this.lifecycle else MergedLifecycle(this.lifecycle, lifecycle),
    stateKeeper    = stateKeeper.child(key, lifecycle),
    instanceKeeper = instanceKeeper.child(key, lifecycle),
    backHandler    = backHandler.child(lifecycle, backHandlerPriority),
)
```

- `lifecycle == null` → child *shares* the parent lifecycle reference (no merge):
  child follows parent through resume and destroy.
- Non-null → `MergedLifecycle(parent, child)`; manual destruction of the supplied
  lifecycle is a programmer error (`IllegalStateException`).
- `backHandlerPriority` overload (default `BackCallback.PRIORITY_DEFAULT = 0`) controls
  global back ordering; default ordering is reverse creation order (LIFO on ties).

## 4. `MergedLifecycle` — `min(parent, manual)` as a lifecycle

Internal. Only `DESTROYED` is honored at construction; any other input state is treated as
`INITIALIZED`, so initial state is always `INITIALIZED` and forward progress comes from
subscription replay + live callbacks. Every callback maps to `moveTo(min(side1, side2))`
with `onCreate→CREATED, onStart→STARTED, onResume→RESUMED, onPause→STARTED, onStop→CREATED,
onDestroy→DESTROYED` (reverse transitions are lossy by design). `moveTo` is idempotent
with sticky destroy (`INITIALIZED→DESTROYED` goes through `create();destroy()`; all other
post-destroy moves are no-ops) and relies on Essenty cascading for multi-step jumps.
If either input is already `DESTROYED`, no subscriptions are taken and the merged
lifecycle is `DESTROYED`. On merged `DESTROYED` both observers unsubscribe (exactly-once
observable destroy; later parent transitions have no effect). Delivery is fully
synchronous and reentrant.

Forward/reverse table (from `MergedLifecycleTest`, 40+ cases): one side alone never
advances; `CREATED×CREATED→ON_CREATE`; `STARTED×STARTED→ON_CREATE,ON_START`;
`RESUMED×RESUMED→ON_CREATE,ON_START,ON_RESUME`; single `pause→ON_PAUSE`, single
`stop→ON_PAUSE,ON_STOP`, any `destroy→ON_PAUSE,ON_STOP,ON_DESTROY`; second destroy is silent.

## 5. `ChildStateKeeper` — key as namespace

```kotlin
check(!isRegistered(key)) { "The key \"$key\" is already in use." }
val child = StateKeeperDispatcher(consume(key, SerializableContainer.serializer()))
if (lifecycle == null) register(key, supplier = child::save)
else if (!lifecycle.isDestroyed) { register(key, supplier = child::save); lifecycle.doOnDestroy { unregister(key) } }
```

- The child key is a namespace: the parent reserves it (`register`) and consumes the
  restored container exactly once (destructive read) to seed the child dispatcher.
- `save()` chains: parent save → child save → grandchildren (nested dispatchers recurse).
- Child-lifecycle destroy releases the key (`unregister`, exactly once) so it is reusable.
- Already-destroyed lifecycle: consume still runs, but nothing is registered — a detached,
  never-saved dispatcher.
- Siblings may reuse the same *internal* state keys; the parent-level key keeps them disjoint.
- Forbidden: duplicate parent-level keys (`IllegalStateException` with the key in the message).

## 6. `ChildInstanceKeeper` — nested retention

```kotlin
if (lifecycle == null || !lifecycle.isDestroyed) {
    val registry = getOrCreate(key, ::ChildInstanceKeeperProvider).instanceKeeperRegistry
    lifecycle?.doOnDestroy { remove(key)?.onDestroy() }
    registry
} else InstanceKeeperDispatcher() // detached
```

`ChildInstanceKeeperProvider` owns the nested dispatcher; parent destroy cascades
(`provider.onDestroy() → registry.destroy()`). Same parent keeper + same key returns the
*same* nested registry object — the identity that survives config-change recreation
(`assertSame` in tests). Child-lifecycle destroy removes the provider exactly once
(`remove` returns null when absent, so double-destroy is safe). Parent `InstanceKeeper`
destruction destroys child instances; bare child-lifecycle destruction (outside the
registered hook) does not. `DefaultComponentContext`'s `attachTo(lifecycle)` default is
what ties root retention to the root lifecycle.

## 7. `ChildBackHandler` — one parent callback per child scope

```kotlin
val handler = childBackHandler(priority, isEnabled = false)
if (lifecycle == null) { handler.isEnabled = true; handler.start() }
else if (!lifecycle.isDestroyed) {
    handler.isEnabled = lifecycle.state >= STARTED
    handler.start()
    lifecycle.subscribe(onStart = { handler.isEnabled = true },
                        onStop  = { handler.isEnabled = false },
                        onDestroy = handler::stop)
}
```

`DefaultChildBackHandler` registers a *single* `parentCallback` in the parent dispatcher;
`parentCallback.isEnabled = childIsEnabled(lifecycle gate) AND delegate.isEnabled (has ≥1
enabled child callback)`, kept in sync via `delegate.addEnabledChangedListener`. `priority`
is a live alias onto the parent callback. `start()`/`stop()` are idempotent (`isRegistered`
guard). Predictive back forwards 1:1 (`onBackStarted→startPredictiveBack`,
`onBackProgressed→progress`, `onBackCancelled→cancel`, `onBack→back`); the parent selects
the child as a unit by child priority, the child then selects its own winner. Destroyed
lifecycle → detached, never started, `parent.back() == false`. Gating table: only
`STARTED/RESUMED + enabled callback` handles back; everything else (including `CREATED +
enabled`) returns false.

## 8. Observable state (`Value` / `MutableValue` / `Relay` / `Cancellation`)

- `Value<T : Any>`: abstract class (not interface — ObjC/Swift exposure), `value` +
  `subscribe(observer): Cancellation` with **synchronous initial emission**. Exists to avoid
  coroutines/Reaktive. `subscribe(lifecycle, mode)` (default `START_STOP`) is lifecycle-aware.
- `MutableValue`: `var value` + `compareAndSet(expected, new)` by **reference** (`===`);
  thread-safe (`Lock`), reentrancy-safe (emit-loop coalesces; late subscribers converge to
  latest); duplicate observer lambdas deduped; `update/updateAndGet/getAndUpdate` are CAS loops.
- `map`: lazy derived value caching by upstream reference; no own observer list.
- `Relay` (internal): serial event bus; reentrant `accept` queues FIFO; first observer
  exception poisons the relay (clears queue, all future `accept` throw
  `IllegalStateException` with the original as cause). Navigation's queued execution is built on it.
- `Cancellation`: SAM token; `doOnCancel` internal combinator.

## 9. Policy (`DecomposeSettings`)

Global `@Volatile` settings: `duplicateConfigurationsEnabled = false` (navigation rejects
duplicate configs by equality unless enabled), `mainThreadCheckEnabled = true`
(violations route `NotOnMainThreadException` to `onDecomposeError`, never throw),
`onDecomposeError = ::printError`. Deprecated `onDecomposeError` top-level and
`DecomposeExperimentFlags` delegate to it. Linux/web thread checks are no-ops.
