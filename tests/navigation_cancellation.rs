use decompose_rs::{
    BackPhase, ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState,
    router::{
        NavigationError, NavigationOutcome,
        stack::{StackNavigation, StackNavigator, StackOptions, child_stack},
    },
};
use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

fn root() -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::new(lifecycle)
}

#[test]
fn parent_destroy_completes_accepted_pending_receipt_with_error_once() {
    let parent = root();
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &parent,
        &navigation,
        vec![0],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let pending = Rc::new(RefCell::new(None));
    let output = pending.clone();
    let nested = navigation.clone();
    let lifecycle = parent.lifecycle();
    parent
        .back_handler()
        .register(100, true, move |event, _| {
            if event.phase == BackPhase::Invoked {
                let NavigationOutcome::Pending(receipt) = nested.try_push(1).unwrap() else {
                    panic!("expected accepted deferred navigation");
                };
                assert!(receipt.is_pending());
                *output.borrow_mut() = Some(receipt);
                lifecycle.destroy().unwrap();
            }
        })
        .unwrap();
    parent.back_handler().back();
    let receipt = pending.borrow_mut().take().unwrap();
    assert!(!receipt.is_pending());
    assert!(matches!(
        receipt.take_results(),
        Some(Err(NavigationError::ChildUnavailable))
    ));
    assert!(receipt.take_results().is_none());
    assert_eq!(*stack.get().active.configuration, 0);
    assert!(parent.lifecycle().state().is_destroyed());
    assert_eq!(parent.state_store().provider_count().unwrap(), 0);
    assert!(
        matches!(navigation.try_push(2).unwrap(), NavigationOutcome::Completed(results) if results.is_empty())
    );
}

#[test]
fn back_unwind_completes_pending_receipt_and_navigation_recovers() {
    let parent = root();
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &parent,
        &navigation,
        vec![0],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let pending = Rc::new(RefCell::new(None));
    let output = pending.clone();
    let nested = navigation.clone();
    let id = parent
        .back_handler()
        .register(100, true, move |event, _| {
            if event.phase == BackPhase::Invoked {
                let NavigationOutcome::Pending(receipt) = nested.try_push(1).unwrap() else {
                    panic!("expected accepted deferred navigation");
                };
                *output.borrow_mut() = Some(receipt);
                panic!("original Back panic");
            }
        })
        .unwrap();
    let panic = catch_unwind(AssertUnwindSafe(|| parent.back_handler().back())).unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"original Back panic"));
    let receipt = pending.borrow_mut().take().unwrap();
    assert!(matches!(
        receipt.take_results(),
        Some(Err(NavigationError::ChildUnavailable))
    ));
    assert!(receipt.take_results().is_none());
    assert_eq!(*stack.get().active.configuration, 0);
    parent.back_handler().unregister(id.id());
    assert!(
        matches!(navigation.try_push(2).unwrap(), NavigationOutcome::Completed(results) if results.len() == 1)
    );
    assert_eq!(*stack.get().active.configuration, 2);
}

#[test]
fn cancellation_is_reported_per_receiver_without_rolling_back_successful_receiver() {
    let first = root();
    let second = root();
    let navigation = StackNavigation::new();
    let first_stack = child_stack(
        &first,
        &navigation,
        vec![0],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let second_stack = child_stack(
        &second,
        &navigation,
        vec![10],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let pending = Rc::new(RefCell::new(None));
    let output = pending.clone();
    let nested = navigation.clone();
    let lifecycle = first.lifecycle();
    first
        .back_handler()
        .register(100, true, move |event, _| {
            if event.phase == BackPhase::Invoked {
                let NavigationOutcome::Pending(receipt) = nested.try_push(1).unwrap() else {
                    panic!("expected pending first receiver");
                };
                *output.borrow_mut() = Some(receipt);
                lifecycle.destroy().unwrap();
            }
        })
        .unwrap();
    first.back_handler().back();
    let receipt = pending.borrow_mut().take().unwrap();
    assert!(matches!(
        receipt.take_results(),
        Some(Err(NavigationError::ChildUnavailable))
    ));
    assert_eq!(*first_stack.get().active.configuration, 0);
    assert_eq!(*second_stack.get().active.configuration, 1);
    assert!(
        matches!(navigation.try_push(2).unwrap(), NavigationOutcome::Completed(results) if results.len() == 1)
    );
    assert_eq!(*second_stack.get().active.configuration, 2);
}

#[test]
fn panicking_completions_finish_independent_events_and_preserve_original_unwind() {
    for destroy in [false, true] {
        let parent = root();
        let navigation = StackNavigation::new();
        let stack = child_stack(
            &parent,
            &navigation,
            vec![0],
            StackOptions::default(),
            |_, ctx| ctx,
        )
        .unwrap();
        let completed = Rc::new(RefCell::new(Vec::new()));
        let dropped = Rc::new(Cell::new(0));
        struct Capture(Rc<Cell<usize>>);
        impl Drop for Capture {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        let output = completed.clone();
        let releases = dropped.clone();
        let nested = navigation.clone();
        let lifecycle = parent.lifecycle();
        parent
            .back_handler()
            .register(100, true, move |event, _| {
                if event.phase == BackPhase::Invoked {
                    for key in [1, 2, 3] {
                        let log = output.clone();
                        let capture = Capture(releases.clone());
                        nested.navigate_with(
                            move |mut state| {
                                let _ = &capture;
                                state.push(Rc::new(key));
                                state
                            },
                            move |result| {
                                assert!(matches!(result, Err(NavigationError::ChildUnavailable)));
                                log.borrow_mut().push(key);
                                if key != 3 {
                                    panic!("completion {key}");
                                }
                            },
                        );
                    }
                    if destroy {
                        lifecycle.destroy().unwrap();
                    } else {
                        panic!("original Back panic");
                    }
                }
            })
            .unwrap();
        let panic = catch_unwind(AssertUnwindSafe(|| parent.back_handler().back())).unwrap_err();
        if destroy {
            assert_eq!(
                panic.downcast_ref::<String>().map(String::as_str),
                Some("completion 1")
            );
        } else {
            assert_eq!(panic.downcast_ref::<&str>(), Some(&"original Back panic"));
        }
        assert_eq!(*completed.borrow(), [1, 2, 3]);
        assert_eq!(dropped.get(), 3);
        assert_eq!(*stack.get().active.configuration, 0);
        if destroy {
            assert_eq!(parent.state_store().provider_count().unwrap(), 0);
        } else {
            assert!(
                matches!(navigation.try_push(4).unwrap(), NavigationOutcome::Completed(results) if results.len() == 1)
            );
            assert_eq!(*stack.get().active.configuration, 4);
        }
    }
}

#[test]
fn source_without_receivers_legitimately_completes_with_empty_results() {
    let navigation = StackNavigation::new();
    assert!(
        matches!(navigation.try_push(1).unwrap(), NavigationOutcome::Completed(results) if results.is_empty())
    );
}
