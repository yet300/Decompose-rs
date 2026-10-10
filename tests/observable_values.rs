#![allow(clippy::redundant_clone)]
use decompose_rs::value::{MutableValue, ObserveLifecycleMode, Subscription, Value};
use decompose_rs::{LifecycleRegistry, LifecycleState};
use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

#[test]
fn identity_cas_rejects_equal_payload_and_emits_same_snapshot() {
    struct Payload(i32);
    let initial = Rc::new(Payload(1));
    let value = MutableValue::from_rc(initial.clone());
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let _token = value.value().subscribe(move |_| count.set(count.get() + 1));
    assert!(
        value
            .compare_exchange(&Rc::new(Payload(1)), Rc::new(Payload(2)))
            .is_err()
    );
    assert_eq!(calls.get(), 1);
    assert!(Rc::ptr_eq(&value.get(), &initial));
    assert!(value.compare_exchange(&initial, initial.clone()).is_ok());
    assert_eq!(calls.get(), 2);
    assert_eq!(value.get().0, 1);
}

#[test]
fn update_retries_stale_snapshot_and_returns_exact_committed_identity() {
    let value = MutableValue::new(0);
    let writer = value.clone();
    let attempts = Cell::new(0);
    let committed = Rc::new(11);
    let returned = value.update_rc_and_get(|old| {
        attempts.set(attempts.get() + 1);
        if *old == 0 {
            writer.set(10);
            Rc::new(1)
        } else {
            committed.clone()
        }
    });
    assert_eq!(attempts.get(), 2);
    assert!(Rc::ptr_eq(&returned, &committed));
    let writer = value.clone();
    let _token = value.value().subscribe(move |snapshot| {
        if *snapshot == 12 {
            writer.set(13);
        }
    });
    let next = Rc::new(12);
    assert!(Rc::ptr_eq(
        &value.update_rc_and_get(|_| next.clone()),
        &next
    ));
    assert_eq!(*value.get(), 13);
    let old = value.get();
    assert!(Rc::ptr_eq(&value.get_and_update_rc(|_| Rc::new(14)), &old));
    value.update_rc(|old| Rc::new(*old + 1));
    assert_eq!(*value.get(), 15);
}

#[test]
fn transformer_panic_keeps_reentrant_effects_and_no_payload_bounds() {
    struct Payload(i32);
    let value = MutableValue::new(Payload(0));
    assert!(
        catch_unwind(AssertUnwindSafe(|| value.update_rc(|_| {
            value.set(Payload(7));
            panic!("transformer");
        })))
        .is_err()
    );
    assert_eq!(value.get().0, 7);
    value.update_rc(|old| Rc::new(Payload(old.0 + 1)));
    assert_eq!(value.get().0, 8);
}

#[test]
fn mapped_cache_is_eager_shared_and_lazy_without_observers() {
    struct Input(i32);
    struct Output(i32);
    let value = MutableValue::new(Input(1));
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let mapped = value.value().map(move |input| {
        count.set(count.get() + 1);
        Output(input.0 * 2)
    });
    assert_eq!(calls.get(), 1);
    let initial = mapped.get();
    assert!(Rc::ptr_eq(&initial, &mapped.clone().get()));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = seen.clone();
    let first = mapped.subscribe(move |x| out.borrow_mut().push(x));
    let out = seen.clone();
    let second = mapped.subscribe(move |x| out.borrow_mut().push(x));
    assert!(Rc::ptr_eq(&seen.borrow()[0], &seen.borrow()[1]));
    assert_eq!(calls.get(), 1);
    value.set(Input(2));
    assert_eq!(calls.get(), 2);
    assert!(Rc::ptr_eq(&seen.borrow()[2], &seen.borrow()[3]));
    assert!(Rc::ptr_eq(&seen.borrow()[2], &mapped.get()));
    value.set_rc(value.get());
    assert_eq!(calls.get(), 2);
    drop(first);
    second.cancel();
    value.set(Input(3));
    assert_eq!(calls.get(), 2);
    assert_eq!(mapped.get().0, 6);
    assert_eq!(calls.get(), 3);
    value.set(Input(3));
    assert_eq!(mapped.get().0, 6);
    assert_eq!(calls.get(), 4);
}

