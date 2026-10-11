use decompose_rs::value::MutableValue;
use std::{cell::RefCell, rc::Rc};

use decompose_rs::{
    ComponentContext, TestComponentContext,
    router::{
        NavigationError, NavigationOutcome,
        stack::{StackNavigation, StackNavigator, StackOptions, child_stack},
    },
};

#[test]
fn fallible_stack_helpers_report_rejection_and_preserve_keys() {
    let context = TestComponentContext::new();
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &context,
        &navigation,
        vec![1],
        StackOptions::default(),
        |c, _| *c,
    )
    .unwrap();
    let key = stack.with(|s| s.active.key.clone());
    assert!(
        matches!(navigation.try_push(2).unwrap(), NavigationOutcome::Completed(results) if results.len() == 1)
    );
    let _ = navigation.try_pop().unwrap();
    assert_eq!(stack.with(|s| s.active.key.clone()), key);
    assert!(matches!(
        navigation.try_replace_all(vec![]),
        Err(NavigationError::EmptyStack)
    ));
    assert_eq!(stack.with(|s| *s.active.configuration), 1);
    context.lifecycle().destroy().unwrap();
}

#[test]
fn fallible_navigation_preserves_nested_fifo_and_exposes_pending_work() {
    let context = TestComponentContext::new();
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &context,
        &navigation,
        vec![1],
        StackOptions::default(),
        |c, _| *c,
    )
    .unwrap();
    let pending = Rc::new(RefCell::new(None));
    let output = pending.clone();
    let nested = navigation.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let _token = stack.subscribe_ref(move |state| {
        log.borrow_mut().push(*state.active.configuration);
        if *state.active.configuration == 2 {
            match nested.try_push(3).unwrap() {
                NavigationOutcome::Pending(receipt) => *output.borrow_mut() = Some(receipt),
                NavigationOutcome::Completed(_) => panic!("nested work must remain FIFO"),
            }
        }
    });
    let _ = navigation.try_push(2).unwrap();
    assert_eq!(*seen.borrow(), [1, 2, 3]);
    let receipt = pending.borrow_mut().take().unwrap();
    assert!(!receipt.is_pending());
    assert_eq!(receipt.take_results().unwrap().unwrap().len(), 1);
    assert!(receipt.take_results().is_none());
}

#[test]
fn a_source_without_receivers_has_an_empty_successful_result() {
    assert!(
        matches!(StackNavigation::<u8>::new().try_push(1).unwrap(), NavigationOutcome::Completed(results) if results.is_empty())
    );
}

#[test]
fn borrowed_reads_and_edits_keep_snapshots_and_replay() {
    let source = MutableValue::new(vec![1]);
    let old = source.get();
    let reader = source.value();
    let suffix = [2, 3];
    source.modify(|items| items.extend_from_slice(&suffix));
    assert_eq!(old.as_slice(), [1]);
    assert_eq!(reader.get_cloned(), [1, 2, 3]);
    let offset = 10;
    assert_eq!(reader.with(|items| items.len() + offset), 13);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    let _token = reader.subscribe_cloned(move |items| output.borrow_mut().push(items));
    source.modify(|items| items.push(4));
    assert_eq!(*seen.borrow(), [vec![1, 2, 3], vec![1, 2, 3, 4]]);
}

#[test]
fn edit_retries_reentrant_writes_without_holding_a_borrow() {
    let source = MutableValue::new(1);
    let mut calls = 0;
    source.modify(|value| {
        calls += 1;
        if calls == 1 {
            source.set(10);
        }
        *value += 1;
    });
    assert_eq!(calls, 2);
    assert_eq!(source.get_cloned(), 11);
}

#[test]
fn compare_exchange_reports_current_identity_on_failure() {
    let source = MutableValue::new(1);
    let old = source.get();
    source.set(2);
    let current = source.compare_exchange(&old, Rc::new(3)).unwrap_err();
    assert!(Rc::ptr_eq(&current, &source.get()));
    let previous = source.compare_exchange(&current, Rc::new(4)).unwrap();
    assert!(Rc::ptr_eq(&previous, &current));
    assert_eq!(source.get_cloned(), 4);
}

