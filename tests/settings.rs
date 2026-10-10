use decompose_rs::router::{
    NavigationError,
    stack::{StackNavigation, StackOptions, child_stack},
};
use decompose_rs::{
    DecomposeDiagnostic, DecomposeSettings, DiagnosticOperation, TestComponentContext,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[test]
fn defaults_snapshot_global_duplicate_policy_and_explicit_false_wins() {
    let old_options = StackOptions::<u8>::default();
    DecomposeSettings::with(
        DecomposeSettings {
            duplicate_configurations_enabled: true,
            ..Default::default()
        },
        || {
            let context = TestComponentContext::new();
            let source = StackNavigation::new();
            assert!(
                child_stack(
                    &context,
                    &source,
                    vec![1, 1],
                    StackOptions::default(),
                    |_, _| ()
                )
                .is_ok()
            );
            assert!(decompose_rs::router::pages::PagesOptions::<u8>::default().allow_duplicates);
            assert!(
                decompose_rs::router::children::ChildrenOptions::<
                    (),
                    Vec<decompose_rs::router::children::ChildNavState<u8>>,
                >::default()
                .allow_duplicates
            );
            let context = TestComponentContext::new();
            assert!(matches!(
                child_stack(&context, &source, vec![1, 1], old_options, |_, _| ()),
                Err(NavigationError::DuplicateConfiguration)
            ));
            let context = TestComponentContext::new();
            let explicit = StackOptions {
                key: "explicit".into(),
                handle_back_button: false,
                allow_duplicates: false,
                codec: None,
            };
            assert!(matches!(
                child_stack(&context, &source, vec![1, 1], explicit, |_, _| ()),
                Err(NavigationError::DuplicateConfiguration)
            ));
        },
    );
    assert!(!DecomposeSettings::current().duplicate_configurations_enabled);
}

#[test]
fn construction_diagnostics_run_after_rollback_and_allow_same_key_retry() {
    let context = Rc::new(TestComponentContext::new());
    let source = StackNavigation::new();
    let target = context.clone();
    let nested = source.clone();
    let restored = Rc::new(Cell::new(false));
    let output = restored.clone();
    DecomposeSettings::with(
        DecomposeSettings {
            on_decompose_error: Rc::new(move |diagnostic| {
                assert!(matches!(
                    diagnostic,
                    DecomposeDiagnostic::Navigation {
                        operation: DiagnosticOperation::Construction,
                        error: NavigationError::EmptyStack,
                        ..
                    }
                ));
                assert!(
                    child_stack(&*target, &nested, vec![1u8], Default::default(), |_, _| ())
                        .is_ok()
                );
                output.set(true);
            }),
            ..Default::default()
        },
        || {
            assert!(matches!(
                child_stack(&*context, &source, vec![], Default::default(), |_, _| ()),
                Err(NavigationError::EmptyStack)
            ));
        },
    );
    assert!(restored.get());
}

#[test]
fn convenience_navigation_still_panics_after_diagnostics() {
    let context = TestComponentContext::new();
    let source = StackNavigation::new();
    let _stack = child_stack(&context, &source, vec![1u8], Default::default(), |_, _| ()).unwrap();
    let called = Rc::new(Cell::new(false));
    let output = called.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        DecomposeSettings::with(
            DecomposeSettings {
                on_decompose_error: Rc::new(move |_| output.set(true)),
                ..Default::default()
            },
            || source.replace_all(Vec::new()),
        )
    }));
    assert!(result.is_err());
    assert!(called.get());
    source.push(2);
}

#[test]
fn scoped_settings_restore_after_unwind_and_reentrant_updates_retry() {
    let result = std::panic::catch_unwind(|| {
        DecomposeSettings::with(
            DecomposeSettings {
                duplicate_configurations_enabled: true,
                ..Default::default()
            },
            || panic!("scope"),
        )
    });
    assert!(result.is_err());
    assert!(!DecomposeSettings::current().duplicate_configurations_enabled);
    let calls = Cell::new(0);
    DecomposeSettings::with(Default::default(), || {
        DecomposeSettings::update(|mut settings| {
            if calls.replace(calls.get() + 1) == 0 {
                DecomposeSettings::update(|mut inner| {
                    inner.duplicate_configurations_enabled = true;
                    inner
                });
            }
            assert_eq!(settings.duplicate_configurations_enabled, calls.get() > 1);
            settings.on_decompose_error = Rc::new(|_| {});
            settings
        });
        assert_eq!(calls.get(), 2);
        assert!(DecomposeSettings::current().duplicate_configurations_enabled);
    });
}