#[test]
fn mapped_chain_and_detach_keep_observers_until_mapped_handle_drop() {
    let value = MutableValue::new(1);
    let mapped = value.value().map(|x| x * 2).map(|x| x.to_string());
    let capture = Rc::new(RefCell::new(Vec::new()));
    let weak = Rc::downgrade(&capture);
    mapped
        .subscribe(move |x| capture.borrow_mut().push((*x).clone()))
        .detach();
    value.set(2);
    assert_eq!(*weak.upgrade().unwrap().borrow(), ["2", "4"]);
    drop(mapped);
    assert!(weak.upgrade().is_none());
}

#[test]
fn mapper_reentry_preserves_newer_cache_and_same_input_guard_recovers() {
    let value = MutableValue::new(0);
    let slot = Rc::new(RefCell::new(None::<Value<i32>>));
    let reader = slot.clone();
    let writer = value.clone();
    let mapped = value.value().map(move |x| {
        if *x == 1 {
            writer.set(2);
            assert_eq!(*reader.borrow().as_ref().unwrap().get(), 20);
        }
        x * 10
    });
    *slot.borrow_mut() = Some(mapped.clone());
    value.set(1);
    assert_eq!(*mapped.get(), 10);
    let newer = mapped.get();
    assert_eq!(*newer, 20);
    assert!(Rc::ptr_eq(&newer, &mapped.get()));
    let recurse = Rc::new(Cell::new(true));
    let flag = recurse.clone();
    let slot2 = Rc::new(RefCell::new(None::<Value<i32>>));
    let reader2 = slot2.clone();
    let guarded = value.value().map(move |x| {
        if *x == 3 && flag.get() {
            reader2.borrow().as_ref().unwrap().get();
        }
        *x
    });
    *slot2.borrow_mut() = Some(guarded.clone());
    value.set(3);
    assert!(catch_unwind(AssertUnwindSafe(|| guarded.get())).is_err());
    recurse.set(false);
    assert_eq!(*guarded.get(), 3);
    slot.borrow_mut().take();
    slot2.borrow_mut().take();
}

#[test]
fn mapper_panic_is_retryable_with_and_without_observers() {
    let value = MutableValue::new(0);
    let panic_once = Rc::new(Cell::new(true));
    let flag = panic_once.clone();
    let mapped = value.value().map(move |x| {
        if *x == 1 && flag.replace(false) {
            panic!("mapper");
        }
        x * 2
    });
    value.set(1);
    assert!(catch_unwind(AssertUnwindSafe(|| mapped.get())).is_err());
    assert_eq!(*mapped.get(), 2);
    let _token = mapped.subscribe(|_| {});
    panic_once.set(true);
    assert!(catch_unwind(AssertUnwindSafe(|| value.set(1))).is_err());
    assert_eq!(*mapped.get(), 2);
    value.set(2);
    assert_eq!(*mapped.get(), 4);
}

#[test]
fn lifecycle_modes_gate_each_initial_state_and_replay_on_reopen() {
    assert_eq!(
        ObserveLifecycleMode::default(),
        ObserveLifecycleMode::StartStop
    );
    for (mode, threshold) in [
        (ObserveLifecycleMode::CreateDestroy, LifecycleState::Created),
        (ObserveLifecycleMode::StartStop, LifecycleState::Started),
        (ObserveLifecycleMode::ResumePause, LifecycleState::Resumed),
    ] {
        for state in [
            LifecycleState::Initialized,
            LifecycleState::Created,
            LifecycleState::Started,
            LifecycleState::Resumed,
            LifecycleState::Destroyed,
        ] {
            let lifecycle = LifecycleRegistry::new();
            if state != LifecycleState::Initialized {
                lifecycle.move_to(state).unwrap();
            }
            let value = MutableValue::new(0);
            let seen = Rc::new(RefCell::new(Vec::new()));
            let output = seen.clone();
            let token = value
                .value()
                .subscribe_with_lifecycle(&lifecycle, mode, move |x| output.borrow_mut().push(*x));
            let active = state >= threshold && state != LifecycleState::Destroyed;
            assert_eq!(seen.borrow().len(), usize::from(active));
            value.set(1);
            assert_eq!(seen.borrow().len(), if active { 2 } else { 0 });
            if state != LifecycleState::Destroyed {
                lifecycle.move_to(LifecycleState::Resumed).unwrap();
                assert_eq!(*seen.borrow().last().unwrap(), 1);
                lifecycle.move_to(LifecycleState::Created).unwrap();
                value.set(2);
                if mode != ObserveLifecycleMode::CreateDestroy {
                    assert_eq!(*seen.borrow().last().unwrap(), 1);
                }
                lifecycle.move_to(LifecycleState::Resumed).unwrap();
                assert_eq!(*seen.borrow().last().unwrap(), 2);
                lifecycle.destroy().unwrap();
                assert_eq!(lifecycle.subscriber_count(), 0);
            }
            token.cancel();
        }
    }
}