#[test]
fn explicit_configuration_carries_duplicate_policy_and_diagnostics() {
    use decompose_rs::{DecomposeConfig, DefaultComponentContext};
    let diagnostics = Rc::new(RefCell::new(Vec::new()));
    let output = diagnostics.clone();
    let config = DecomposeConfig::default()
        .with_error_handler(move |error| output.borrow_mut().push(error.clone()));
    let root = TestComponentContext::new();
    let context = DefaultComponentContext::new(root.lifecycle()).with_config(config.clone());
    let navigation = StackNavigation::new();
    let options = StackOptions::with_config(&config);
    assert!(!options.allow_duplicates);
    let stack = child_stack(&context, &navigation, vec![1], options, |c, _| *c).unwrap();
    assert!(matches!(
        navigation.try_push(1),
        Err(NavigationError::DuplicateConfiguration)
    ));
    assert_eq!(stack.with(|s| *s.active.configuration), 1);
    let child = context.child_context("nested").unwrap();
    assert!(child.configuration().is_some());
    let result = child_stack(
        &child,
        &StackNavigation::new(),
        vec![1, 1],
        StackOptions::with_config(&config),
        |c, _| *c,
    );
    assert!(matches!(
        result,
        Err(NavigationError::DuplicateConfiguration)
    ));
    root.lifecycle().destroy().unwrap();
    assert_eq!(diagnostics.borrow().len(), 2);
}

#[test]
fn keeper_services_return_owned_data_and_allow_factory_reentry() {
    use decompose_rs::{InstanceAccessError, InstanceKeeperError};
    let driver = TestComponentContext::new();
    let context = decompose_rs::DefaultComponentContext::new(driver.lifecycle());
    let state = context.state_store();
    state.register("answer", || vec![42]).unwrap();
    assert_eq!(state.save().unwrap()["answer"], [42]);
    let instances = context.instance_store();
    let borrowed = String::from("local service");
    let value = instances
        .get_or_create("service", || {
            // The factory runs outside internal keeper borrows.
            instances.put("other", 7_u8).unwrap();
            borrowed.len()
        })
        .unwrap();
    assert_eq!(*value, borrowed.len());
    assert_eq!(*instances.get::<u8>("other").unwrap().unwrap(), 7);
    assert!(matches!(
        instances.get::<u8>("service"),
        Err(InstanceAccessError::Keeper(
            InstanceKeeperError::TypeMismatch(_)
        ))
    ));
    // No externally held guard can block child construction.
    let _child = context.child_context("child").unwrap();
    context.lifecycle().destroy().unwrap();
    assert!(matches!(
        instances.put("late", 1),
        Err(InstanceAccessError::Keeper(InstanceKeeperError::Destroyed))
    ));
}

#[test]
fn opaque_stores_expose_owned_results_without_borrow_guards() {
    let context = TestComponentContext::new();
    context.state_store().register("key", || vec![1]).unwrap();
    assert_eq!(context.state_store().save().unwrap()["key"], vec![1]);
    assert!(context.state_store().is_registered("key").unwrap());
}

#[test]
fn history_sync_adapter_preserves_typed_error_sources() {
    use decompose_rs::router::webhistory::{
        HistoryError, HistoryOutcome, Route, RouteCodec, TypedWebNavigation, WebNavigation,
    };
    use std::error::Error;
    let routes = MutableValue::new(vec![Route::from_key(1_u8, "one")]);
    let codec = RouteCodec::new(
        |key| Ok(vec![*key]),
        |bytes| {
            bytes
                .first()
                .copied()
                .ok_or_else(|| HistoryError::codec_source(std::io::Error::other("missing key")))
        },
    );
    let model = TypedWebNavigation::new_sync(routes.value(), codec, |_| Ok(()));
    assert!(matches!(
        model.try_navigate(vec![vec![2]]).unwrap(),
        HistoryOutcome::Completed
    ));
    let error = model.try_navigate(vec![vec![]]).unwrap_err();
    let source = error.source().unwrap();
    assert!(source.source().unwrap().is::<std::io::Error>());
}

