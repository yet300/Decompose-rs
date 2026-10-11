# Аудит Decompose-rs: полнота порта и качество Rust

Дата: 2026-10-11. Rust HEAD: `ddd8f69` (`main`, рабочее дерево исходно чистое).
Проверенный компилятор: `rustc 1.96.0 (ac68faa20 2026-05-25)`.

Upstream:
- закреплённая версия: [00840bcfed6d55710c8fbb1a72503f2041eda490](https://github.com/arkivanov/Decompose/tree/00840bcfed6d55710c8fbb1a72503f2041eda490), локальный `upstream-ref`;
- текущий HEAD, прочитанный через `git ls-remote` и клонированный для проверки: [0824a183f7fe3862a1409553e66346c999de7084](https://github.com/arkivanov/Decompose/tree/0824a183f7fe3862a1409553e66346c999de7084), commit от 2026-10-10, merge PR #1035.

## Вывод

Это существенно адаптированный к Rust порт независимого ядра, а не механическая перепись Kotlin. Все пять стандартных моделей навигации и generic navigation присутствуют. Основные проблемы находятся в обработке деструкторов и rollback, а не в именовании или отсутствии очередного router.

Полным переносом всей upstream-библиотеки проект не является: UI/platform/DOM-интеграции намеренно исключены. От текущего upstream отстаёт переносимая логика версионирования web history.

Обнаружены три воспроизводимых дефекта управления ресурсами, одно отставание от нового upstream, одна подтверждённая проблема масштабирования и одно небольшое незадокументированное различие поведения. Зелёные проверки не покрывают найденные дефекты.

Уверенность **высокая** для воспроизведённых дефектов, наличия основных возможностей и сравнения исходников двух upstream-срезов; **умеренная** для полной поведенческой эквивалентности. Kotlin test suite не запускался как differential oracle.

## Приоритеты

| Приоритет | Находка | Уверенность |
|---|---|---|
| P1 | `StateStore::unregister` теряет успешные reentrant-изменения; panic удаляет посторонние providers/state | Высокая, воспроизведено |
| P1 | Rollback подготовленных scopes вызывает двойной panic и abort процесса | Высокая, воспроизведено в отдельном процессе |
| P2 | Lifecycle teardown уничтожает retained payloads под `RefCell` borrow | Высокая, воспроизведено |
| P2 | Нет новой схемы version/recreation для portable web history | Высокая по сравнению исходников |
| P2 для больших списков | Неизменившийся `ChildItems` выполняет O(n²) сравнений | Высокая, измерен счётчик Eq |
| P3 | `pop_while` не вызывает predicate на root, в отличие от Kotlin | Высокая, Rust воспроизведён и Kotlin исходник проверен |

P1 означает исправление до заявления о надёжном rollback/reentry. P2 — следующий этап в зависимости от использования history/больших списков. Это приоритеты аудита, не оценка частоты отказов в реальных приложениях.

## 1. StateStore::unregister теряет данные

Код: [src/context/stores.rs:122](/Users/yet/development/rust/decompose/src/context/stores.rs:122).

Метод перемещает **весь** keeper из общего `RefCell`, оставляя на его месте пустой. Затем удаляет provider из временно вынутого keeper и возвращает keeper обратно.

Проблемы:
1. Деструктор capture удаляемого provider видит пустое хранилище. Его `register("replacement", ...)` успешно возвращает `Ok`, а `is_registered` сразу подтверждает регистрацию.
2. Финальный swap возвращает старый keeper поверх нового. Успешная регистрация исчезает.
3. Если деструктор provider паникует, swap не выполняется. Вынутый keeper уничтожается вместе с посторонними providers и оставшимся restored state.

Воспроизведение:
```text
registration succeeded during Drop: true, persists afterward: false, unrelated retained: true
unregister panicked: true, unrelated retained afterward: false, provider_count: 0
```

Это не просто ограничение на reentry: API уже вернул успешный результат вложенной операции, который позже молча потерян.

Текущий тест [tests/rust_api.rs:390](/Users/yet/development/rust/decompose/tests/rust_api.rs:390) проверяет только, что reentrant `pending_count()` возвращает `Ok`. Он не проверяет содержимое store, сохранность вложенных mutations или соседних providers после panic.

Направление исправления: отделять только удаляемый provider/captures под кратким borrow и освобождать их после снятия borrow. Если Essenty не предоставляет такой операции, понадобится ownership-indirection в адаптере или новый primitive API. Простое добавление guard для возврата keeper защитит часть panic-сценария, но само по себе не сохранит вложенные mutations.

## 2. Rollback может abort-нуть весь процесс

Код: [src/router/engine.rs:235](/Users/yet/development/rust/decompose/src/router/engine.rs:235), `Prepared::drop`; также `ScopeParts::end` на [строке 114](/Users/yet/development/rust/decompose/src/router/engine.rs:114) и `ScopeParts::drop` на [строке 159](/Users/yet/development/rust/decompose/src/router/engine.rs:159).

Сценарий:
1. `child_stack` строит начальный стек `[1, 2]`.
2. Фабрика первого child регистрирует `do_on_destroy`, который паникует.
3. Фабрика второго child паникует.
4. При unwind `Prepared::drop` синхронно вызывает `scope.parts.end(...)`.
5. Обработчик Destroy первого child паникует внутри уже активного unwind.
6. Rust завершает процесс. Внешний `catch_unwind` не спасает.

Проверено в отдельном процессе:
```text
factory panic
cleanup panic
panic in a destructor during cleanup
thread caused non-unwinding panic. aborting.
exit 134; signal 6
```

Другие места — `Construction::drop` и `Engine::teardown` — уже собирают cleanup panics через `catch_unwind`, сохраняют первый panic и заканчивают независимую очистку. `Prepared::drop` этого не делает. Это внутренне противоречивый контракт panic recovery.

Направление исправления: единый протокол очистки для Prepared/ScopeParts/retention, завершающий независимые шаги, сохраняющий исходный unwind и не порождающий второй panic из Drop. Проверять subprocess-тестом, поскольку abort нельзя проверить обычным `catch_unwind`.

## 3. Lifecycle teardown нарушает обещание освобождения вне borrow

Код:
- [src/context.rs:355](/Users/yet/development/rust/decompose/src/context.rs:355): root destroy hook вызывает `owned.borrow_mut().destroy_all()`;
- [src/router/engine.rs:128](/Users/yet/development/rust/decompose/src/router/engine.rs:128): уничтожение child instances под borrow;
- [src/retention.rs:20](/Users/yet/development/rust/decompose/src/retention.rs:20): `RetainedScope::drop`.

Публичный `InstanceStore::release` на [src/context/stores.rs:362](/Users/yet/development/rust/decompose/src/context/stores.rs:362) аккуратно заменяет keeper и освобождает старые payloads **после** снятия borrow. Но lifecycle-owned пути обходят этот механизм.

В retained payload помещён Drop, вызывающий `instance_store.is_destroyed()`. При обычном `root.lifecycle().destroy()` этот вызов получает `InstanceAccessError::Borrowed`, а не нормальное состояние уничтоженного store.

```text
instance destructor read on lifecycle destroy succeeded: false
```

Если cleanup вызывает такой getter с `unwrap()`, обычное уничтожение начинает паниковать. Нарушение относится к внутренней дисциплине библиотеки, а не к пользователю, удерживающему borrow: публичные opaque services как раз должны исключать такой протокол.

Направление исправления: все пути завершения retention должны использовать один механизм отделения владения и drop вне borrow; затем проверить root Destroy, удаление child, освобождение RetainedScope, reentry и panic.

## 4. Что осталось перенести

### Независимое ядро

Сравнение `decompose/src/commonMain` закреплённого и текущего upstream не обнаружило изменений. Проверенные test-utils также не получили новых core API.

| Область | Состояние Rust | Примечание |
|---|---|---|
| ComponentContext, custom contexts, child contexts | Перенесено с Rust-адаптацией | Trait, factory, opaque stores; custom context без Clone |
| Stack и операции | Перенесено | Есть minor predicate difference ниже |
| Slot | Перенесено | Option для нуля/одного child |
| Pages | Перенесено | Selection Option, lifecycle policy, state восстановление |
| Panels | Перенесено | Типизированные роли, Single/Dual/Triple, без Extra через uninhabited type |
| Items/LazyChildItems | Перенесено | Eq-only ItemMap и явное lazy создание; есть CPU scalability issue |
| Generic children/navigation | Перенесено | Custom state/events/statuses/projection/hooks |
| Value/MutableValue/map/lifecycle subscription | Перенесено | Rc snapshots, RAII tokens, synchronous reentry |
| State/Instance/Back/retention | Перенесено функционально | Найдены defects teardown/rollback |
| Lazy initial state, memory/selective/whole-state persistence | Перенесено | Кодеки и собственные Rust formats |
| Settings и test context | Перенесено с адаптацией | Явный DecomposeConfig; public TestComponentContext |
| Recursive и legacy stack history | Перенесены algorithms закреплённой версии | Portable backend, без DOM binding; новая версия upstream ещё не покрыта |

Отсутствующих целых commonMain-семейств или крупных helper-групп не обнаружено. Это результат inventory/source review и Rust tests, не доказательство эквивалентности каждого callback при panic.

### Новая логика web history

Текущий upstream добавил:
- `withWebHistory(schemaVersion, block): Value<T>`;
- проверку версии сохранённого state;
- проверку версии при Back/Forward;
- пересоздание root с чистым StateKeeper при несовпадении;
- безусловную замену текущей history entry при attachment;
- корректировку index offset в guard-откате.

Источники:
- [JS API и root recreation](https://github.com/arkivanov/Decompose/blob/0824a183f7fe3862a1409553e66346c999de7084/decompose/src/jsMain/kotlin/com/arkivanov/decompose/router/webhistory/WebHistoryNavigation.kt#L38-L83);
- [portable history version check и attachment](https://github.com/arkivanov/Decompose/blob/0824a183f7fe3862a1409553e66346c999de7084/decompose/src/webMain/kotlin/com/arkivanov/decompose/router/webhistory/WebHistoryNavigation.kt#L20-L46).

Rust `HistoryState` на [src/router/webhistory/mod.rs:132](/Users/yet/development/rust/decompose/src/router/webhistory/mod.rs:132) имеет только `index` и `nodes`. В `HistoryController` нет version/recreation policy. [attach_impl:292](/Users/yet/development/rust/decompose/src/router/webhistory/controller.rs:292) сохраняет старую managed entry, а Pop восстанавливает nodes без schema check.

Следствие: при изменении route schema после обновления приложения старые записи могут вызвать ошибку codec/router; встроенного пути восстановления через пересоздание root нет. Это **отставание от текущего HEAD**, а не нарушение матрицы для `00840bc`.

DOM/sessionStorage wrapper остаётся намеренным platform exclusion. Но version policy и recreation callback можно перенести независимо от браузерных API.

### Намеренно отсутствует

Полный upstream содержит `jetpack-component-context`, `extensions-android`, `extensions-compose`, `extensions-compose-experimental`, платформенные bridges и UI/animation integrations. В Rust нет соответствующих host/UI адаптеров, автоматического lifecycle/Back/predictive-gesture wiring и browser DOM/sessionStorage/deep-link bootstrap.

Эти exclusions прямо указаны в [src/lib.rs](/Users/yet/development/rust/decompose/src/lib.rs) и [README.md](/Users/yet/development/rust/decompose/README.md). Для пользователя чистого core это не blocker. Для готовой навигационной библиотеки под Rust UI/web потребуются интеграции с выбранными UI/host frameworks.

Не перенесена совместимость Kotlin serialized bytes. Собственные DCS1/DST1/DSL1/DPG1/DPN1/DPI1/DCH1 formats требуют отдельного converter при миграции сохранённых данных приложения. Отсутствие Send/Sync и no_std также объявлено архитектурной границей, а не случайным пропуском Kotlin API.

## 5. Масштабирование ChildItems

Код: [src/router/items/mod.rs:68](/Users/yet/development/rust/decompose/src/router/items/mod.rs:68), validation через `items[..i].contains(c)`. Дополнительные линейные scans есть в normalize, ItemMap, а также сопоставлении old/new children в [engine.rs:516](/Users/yet/development/rust/decompose/src/router/engine.rs:516).

Проверка: уникальные Eq-конфигурации, все элементы неактивны, компоненты не создаются; `try_set_items(|items| items)` передаёт тот же список. Счётчик Eq обнулён после создания router.

| Размер списка | Сравнения Eq одного неизменившегося обновления |
|---:|---:|
| 256 | 32 640 |
| 512 | 130 816 |
| 1 024 | 523 776 |

Числа равны n(n−1)/2. Удвоение размера даёт примерно четырёхкратный рост. Это число сравнений, не измерение latency; стоимость пользовательского Eq может дополнительно влиять на время.

Eq-only API и стабильный порядок — допустимый выбор для коротких стеков. Но ChildItems позиционируется для lazy списков: lazy creation не устраняет CPU стоимость полного списка.

Направление улучшения: benchmarks на целевом объёме, optional Hash/stable-id indexed path, сохранение нынешнего Eq-only fallback. Не следует навязывать Hash всем configurations без обсуждения compatibility.

## 6. Незадокументированное отличие popWhile

Rust [stack/navigation.rs:75](/Users/yet/development/rust/decompose/src/router/stack/navigation.rs:75) прекращает predicate при длине 1. Kotlin [StackNavigatorExt.kt:117](https://github.com/arkivanov/Decompose/blob/00840bcfed6d55710c8fbb1a72503f2041eda490/decompose/src/commonMain/kotlin/com/arkivanov/decompose/router/stack/StackNavigatorExt.kt#L112-L120) сначала выполняет `dropLastWhile(predicate)`, включая root, и только затем возвращает root при пустом результате.

| Вход, predicate всегда true | Kotlin: callbacks | Rust: callbacks |
|---|---|---|
| [1, 2] | [2, 1] | [2] |
| [1] | [1] | [] |

Итоговый стек одинаков. Побочные эффекты и exception/panic на root различаются. Чистота predicate не является ограничением API.

Возможные решения: сохранить upstream callback behavior либо явно документировать различие как выбранный Rust contract. Приоритет низкий; это не отсутствие pop операции.

## 7. Насколько код соответствует Rust

### Идиоматичные решения

- `ComponentContext` как trait; конкретный context без обязательного Clone и detached factory для позднего создания children.
- `Rc`/`Weak` для разделяемого владения внутри явно single-threaded synchronous core. Arc/Mutex здесь не нужны без отдельного concurrent design.
- Manual Clone у generic handle types сохраняет возможность работать с non-Clone configurations/components. Замена на derive(Clone) добавила бы ненужные bounds.
- `Option` вместо sentinel values; uninhabited Extra в two-panel API.
- `Result`, typed errors с источниками, `NavigationOutcome::{Completed,Pending}`; explicit `*_or_panic`.
- RAII Subscription с must_use; Drop отменяет подписку; detach имеет отдельный documented contract.
- `FnOnce` для immediate initializer, допускающий borrowed captures; `'static` для действительно retained callbacks.
- Пользовательские codecs вместо обязательной зависимости serde.
- Явный DecomposeConfig вместо global mutable settings.
- Plain options structs с Default и struct update syntax.
- `Value::with/get_cloned/subscribe_ref` и `MutableValue::modify` поверх shared snapshot API.

### Сохранённые архитектурные компромиссы

- Контексты/routers имеют lifecycle-owned lifetime: drop view не означает Destroy. Это сознательно сохраняет Decompose semantics, но требует явного owner/host teardown. Дополнительный RootScope owner с RAII можно рассмотреть как удобство Rust integration.
- Навигаторы с generic callbacks используют static dispatch и не object-safe. Sources остаются object-safe. Это обоснованный tradeoff, не дефект.
- Сложные signatures Panels и callback-heavy engine отражают типизированные роли и synchronous reentry. Builder/aliases могут улучшать ergonomics; переписывать всё ради внешнего сходства со std не требуется.
- Основной engine `switch` смешивает matching, save, создание scopes, lifecycle changes, retention, publication; construction тоже объединяет множество обязанностей. Укрупнённый cleanup protocol и отделение pure matching от effects помогут обслуживанию. Это рекомендация, не отдельный доказанный regression.

Главная неидиоматичность — не названия Stack/ComponentContext, а запуск пользовательского Drop/Destroy под внутренними borrows и незащищённый Drop rollback. В Rust эти границы владения должны быть надёжнее.

## 8. Проверки

Свежие команды, exit 0:
- `rtk proxy cargo test --all-targets --locked`: **520** unit/integration tests; examples здесь только компилируются как test targets.
- `rtk proxy cargo test --doc --locked`: **1 doctest**.
- `rtk proxy cargo fmt --all -- --check`.
- `rtk proxy cargo clippy --all-targets --all-features --locked -- -D warnings`.
- `rtk proxy env RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --locked`.
- Все **15 examples** отдельно выполнены через `cargo run --locked --quiet --example ...`.
- `cargo check --locked --lib --target wasm32-unknown-unknown`.
- `cargo check --locked --lib --target aarch64-linux-android`.
- `cargo check --locked --lib --target aarch64-apple-ios`.
- `cargo package --offline --locked`: package собран и компиляционно проверен; 135 files, 1.6 MiB uncompressed, 354.5 KiB compressed.

Количество 520 дополнительно сверено по `cargo test --all-targets --locked -- --list`. Старое число 519 в compatibility matrix относится к предыдущему срезу.

Cross-compilation подтверждает type checking library, а не работу OS/browser lifecycle или linking конечного UI application.

В репозитории нет tracked `.github` workflows и `rust-toolchain(.toml)`. Рекомендую закрепить CI: fmt, strict Clippy/rustdoc, unit/integration/doc tests, несколько target checks и subprocess regression для rollback abort. `rust-version` уже задаёт minimum compiler; toolchain file нужен для воспроизводимого dev/CI environment, не для исправления текущей сборки.

Для parity нужен набор общих сценариев/trace fixtures, выполняемых Kotlin и Rust: state transitions, ordering lifecycle/observers/completion, identity, duplicate policies, save/restore, Back, rollback. 520 Rust tests не заменяют такой differential oracle.

## 9. Рекомендуемый порядок

1. Исправить StateStore unregister: не терять mutations/state.
2. Унифицировать cleanup/Drop всех scopes, исключив двойной panic при unwind.
3. Исправить освобождение instances в lifecycle/retention вне borrow.
4. Добавить regression cases по реальным симптомам выше и CI.
5. Обновить portable history policy под schemaVersion/recreation текущего upstream.
6. Решить policy для popWhile root predicate и записать в COMPATIBILITY.
7. При необходимости больших Items — benchmark и optional indexed path.
8. Реализовать host adapters под конкретную целевую UI-платформу, если задача выходит за core.

## Приложение: воспроизведения

Все программы запускались как внешние Cargo projects в /tmp с dependency:
```toml
[dependencies]
decompose-rs = { path = "/Users/yet/development/rust/decompose" }
```

У временных проектов отдельный lockfile; они использовали локальный audited decompose-rs и тот же Essenty 0.1.0. Сборка самого репозитория проверялась с его Cargo.lock. Production sources не изменялись.

### StateStore и lifecycle instance Drop

```rust
use decompose_rs::{StateStore, InstanceStore, ComponentContext, DefaultComponentContext, LifecycleRegistry};
use std::{cell::Cell, panic::{catch_unwind, AssertUnwindSafe}, rc::Rc};

struct RegisterOnDrop(StateStore, Rc<Cell<bool>>);
impl Drop for RegisterOnDrop {
    fn drop(&mut self) {
        self.0.register("replacement", || vec![42]).unwrap();
        self.1.set(self.0.is_registered("replacement").unwrap());
    }
}
struct PanicOnDrop;
impl Drop for PanicOnDrop {
    fn drop(&mut self) { panic!("provider destructor panic"); }
}
struct ReadInstanceOnDrop(InstanceStore, Rc<Cell<bool>>);
impl Drop for ReadInstanceOnDrop {
    fn drop(&mut self) { self.1.set(self.0.is_destroyed().is_ok()); }
}
fn main() {
    let store = StateStore::default();
    let registered = Rc::new(Cell::new(false));
    let capture = RegisterOnDrop(store.clone(), registered.clone());
    store.register("removed", move || { let _ = &capture; vec![1] }).unwrap();
    store.register("unrelated", || vec![2]).unwrap();
    store.unregister("removed").unwrap();
    println!("registration succeeded during Drop: {}, persists afterward: {}, unrelated retained: {}",
        registered.get(), store.is_registered("replacement").unwrap(), store.is_registered("unrelated").unwrap());
    assert!(registered.get());
    assert!(!store.is_registered("replacement").unwrap());
    let capture = PanicOnDrop;
    store.register("panicking", move || { let _ = &capture; vec![3] }).unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| store.unregister("panicking")));
    println!("unregister panicked: {}, unrelated retained afterward: {}, provider_count: {}", result.is_err(), store.is_registered("unrelated").unwrap(), store.provider_count().unwrap());
    assert!(result.is_err());
    assert!(!store.is_registered("unrelated").unwrap());
    let root = DefaultComponentContext::new(LifecycleRegistry::new());
    let instance_store = root.instance_store();
    let read_succeeded = Rc::new(Cell::new(false));
    drop(instance_store.put("value", ReadInstanceOnDrop(instance_store.clone(), read_succeeded.clone())).unwrap());
    root.lifecycle().destroy().unwrap();
    println!("instance destructor read on lifecycle destroy succeeded: {}", read_succeeded.get());
    assert!(!read_succeeded.get());
}
```

Команда:
```text
rtk proxy cargo run --offline --manifest-path /tmp/decompose-idioms-audit/Cargo.toml
```

### Abort при rollback

```rust
use decompose_rs::{ComponentContext, DefaultComponentContext, LifecycleRegistry};
use decompose_rs::router::stack::{child_stack, StackNavigation, StackOptions};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn main() {
    let parent = DefaultComponentContext::new(LifecycleRegistry::new());
    let navigation = StackNavigation::new();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = child_stack(&parent, &navigation, vec![1, 2], StackOptions::default(), |route, context| {
            if *route == 1 {
                let _ = context.lifecycle().do_on_destroy(|| panic!("cleanup panic")).detach();
            } else {
                panic!("factory panic");
            }
            context
        });
    }));
    assert!(result.is_err());
    println!("original factory panic caught");
}
```

Команда:
```text
rtk proxy cargo run --offline --manifest-path /tmp/decompose-rollback-audit/Cargo.toml --bin decompose-rollback-audit
```

Запускать отдельным процессом: ожидаемый симптом текущего кода — signal 6 / exit 134, внешний catch_unwind не достигается.

### Счётчик сравнений Items

```rust
use decompose_rs::{ComponentContext, DefaultComponentContext, LifecycleRegistry};
use decompose_rs::router::items::{child_items, Items, ItemsNavigation, ItemsNavigator, ItemsOptions};
use std::{cell::Cell, rc::Rc};

struct Route { id: usize, comparisons: Rc<Cell<usize>> }
impl PartialEq for Route {
    fn eq(&self, other: &Self) -> bool {
        self.comparisons.set(self.comparisons.get() + 1);
        self.id == other.id
    }
}
impl Eq for Route {}

fn main() {
    for n in [256, 512, 1024] {
        let comparisons = Rc::new(Cell::new(0));
        let initial = Items::new((0..n).map(|id| Route { id, comparisons: comparisons.clone() }).collect()).unwrap();
        let root = DefaultComponentContext::new(LifecycleRegistry::new());
        let navigation = ItemsNavigation::new();
        let _items = child_items(&root, &navigation, initial, ItemsOptions::default(), |_, ctx| ctx).unwrap();
        comparisons.set(0);
        let _outcome = navigation.try_set_items(|items| items).unwrap();
        println!("inactive items={n}; equality checks for unchanged membership={}", comparisons.get());
        root.lifecycle().destroy().unwrap();
    }
}
```

Команда:
```text
rtk proxy cargo run --offline --manifest-path /tmp/decompose-rollback-audit/Cargo.toml --bin equality
```

### popWhile

```rust
use decompose_rs::router::stack::{child_stack, StackNavigation, StackNavigator, StackOptions};
use decompose_rs::{DefaultComponentContext, LifecycleRegistry, LifecycleState};
use std::{cell::RefCell, rc::Rc};
fn main() {
 let l=LifecycleRegistry::new(); l.move_to(LifecycleState::Resumed).unwrap(); let ctx=DefaultComponentContext::new(l); let n=StackNavigation::new(); let stack=child_stack(&ctx,&n,vec![1,2],StackOptions::default(),|_,c|c).unwrap();
 let calls=Rc::new(RefCell::new(Vec::new())); let out=calls.clone(); let _ = n.try_pop_while(move |c|{out.borrow_mut().push(*c); true}).unwrap(); println!("two element predicate calls: {:?}; active={}",calls.borrow(),stack.get().active.configuration);
 calls.borrow_mut().clear(); let out=calls.clone(); let _ = n.try_pop_while(move |c|{out.borrow_mut().push(*c); true}).unwrap(); println!("singleton predicate calls: {:?}",calls.borrow());
}
```

Команда:
```text
rtk proxy cargo run --offline --manifest-path /tmp/decompose-popwhile-audit/Cargo.toml
```