#[test]
fn lifecycle_cancel_drop_detach_destroy_release_captures() {
    for action in 0..4 {
        let lifecycle = LifecycleRegistry::new();
        lifecycle.move_to(LifecycleState::Resumed).unwrap();
        let value = MutableValue::new(0);
        let capture = Rc::new(Cell::new(0));
        let weak = Rc::downgrade(&capture);
        let token = value.value().subscribe_with_lifecycle(
            &lifecycle,
            ObserveLifecycleMode::StartStop,
            move |_| capture.set(capture.get() + 1),
        );
        match action {
            0 => {
                token.cancel();
                token.cancel();
            }
            1 => drop(token),
            2 => {
                token.detach();
                value.set(1);
                assert_eq!(weak.upgrade().unwrap().get(), 2);
                lifecycle.destroy().unwrap();
            }
            _ => {
                lifecycle.destroy().unwrap();
                drop(token);
            }
        }
        assert!(weak.upgrade().is_none());
        assert_eq!(lifecycle.subscriber_count(), 0);
    }
}

#[test]
fn lifecycle_replay_shutdown_cancels_late_returned_source_token() {
    for destroy in [false, true] {
        let lifecycle = LifecycleRegistry::new();
        lifecycle.move_to(LifecycleState::Resumed).unwrap();
        let value = MutableValue::new(0);
        let target = lifecycle.clone();
        let calls = Rc::new(Cell::new(0));
        let count = calls.clone();
        let token = value.value().subscribe_with_lifecycle(
            &lifecycle,
            ObserveLifecycleMode::StartStop,
            move |_| {
                count.set(count.get() + 1);
                if count.get() == 1 {
                    if destroy {
                        target.destroy().unwrap();
                    } else {
                        target.move_to(LifecycleState::Created).unwrap();
                    }
                }
            },
        );
        value.set(1);
        assert_eq!(calls.get(), 1);
        if !destroy {
            lifecycle.start().unwrap();
            assert_eq!(calls.get(), 2);
        }
        token.cancel();
        assert_eq!(lifecycle.subscriber_count(), 0);
    }
}

#[test]
fn lifecycle_setup_panic_rolls_back_both_registrations() {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    let value = MutableValue::new(0);
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    assert!(
        catch_unwind(AssertUnwindSafe(|| value.value().subscribe_with_lifecycle(
            &lifecycle,
            ObserveLifecycleMode::StartStop,
            move |_| {
                let _ = &capture;
                panic!("replay");
            }
        )))
        .is_err()
    );
    assert_eq!(lifecycle.subscriber_count(), 0);
    assert!(weak.upgrade().is_none());
    value.set(1);
}

#[test]
fn lifecycle_observer_can_cancel_itself_and_panics_remain_cancellable() {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Started).unwrap();
    let value = MutableValue::new(0);
    let slot = Rc::new(RefCell::new(None::<Subscription>));
    let cancel = slot.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = seen.clone();
    *slot.borrow_mut() = Some(value.value().subscribe_with_lifecycle(
        &lifecycle,
        ObserveLifecycleMode::StartStop,
        move |x| {
            out.borrow_mut().push(*x);
            if *x == 1 {
                cancel.borrow().as_ref().unwrap().cancel();
            }
        },
    ));
    value.set(1);
    value.set(2);
    assert_eq!(*seen.borrow(), [0, 1]);
    assert_eq!(lifecycle.subscriber_count(), 0);
    let token =
        value
            .value()
            .subscribe_with_lifecycle(&lifecycle, ObserveLifecycleMode::StartStop, |x| {
                if *x == 3 {
                    panic!("ordinary observer");
                }
            });
    assert!(catch_unwind(AssertUnwindSafe(|| value.set(3))).is_err());
    token.cancel();
    assert_eq!(lifecycle.subscriber_count(), 0);
    slot.borrow_mut().take();
}