#[test]
fn history_pending_receipt_completes_once_without_resubmission() {
    use decompose_rs::router::webhistory::{
        HistoryCompletion, HistoryOutcome, Route, RouteCodec, TypedWebNavigation, WebNavigation,
    };
    let routes = MutableValue::new(vec![Route::from_key(1_u8, "one")]);
    let pending = Rc::new(RefCell::new(None::<HistoryCompletion>));
    let output = pending.clone();
    let codec = RouteCodec::new(|key| Ok(vec![*key]), |bytes| Ok(bytes[0]));
    let model = TypedWebNavigation::new(routes.value(), codec, move |_, completion| {
        *output.borrow_mut() = Some(completion)
    });
    let HistoryOutcome::Pending(receipt) = model.try_navigate(vec![vec![2]]).unwrap() else {
        panic!("completion was deferred");
    };
    assert!(receipt.is_pending());
    assert!(receipt.take_result().is_none());
    pending.borrow_mut().take().unwrap()(Ok(()));
    assert!(!receipt.is_pending());
    assert!(receipt.take_result().unwrap().is_ok());
    assert!(receipt.take_result().is_none());
}

#[test]
fn typed_stack_codec_keeps_bytes_and_reports_original_source() {
    use decompose_rs::{
        DefaultComponentContext, StateKeeper, StateStore, router::stack::StackCodec,
    };
    use std::error::Error;
    fn save(codec: StackCodec<u8>) -> std::collections::BTreeMap<String, Vec<u8>> {
        let driver = TestComponentContext::new();
        let context = DefaultComponentContext::new(driver.lifecycle());
        let _stack = child_stack(
            &context,
            &StackNavigation::new(),
            vec![1],
            StackOptions {
                codec: Some(codec),
                ..StackOptions::new()
            },
            |c, _| *c,
        )
        .unwrap();
        context.state_store().save().unwrap()
    }
    let legacy = save(StackCodec::new(
        |items| Ok(items.iter().map(|c| **c).collect()),
        |bytes| Ok(bytes.to_vec()),
    ));
    let typed = save(StackCodec::new_fallible(
        |items| Ok(items.iter().map(|c| **c).collect()),
        |bytes| Ok(bytes.to_vec()),
    ));
    assert_eq!(legacy, typed);
    let driver = TestComponentContext::new();
    let context = DefaultComponentContext::with_state_store(
        driver.lifecycle(),
        StateStore::from(StateKeeper::with_restored(typed)),
    );
    let result = child_stack(
        &context,
        &StackNavigation::new(),
        vec![2],
        StackOptions {
            codec: Some(StackCodec::new_fallible(
                |_| Ok(vec![]),
                |_| {
                    Err(NavigationError::state_source(std::io::Error::other(
                        "decoder failure",
                    )))
                },
            )),
            ..StackOptions::new()
        },
        |c, _| *c,
    );
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("decoder must fail"),
    };
    assert!(
        error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<std::io::Error>()
    );
}

#[test]
fn owned_map_operations_preserve_eq_only_insertion_order() {
    use decompose_rs::router::items::ItemMap;
    #[derive(PartialEq, Eq)]
    struct Key(u8); // Deliberately lacks Hash and Ord.
    let mut map = ItemMap::new();
    map.insert_owned(Key(2), "two");
    map.insert_owned(Key(1), "one");
    assert_eq!(map.insert_owned(Key(2), "second"), Some("two"));
    assert_eq!(
        map.iter_ref().map(|(k, v)| (k.0, *v)).collect::<Vec<_>>(),
        [(2, "second"), (1, "one")]
    );
}