#[test]
fn errors_remain_typed_and_handler_can_update_settings_and_reenter_navigation() {
    let context = TestComponentContext::new();
    let source = StackNavigation::new();
    let stack = child_stack(&context, &source, vec![1u8], Default::default(), |_, _| ()).unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let output = log.clone();
    let nested = source.clone();
    let settings = DecomposeSettings {
        on_decompose_error: Rc::new(move |diagnostic| {
            output.borrow_mut().push(diagnostic.clone());
            DecomposeSettings::update(|mut settings| {
                settings.duplicate_configurations_enabled = true;
                settings
            });
            nested.push(2);
        }),
        ..Default::default()
    };
    DecomposeSettings::with(settings, || {
        let result = Rc::new(RefCell::new(None));
        let completed = result.clone();
        source.navigate_with(
            |_| Vec::new(),
            move |value| *completed.borrow_mut() = Some(value),
        );
        assert!(matches!(
            *result.borrow(),
            Some(Err(NavigationError::EmptyStack))
        ));
        assert_eq!(*stack.get().active.configuration, 2);
    });
    assert!(matches!(
        log.borrow().as_slice(),
        [DecomposeDiagnostic::Navigation {
            operation: DiagnosticOperation::Navigation,
            error: NavigationError::EmptyStack,
            ..
        }]
    ));
}

#[test]
fn nested_reporting_is_suppressed_and_panic_releases_guard() {
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    DecomposeSettings::with(
        DecomposeSettings {
            on_decompose_error: Rc::new(move |error| {
                output.set(output.get() + 1);
                DecomposeSettings::report_error(error.clone());
            }),
            ..Default::default()
        },
        || {
            DecomposeSettings::report_error(DecomposeDiagnostic::Navigation {
                operation: DiagnosticOperation::Navigation,
                key: "test".into(),
                error: NavigationError::EmptyStack,
            });
        },
    );
    assert_eq!(calls.get(), 1);
    let error = DecomposeDiagnostic::Navigation {
        operation: DiagnosticOperation::Navigation,
        key: "test".into(),
        error: NavigationError::EmptyStack,
    };
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        DecomposeSettings::with(
            DecomposeSettings {
                on_decompose_error: Rc::new(|_| panic!("diagnostic")),
                ..Default::default()
            },
            || DecomposeSettings::report_error(error.clone()),
        )
    }));
    assert!(panicked.is_err());
    let output = calls.clone();
    DecomposeSettings::with(
        DecomposeSettings {
            on_decompose_error: Rc::new(move |_| output.set(output.get() + 1)),
            ..Default::default()
        },
        || DecomposeSettings::report_error(error),
    );
    assert_eq!(calls.get(), 2);
}

#[test]
fn rejected_navigation_emitted_by_error_handler_keeps_completion_without_recursive_diagnostics() {
    let context = TestComponentContext::new();
    let source = StackNavigation::new();
    let _stack = child_stack(&context, &source, vec![1u8], Default::default(), |_, _| ()).unwrap();
    let calls = Rc::new(Cell::new(0));
    let completions = Rc::new(Cell::new(0));
    let count = calls.clone();
    let completed = completions.clone();
    let nested = source.clone();
    DecomposeSettings::with(
        DecomposeSettings {
            on_decompose_error: Rc::new(move |_| {
                count.set(count.get() + 1);
                // The cap makes a missing suppression guard fail without hanging.
                if count.get() < 3 {
                    let output = completed.clone();
                    nested.navigate_with(
                        |_| Vec::new(),
                        move |result| {
                            assert!(matches!(result, Err(NavigationError::EmptyStack)));
                            output.set(output.get() + 1);
                        },
                    );
                }
            }),
            ..Default::default()
        },
        || {
            let completed = completions.clone();
            source.navigate_with(
                |_| Vec::new(),
                move |result| {
                    assert!(matches!(result, Err(NavigationError::EmptyStack)));
                    completed.set(completed.get() + 1);
                },
            );
        },
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(completions.get(), 2);
}

#[test]
fn handler_origin_completion_panic_releases_source_and_diagnostic_guards() {
    let context = TestComponentContext::new();
    let source = StackNavigation::new();
    let _stack = child_stack(&context, &source, vec![1u8], Default::default(), |_, _| ()).unwrap();
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    let nested = source.clone();
    DecomposeSettings::with(
        DecomposeSettings {
            on_decompose_error: Rc::new(move |_| {
                output.set(output.get() + 1);
                if output.get() == 1 {
                    nested.navigate_with(|_| Vec::new(), |_| panic!("nested completion"));
                }
            }),
            ..Default::default()
        },
        || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                source.navigate_with(|_| Vec::new(), |_| {})
            }));
            assert!(result.is_err());
            assert_eq!(calls.get(), 1);
            source.navigate_with(
                |_| Vec::new(),
                |result| assert!(matches!(result, Err(NavigationError::EmptyStack))),
            );
            assert_eq!(calls.get(), 2);
            source.push(2);
        },
    );
}