struct OnDrop(Option<Box<dyn FnOnce()>>);
impl Drop for OnDrop {
    fn drop(&mut self) {
        if let Some(callback) = self.0.take() {
            callback();
        }
    }
}

#[test]
fn replaced_mapped_payload_drops_without_a_cache_borrow() {
    let value = MutableValue::new(0);
    let dropped = Rc::new(Cell::new(0));
    let output = dropped.clone();
    let writer = value.clone();
    let mapped = value.value().map(move |input| {
        let output = output.clone();
        let writer = writer.clone();
        let initial = *input == 0;
        OnDrop(Some(Box::new(move || {
            output.set(output.get() + 1);
            if initial {
                writer.update_rc(|old| Rc::new(*old + 1));
            }
        })))
    });
    value.set(1);
    drop(mapped.get());
    assert_eq!(dropped.get(), 1);
    assert_eq!(*value.get(), 2);
    drop(mapped.get());
    assert_eq!(dropped.get(), 2);
}

#[test]
fn rejected_cas_and_discarded_update_candidates_drop_outside_source_borrow() {
    let count = Rc::new(Cell::new(0));
    let value = MutableValue::new(OnDrop(None));
    let writer = value.clone();
    let output = count.clone();
    let rejected = Rc::new(OnDrop(Some(Box::new(move || {
        writer.set(OnDrop(None));
        output.set(output.get() + 1);
    }))));
    assert!(
        value
            .compare_exchange(&Rc::new(OnDrop(None)), rejected)
            .is_err()
    );
    assert_eq!(count.get(), 1);
    let mut attempts = 0;
    value.update_rc(|_| {
        attempts += 1;
        if attempts == 1 {
            value.set(OnDrop(None));
            let writer = value.clone();
            let output = count.clone();
            Rc::new(OnDrop(Some(Box::new(move || {
                writer.set(OnDrop(None));
                output.set(output.get() + 1);
            }))))
        } else {
            Rc::new(OnDrop(None))
        }
    });
    assert_eq!(attempts, 2);
    assert_eq!(count.get(), 2);
}

#[test]
fn mapped_reentrant_observation_and_updates_deliver_newer_cache() {
    let value = MutableValue::new(0);
    let slot = Rc::new(RefCell::new(None::<Value<i32>>));
    let reader = slot.clone();
    let writer = value.clone();
    let nested = Rc::new(RefCell::new(None::<Subscription>));
    let token_slot = nested.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = seen.clone();
    let mapped = value.value().map(move |input| {
        if *input == 1 {
            writer.update_rc(|_| Rc::new(2));
            let mapped = reader.borrow().as_ref().unwrap().clone();
            assert_eq!(*mapped.get(), 20);
            let out = out.clone();
            *token_slot.borrow_mut() = Some(mapped.subscribe(move |x| out.borrow_mut().push(*x)));
        }
        input * 10
    });
    *slot.borrow_mut() = Some(mapped.clone());
    let initial = mapped.subscribe(|_| {});
    value.set(1);
    assert!(
        seen.borrow().iter().all(|x| *x == 20),
        "{:?}",
        *seen.borrow()
    );
    assert_eq!(*mapped.get(), 20);
    drop(initial);
    nested.borrow_mut().take();
    slot.borrow_mut().take();
}

#[test]
fn mapped_downstream_replay_panic_disconnects_upstream() {
    let value = MutableValue::new(0);
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let mapped = value.value().map(move |input| {
        count.set(count.get() + 1);
        *input
    });
    assert!(catch_unwind(AssertUnwindSafe(|| mapped.subscribe(|_| panic!("replay")))).is_err());
    value.set(1);
    assert_eq!(calls.get(), 1);
    assert_eq!(*mapped.get(), 1);
    assert_eq!(calls.get(), 2);
}