#[test]
fn result_broadcast_reports_first_failure_after_other_receivers_commit() {
    let context = TestComponentContext::new();
    let navigation = StackNavigation::new();
    let first = child_stack(
        &context,
        &navigation,
        vec![1],
        StackOptions {
            key: "first".into(),
            ..StackOptions::new()
        },
        |c, _| *c,
    )
    .unwrap();
    let second = child_stack(
        &context,
        &navigation,
        vec![1],
        StackOptions {
            key: "second".into(),
            allow_duplicates: true,
            ..StackOptions::new()
        },
        |c, _| *c,
    )
    .unwrap();
    assert!(matches!(
        navigation.try_push(1),
        Err(NavigationError::DuplicateConfiguration)
    ));
    assert_eq!(first.with(|s| s.items().count()), 1);
    assert_eq!(second.with(|s| s.items().count()), 2);
}

#[test]
fn releasing_instance_values_runs_destructors_outside_store_borrows() {
    use decompose_rs::InstanceStore;
    struct Reenter(InstanceStore);
    impl Drop for Reenter {
        fn drop(&mut self) {
            assert!(self.0.len().is_ok());
        }
    }
    let store = InstanceStore::default();
    let value = store.put("first", Reenter(store.clone())).unwrap();
    drop(value);
    let rejected = store.put("first", Reenter(store.clone()));
    assert!(rejected.is_err());
    store.clear().unwrap();
    assert!(store.is_empty().unwrap());
    let value = store.put("next", Reenter(store.clone())).unwrap();
    drop(value);
    store.destroy_all().unwrap();
    assert!(store.is_destroyed().unwrap());
}

#[test]
fn provider_captures_drop_outside_store_borrows() {
    use decompose_rs::StateStore;
    struct Reenter(StateStore, Rc<RefCell<Vec<bool>>>);
    impl Drop for Reenter {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0.pending_count().is_ok());
        }
    }
    let store = StateStore::default();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let capture = Reenter(store.clone(), seen.clone());
    store
        .register("key", move || {
            let _ = &capture;
            vec![1]
        })
        .unwrap();
    let rejected = Reenter(store.clone(), seen.clone());
    assert!(
        store
            .register("key", move || {
                let _ = &rejected;
                vec![2]
            })
            .is_err()
    );
    assert!(store.unregister("key").unwrap());
    assert_eq!(*seen.borrow(), [true, true]);
}

#[test]
fn custom_helpers_use_navigate_with_override() {
    use decompose_rs::router::stack::{NavigationResult, StackNavigator};
    use std::cell::Cell;
    struct Custom(Cell<usize>);
    impl StackNavigator<u8> for Custom {
        fn navigate_with(
            &self,
            _: impl Fn(Vec<Rc<u8>>) -> Vec<Rc<u8>> + 'static,
            _: impl Fn(Result<NavigationResult<u8>, NavigationError>) + 'static,
        ) {
            self.0.set(self.0.get() + 1);
        }
    }
    let navigation = Custom(Cell::new(0));
    navigation.push_or_panic(1);
    navigation.pop_or_panic();
    navigation.replace_all_or_panic(vec![2]);
    assert_eq!(navigation.0.get(), 3);
}

#[test]
fn abandoned_nested_navigation_reports_failure() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let context = TestComponentContext::new();
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &context,
        &navigation,
        vec![1],
        StackOptions::new(),
        |c, _| *c,
    )
    .unwrap();
    let pending = Rc::new(RefCell::new(None));
    let output = pending.clone();
    let nested = navigation.clone();
    let _token = stack.subscribe_ref(move |state| {
        if *state.active.configuration == 2 {
            let NavigationOutcome::Pending(receipt) = nested.try_push(3).unwrap() else {
                panic!("expected pending");
            };
            *output.borrow_mut() = Some(receipt);
            panic!("observer panic abandons queued event");
        }
    });
    assert!(catch_unwind(AssertUnwindSafe(|| navigation.try_push(2))).is_err());
    let receipt = pending.borrow_mut().take().unwrap();
    assert!(!receipt.is_pending());
    assert!(receipt.take_results().unwrap().is_err());
    assert_eq!(stack.with(|state| *state.active.configuration), 2);
}