#[test]
fn mapped_upstream_connect_panic_rolls_back_and_can_retry() {
    let value = MutableValue::new(0);
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let fail = Rc::new(Cell::new(true));
    let flag = fail.clone();
    let mapped = value.value().map(move |input| {
        count.set(count.get() + 1);
        if *input == 1 && flag.replace(false) {
            panic!("connect mapper");
        }
        *input
    });
    value.set(1);
    assert!(catch_unwind(AssertUnwindSafe(|| mapped.subscribe(|_| {}))).is_err());
    value.set(2);
    assert_eq!(calls.get(), 2);
    let token = mapped.subscribe(|_| {});
    value.set(3);
    assert_eq!(*mapped.get(), 3);
    token.cancel();
}

#[test]
fn lifecycle_capture_drop_can_reenter_source_and_registry_without_destroy() {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Started).unwrap();
    let value = MutableValue::new(0);
    let writer = value.clone();
    let registry = lifecycle.clone();
    let probe = OnDrop(Some(Box::new(move || {
        writer.set(9);
        assert_eq!(registry.subscriber_count(), 0);
        registry.stop().unwrap();
    })));
    let token = value.value().subscribe_with_lifecycle(
        &lifecycle,
        ObserveLifecycleMode::StartStop,
        move |_| {
            let _ = &probe;
        },
    );
    drop(token);
    assert_eq!(*value.get(), 9);
    assert_eq!(lifecycle.state(), LifecycleState::Created);
}

#[test]
fn lifecycle_raii_token_drop_releases_all_captures_after_external_handles_drop() {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Started).unwrap();
    let value = MutableValue::new(0);
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let token = value.value().subscribe_with_lifecycle(
        &lifecycle,
        ObserveLifecycleMode::StartStop,
        move |_| {
            let _ = &capture;
        },
    );
    drop(lifecycle);
    drop(value);
    assert!(weak.upgrade().is_some());
    drop(token);
    assert!(weak.upgrade().is_none());
}

#[test]
fn already_destroyed_lifecycle_drops_callback_immediately() {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.destroy().unwrap();
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let value = MutableValue::new(0);
    let token = value.value().subscribe_with_lifecycle(
        &lifecycle,
        ObserveLifecycleMode::CreateDestroy,
        move |_| {
            let _ = &capture;
            panic!("must remain inactive");
        },
    );
    assert!(weak.upgrade().is_none());
    assert_eq!(lifecycle.subscriber_count(), 0);
    token.cancel();
}

#[test]
fn lifecycle_stale_transition_callback_uses_actual_state() {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.create().unwrap();
    let once = Rc::new(Cell::new(true));
    let flag = once.clone();
    let target = lifecycle.clone();
    let _first = lifecycle.subscribe(move |state| {
        if state == LifecycleState::Started && flag.replace(false) {
            target.stop().unwrap();
        }
    });
    let value = MutableValue::new(0);
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let token = value.value().subscribe_with_lifecycle(
        &lifecycle,
        ObserveLifecycleMode::StartStop,
        move |_| count.set(count.get() + 1),
    );
    lifecycle.start().unwrap();
    assert_eq!(lifecycle.state(), LifecycleState::Created);
    value.set(1);
    assert_eq!(calls.get(), 0);
    lifecycle.start().unwrap();
    assert_eq!(calls.get(), 1);
    token.cancel();
}

#[test]
fn lifecycle_activation_replay_panic_can_retry_same_open_gate() {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.create().unwrap();
    let value = MutableValue::new(0);
    let fail = Rc::new(Cell::new(true));
    let flag = fail.clone();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let token = value.value().subscribe_with_lifecycle(
        &lifecycle,
        ObserveLifecycleMode::StartStop,
        move |_| {
            count.set(count.get() + 1);
            if flag.replace(false) {
                panic!("activation");
            }
        },
    );
    assert!(catch_unwind(AssertUnwindSafe(|| lifecycle.start().unwrap())).is_err());
    lifecycle.resume().unwrap();
    assert_eq!(calls.get(), 2);
    value.set(1);
    assert_eq!(calls.get(), 3);
    token.cancel();
}

#[test]
fn mapper_can_subscribe_after_installing_a_newer_upstream_snapshot() {
    let value = MutableValue::new(0);
    let slot = Rc::new(RefCell::new(None::<Value<i32>>));
    let reader = slot.clone();
    let writer = value.clone();
    let nested = Rc::new(RefCell::new(None::<Subscription>));
    let token_slot = nested.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    let mapped = value.value().map(move |input| {
        if *input == 1 {
            writer.set(2);
            let output = output.clone();
            let mapped = reader.borrow().as_ref().unwrap().clone();
            *token_slot.borrow_mut() =
                Some(mapped.subscribe(move |x| output.borrow_mut().push(*x)));
        }
        input * 10
    });
    *slot.borrow_mut() = Some(mapped.clone());
    let token = mapped.subscribe(|_| {});
    value.set(1);
    assert!(
        seen.borrow().iter().all(|x| *x == 20),
        "{:?}",
        *seen.borrow()
    );
    drop(token);
    nested.borrow_mut().take();
    slot.borrow_mut().take();
}

#[test]
fn lifecycle_replay_close_then_reopen_does_not_resume_old_registration() {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Started).unwrap();
    let value = MutableValue::new(0);
    let writer = value.clone();
    let target = lifecycle.clone();
    let once = Rc::new(Cell::new(true));
    let flag = once.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    let token = value.value().subscribe_with_lifecycle(
        &lifecycle,
        ObserveLifecycleMode::StartStop,
        move |x| {
            output.borrow_mut().push(*x);
            if flag.replace(false) {
                target.stop().unwrap();
                writer.set(1);
                target.start().unwrap();
            }
        },
    );
    assert_eq!(*seen.borrow(), [0, 1]);
    value.set(2);
    assert_eq!(*seen.borrow(), [0, 1, 2]);
    token.cancel();
}

#[test]
fn captured_upstream_delivery_cannot_replace_a_newer_mapped_replay() {
    for advance_again in [false, true] {
        let source = MutableValue::new(0);
        let mapped_slot = Rc::new(RefCell::new(None::<Value<i32>>));
        let reader = mapped_slot.clone();
        let writer = source.clone();
        let late_token = Rc::new(RefCell::new(None::<Subscription>));
        let saved = late_token.clone();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        let _first = source.value().subscribe(move |snapshot| {
            if *snapshot == 1 {
                writer.set(2);
                let mapped = reader.borrow().as_ref().unwrap().clone();
                assert_eq!(*mapped.get(), 20);
                let output = output.clone();
                *saved.borrow_mut() =
                    Some(mapped.subscribe(move |snapshot| output.borrow_mut().push(*snapshot)));
                if advance_again {
                    writer.set(3);
                }
            }
        });
        let mapped = source.value().map(|snapshot| snapshot * 10);
        *mapped_slot.borrow_mut() = Some(mapped.clone());
        let token = mapped.subscribe(|_| {});
        source.set(1);
        assert!(
            seen.borrow().iter().all(|snapshot| *snapshot >= 20),
            "{:?}",
            *seen.borrow()
        );
        assert_eq!(*mapped.get(), if advance_again { 30 } else { 20 });
        let current = mapped.get();
        assert!(Rc::ptr_eq(&current, &mapped.get()));
        source.set_rc(source.get());
        assert!(Rc::ptr_eq(&current, &mapped.get()));
        drop(token);
        late_token.borrow_mut().take();
        mapped_slot.borrow_mut().take();
    }
}

#[test]
fn mapper_receives_the_snapshot_that_started_its_reentrant_calculation() {
    let source = MutableValue::new(0);
    let writer = source.clone();
    let mapped_inputs = Rc::new(RefCell::new(Vec::new()));
    let inputs = mapped_inputs.clone();
    let mapped = source.value().map(move |input| {
        inputs.borrow_mut().push(*input);
        if *input == 1 {
            writer.set(2);
        }
        input * 10
    });
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    let _token = mapped.subscribe(move |snapshot| output.borrow_mut().push(*snapshot));
    source.set(1);
    assert_eq!(*mapped_inputs.borrow(), [0, 1, 2]);
    assert_eq!(*seen.borrow(), [0, 10, 20]);
    source.set(3);
    source.set_rc(source.get());
    assert_eq!(*mapped_inputs.borrow(), [0, 1, 2, 3]);
    assert_eq!(*seen.borrow(), [0, 10, 20, 30, 30]);
}

#[test]
fn mapped_cancellation_panic_still_disconnects_idle_upstream() {
    let source = MutableValue::new(0);
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let mapped = source.value().map(move |input| {
        count.set(count.get() + 1);
        *input
    });
    let capture = OnDrop(Some(Box::new(|| panic!("capture drop panic"))));
    let token = mapped.subscribe(move |_| {
        let _ = &capture;
    });
    let panic = catch_unwind(AssertUnwindSafe(|| token.cancel())).unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"capture drop panic"));
    assert_eq!(calls.get(), 1);
    source.set(1);
    assert_eq!(calls.get(), 1);
    assert_eq!(*mapped.get(), 1);
    assert_eq!(calls.get(), 2);
    token.cancel();
}

#[test]
fn chained_get_preserves_cache_installed_during_upstream_get() {
    struct Payload(i32);
    for depth in 0..3 {
        let source = MutableValue::new(Payload(0));
        let last_slot = Rc::new(RefCell::new(None::<Value<Payload>>));
        let reader = last_slot.clone();
        let writer = source.clone();
        let nested = Rc::new(RefCell::new(None::<Rc<Payload>>));
        let result = nested.clone();
        let mut chain = source.value().map(move |input| {
            if input.0 == 1 {
                writer.set(Payload(2));
                let last = reader.borrow().as_ref().unwrap().clone();
                *result.borrow_mut() = Some(last.get());
            }
            Payload(input.0 * 10)
        });
        for _ in 0..depth {
            chain = chain.map(|input| Payload(input.0));
        }
        let inputs = Rc::new(RefCell::new(Vec::new()));
        let mapped_inputs = inputs.clone();
        let last = chain.map(move |input| {
            mapped_inputs.borrow_mut().push(input.0);
            Payload(input.0)
        });
        *last_slot.borrow_mut() = Some(last.clone());
        source.set(Payload(1));
        let old = last.get();
        assert_eq!(old.0, 10);
        let current = last.get();
        assert_eq!(current.0, 20);
        assert!(Rc::ptr_eq(&current, nested.borrow().as_ref().unwrap()));
        assert_eq!(*inputs.borrow(), [0, 20, 10]);
        assert!(Rc::ptr_eq(&current, &last.get()));
        last_slot.borrow_mut().take();
    }
}

#[test]
fn chained_upstream_get_reentry_cannot_publish_older_than_nested_replay() {
    for depth in 0..3 {
        let source = MutableValue::new(0);
        let last_slot = Rc::new(RefCell::new(None::<Value<i32>>));
        let reader = last_slot.clone();
        let writer = source.clone();
        let nested_token = Rc::new(RefCell::new(None::<Subscription>));
        let saved = nested_token.clone();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        let mut chain = source.value().map(move |input| {
            if *input == 1 {
                writer.set(2);
                let last = reader.borrow().as_ref().unwrap().clone();
                let output = output.clone();
                *saved.borrow_mut() =
                    Some(last.subscribe(move |snapshot| output.borrow_mut().push(snapshot)));
            }
            input * 10
        });
        for _ in 0..depth {
            chain = chain.map(|input| *input);
        }
        let last = chain.map(|input| *input);
        *last_slot.borrow_mut() = Some(last.clone());
        source.set(1);
        let old = last.get();
        assert_eq!(*old, 10);
        let current = last.get();
        assert_eq!(*current, 20);
        assert!(seen.borrow().iter().all(|snapshot| **snapshot == 20));
        assert!(
            seen.borrow()
                .iter()
                .all(|snapshot| Rc::ptr_eq(snapshot, &current))
        );
        let before = seen.borrow().len();
        source.set_rc(source.get());
        assert!(seen.borrow().len() > before);
        assert!(Rc::ptr_eq(&current, &last.get()));
        nested_token.borrow_mut().take();
        last_slot.borrow_mut().take();
    }
}
