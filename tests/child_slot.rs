use decompose_rs::router::slot::*;
use decompose_rs::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn root() -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::new(lifecycle)
}

#[test]
fn empty_activate_equal_replace_and_dismiss_preserve_scope_contract() {
    let root = root();
    let navigation = SlotNavigation::new();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let slot = child_slot(
        &root,
        &navigation,
        None,
        SlotOptions::default(),
        move |_: &u32, ctx| {
            count.set(count.get() + 1);
            ctx
        },
    )
    .unwrap();
    assert!(slot.get().child.is_none());
    assert_eq!(calls.get(), 0);
    navigation.activate(1);
    let first = slot.get().child.as_ref().unwrap().clone();
    assert_eq!(first.instance.lifecycle().state(), LifecycleState::Resumed);
    navigation.activate(1);
    assert!(Rc::ptr_eq(
        &first.instance,
        &slot.get().child.as_ref().unwrap().instance
    ));
    assert_eq!(calls.get(), 1);
    navigation.activate(2);
    assert_eq!(
        first.instance.lifecycle().state(),
        LifecycleState::Destroyed
    );
    assert!(first.instance.instance_keeper().borrow().is_destroyed());
    let second = slot.get().child.as_ref().unwrap().clone();
    assert_ne!(first.key, second.key);
    navigation.dismiss();
    assert!(slot.get().child.is_none());
    assert_eq!(
        second.instance.lifecycle().state(),
        LifecycleState::Destroyed
    );
    navigation.dismiss();
    assert_eq!(calls.get(), 2);
}

#[test]
fn completion_reports_empty_noop_and_per_connected_slot_results() {
    let parent = root();
    let nav = SlotNavigation::<u32>::default();
    let empty = child_slot(&parent, &nav, None, SlotOptions::default(), |_, ctx| ctx).unwrap();
    let full = child_slot(
        &parent,
        &nav,
        Some(1),
        SlotOptions {
            key: "other".into(),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let results = Rc::new(RefCell::new(Vec::new()));
    let output = results.clone();
    nav.dismiss_with(move |r| output.borrow_mut().push(r.unwrap()));
    assert_eq!(*results.borrow(), [false, true]);
    assert!(empty.get().child.is_none());
    assert!(full.get().child.is_none());
    let output = results.clone();
    nav.navigate_with(
        |c| c,
        move |r| {
            let r = r.unwrap();
            assert!(r.old_configuration.is_none());
            assert!(r.new_configuration.is_none());
            output.borrow_mut().push(false);
        },
    );
    assert_eq!(results.borrow().len(), 4);
}

fn codec() -> SlotCodec<u32> {
    SlotCodec::new(
        |c: &u32| Ok(c.to_le_bytes().to_vec()),
        |b| {
            b.try_into()
                .map(u32::from_le_bytes)
                .map_err(|_| "wrong length".into())
        },
    )
}
fn persistent_options() -> SlotOptions<u32> {
    SlotOptions {
        codec: Some(codec()),
        ..Default::default()
    }
}
fn restored_root(
    saved: std::collections::BTreeMap<String, Vec<u8>>,
    instances: Option<SharedInstances>,
) -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    let state = Rc::new(RefCell::new(StateKeeper::with_restored(saved)));
    if let Some(instances) = instances {
        DefaultComponentContext::with_all(lifecycle, state, instances, SharedBack::new())
    } else {
        DefaultComponentContext::with_state_keeper(lifecycle, state)
    }
}

#[test]
fn populated_persistence_restores_configuration_key_nested_state_and_retention() {
    let instances = Rc::new(RefCell::new(InstanceKeeper::new()));
    let parent = restored_root(Default::default(), Some(instances.clone()));
    let nav = SlotNavigation::new();
    let slot = child_slot(&parent, &nav, Some(10), persistent_options(), |_, ctx| ctx).unwrap();
    let original = slot.get().child.as_ref().unwrap().clone();
    original
        .instance
        .state_keeper()
        .borrow_mut()
        .register("nested", || vec![42])
        .unwrap();
    let service = original
        .instance
        .instance_keeper()
        .borrow_mut()
        .get_or_create("service", || 99u32)
        .unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    assert_eq!(&saved["DefaultChildSlot"][..4], b"DSL1");
    parent.lifecycle().destroy().unwrap();
    assert!(!original.instance.instance_keeper().borrow().is_destroyed());
    let recreated = restored_root(saved.clone(), Some(instances));
    let slot = child_slot(
        &recreated,
        &nav,
        Some(999),
        persistent_options(),
        |_, ctx| ctx,
    )
    .unwrap();
    let restored = slot.get().child.as_ref().unwrap().clone();
    assert_eq!(*restored.configuration, 10);
    assert_eq!(restored.key, original.key);
    assert_eq!(
        restored
            .instance
            .state_keeper()
            .borrow_mut()
            .consume_bytes("nested"),
        Some(vec![42])
    );
    let reused = restored
        .instance
        .instance_keeper()
        .borrow_mut()
        .get_or_create("service", || 0u32)
        .unwrap();
    assert!(Rc::ptr_eq(&service, &reused));
    let process = restored_root(saved, None);
    let fresh = child_slot(
        &process,
        &SlotNavigation::new(),
        None,
        persistent_options(),
        |_, ctx| ctx,
    )
    .unwrap();
    let child = fresh.get().child.as_ref().unwrap().clone();
    assert_eq!(*child.configuration, 10);
    let new = child
        .instance
        .instance_keeper()
        .borrow_mut()
        .get_or_create("service", || 0u32)
        .unwrap();
    assert!(!Rc::ptr_eq(&service, &new));
}

#[test]
fn persisted_empty_overrides_initial_and_never_invokes_codec() {
    let parent = root();
    let nav = SlotNavigation::new();
    let options = SlotOptions {
        codec: Some(SlotCodec::new(
            |_: &u32| panic!("encode empty"),
            |_| panic!("decode empty"),
        )),
        ..Default::default()
    };
    let slot = child_slot(&parent, &nav, None, options, |_, ctx| ctx).unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    assert_eq!(saved["DefaultChildSlot"].len(), 16);
    let recreated = restored_root(saved, None);
    let options = SlotOptions {
        codec: Some(SlotCodec::new(
            |_: &u32| panic!("encode empty"),
            |_| panic!("decode empty"),
        )),
        ..Default::default()
    };
    let restored = child_slot(&recreated, &nav, Some(1), options, |_, _| {
        panic!("factory empty")
    })
    .unwrap();
    assert!(slot.get().child.is_none());
    assert!(restored.get().child.is_none());
}

#[test]
fn saving_dismissal_overwrites_populated_membership_and_preserves_counter() {
    let parent = root();
    let nav = SlotNavigation::new();
    let slot = child_slot(&parent, &nav, Some(1), persistent_options(), |_, ctx| ctx).unwrap();
    let old_key = slot.get().child.as_ref().unwrap().key.clone();
    assert!(parent.state_keeper().borrow().save().unwrap()["DefaultChildSlot"].len() > 16);
    nav.dismiss();
    let saved = parent.state_keeper().borrow().save().unwrap();
    let parent = restored_root(saved, None);
    let slot = child_slot(&parent, &nav, Some(2), persistent_options(), |_, ctx| ctx).unwrap();
    assert!(slot.get().child.is_none());
    nav.activate(3);
    assert_ne!(old_key, slot.get().child.as_ref().unwrap().key);
}

#[test]
fn destroying_parent_in_initial_factory_aborts_construction() {
    let root = root();
    let lifecycle = root.lifecycle();
    let result = child_slot(
        &root,
        &SlotNavigation::new(),
        Some(1u32),
        SlotOptions::default(),
        move |_, ctx| {
            lifecycle.destroy().unwrap();
            ctx
        },
    );
    assert!(matches!(result, Err(NavigationError::Destroyed)));
    assert_eq!(root.back_handler().handler_count(), 0);
    assert!(
        !root
            .state_keeper()
            .borrow()
            .is_registered("DefaultChildSlot")
    );
}

#[test]
fn initial_resume_destroying_parent_is_a_construction_error() {
    let root = root();
    let lifecycle = root.lifecycle();
    let result = child_slot(
        &root,
        &SlotNavigation::new(),
        Some(1u32),
        SlotOptions::default(),
        move |_, ctx| {
            let lifecycle = lifecycle.clone();
            let _ = ctx
                .lifecycle()
                .do_on_resume(move || lifecycle.destroy().unwrap())
                .detach();
            ctx
        },
    );
    assert!(matches!(result, Err(NavigationError::Destroyed)));
    assert!(
        !root
            .state_keeper()
            .borrow()
            .is_registered("DefaultChildSlot")
    );
    assert_eq!(root.back_handler().handler_count(), 0);
}

#[test]
fn panicking_navigation_queued_by_initial_factory_rolls_back_constructor() {
    let root = root();
    let nav = SlotNavigation::new();
    let source = nav.clone();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        child_slot(
            &root,
            &nav,
            Some(1u32),
            SlotOptions::default(),
            move |config, ctx| {
                if *config == 1 {
                    source.activate(2);
                } else {
                    panic!("queued factory failure");
                }
                ctx
            },
        )
    }));
    assert!(outcome.is_err());
    assert!(
        !root
            .state_keeper()
            .borrow()
            .is_registered("DefaultChildSlot")
    );
    assert!(root.instance_keeper().borrow().is_empty());
    assert_eq!(root.back_handler().handler_count(), 0);
}

#[test]
fn dropping_views_keeps_navigation_alive_until_parent_destroy_without_a_cycle() {
    let root = root();
    let nav = SlotNavigation::new();
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let stack = child_slot(
        &root,
        &nav,
        Some(1u32),
        SlotOptions::default(),
        move |_, ctx| {
            let _ = &capture;
            ctx
        },
    )
    .unwrap();
    drop(stack);
    nav.activate(2);
    assert!(weak.upgrade().is_some());
    root.lifecycle().destroy().unwrap();
    assert!(weak.upgrade().is_none());
    assert_eq!(root.back_handler().handler_count(), 0);
}

#[test]
fn saves_use_one_membership_snapshot_even_when_a_provider_navigates() {
    let root = root();
    let nav = SlotNavigation::new();
    let source = nav.clone();
    let once = Rc::new(std::cell::Cell::new(true));
    let options = SlotOptions {
        codec: Some(codec()),
        ..SlotOptions::default()
    };
    let stack = child_slot(&root, &nav, Some(1u32), options, move |config, ctx| {
        if *config == 1 {
            let nav = source.clone();
            let once = once.clone();
            ctx.state_keeper()
                .borrow_mut()
                .register("state", move || {
                    if once.replace(false) {
                        nav.activate(2);
                    }
                    vec![11]
                })
                .unwrap();
        }
        ctx
    })
    .unwrap();
    let original_key = stack.get().child.as_ref().unwrap().key.clone();
    let saved = root.state_keeper().borrow().save().unwrap();
    assert_eq!(*stack.get().child.as_ref().unwrap().configuration, 2);
    let restored = DefaultComponentContext::with_state_keeper(
        self::root().lifecycle(),
        Rc::new(RefCell::new(StateKeeper::with_restored(saved))),
    );
    let options = SlotOptions {
        codec: Some(codec()),
        ..SlotOptions::default()
    };
    let stack = child_slot(
        &restored,
        &SlotNavigation::new(),
        Some(99),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(*stack.get().child.as_ref().unwrap().configuration, 1);
    assert_eq!(stack.get().child.as_ref().unwrap().key, original_key);
    assert_eq!(
        stack
            .get()
            .child
            .as_ref()
            .unwrap()
            .instance
            .state_keeper()
            .borrow_mut()
            .consume_bytes("state"),
        Some(vec![11])
    );
}

#[test]
fn queued_initial_event_destroying_parent_returns_a_construction_error() {
    let root = root();
    let nav = SlotNavigation::new();
    let source = nav.clone();
    let parent = root.lifecycle();
    let result = child_slot(
        &root,
        &nav,
        Some(1u32),
        SlotOptions::default(),
        move |_, ctx| {
            let parent = parent.clone();
            source.navigate_with(
                move |old| {
                    parent.destroy().unwrap();
                    old
                },
                |_| {},
            );
            ctx
        },
    );
    assert!(matches!(result, Err(NavigationError::Destroyed)));
    assert_eq!(root.back_handler().handler_count(), 0);
}

#[test]
fn parent_destroyed_while_old_active_pauses_releases_uncommitted_children() {
    let root = DefaultComponentContext::with_instance_keeper(
        root().lifecycle(),
        Rc::new(RefCell::new(InstanceKeeper::new())),
    );
    let nav = SlotNavigation::new();
    let parent = root.lifecycle();
    let created = Rc::new(RefCell::new(None::<SharedInstances>));
    let peer = created.clone();
    let stack = child_slot(
        &root,
        &nav,
        Some(1u32),
        SlotOptions::default(),
        move |config, ctx| {
            if *config == 1 {
                let parent = parent.clone();
                let _ = ctx
                    .lifecycle()
                    .do_on_pause(move || parent.destroy().unwrap())
                    .detach();
            } else {
                *peer.borrow_mut() = Some(ctx.instance_keeper());
            }
            ctx
        },
    )
    .unwrap();
    let outcome = Rc::new(RefCell::new(None));
    let result = outcome.clone();
    nav.navigate_with(
        |_| Some(Rc::new(2)),
        move |value| *result.borrow_mut() = Some(value),
    );
    assert!(matches!(
        &*outcome.borrow(),
        Some(Err(NavigationError::Destroyed))
    ));
    assert!(created.borrow().as_ref().unwrap().borrow().is_destroyed());
    assert_eq!(*stack.get().child.as_ref().unwrap().configuration, 1);
    assert_eq!(root.back_handler().handler_count(), 0);
}

#[test]
fn custom_nonclone_context_and_configuration_work_through_three_slots() {
    struct Custom {
        base: DefaultComponentContext,
        service: Rc<u32>,
    }
    impl ComponentContext for Custom {
        fn lifecycle(&self) -> LifecycleRegistry {
            self.base.lifecycle()
        }
        fn state_keeper(&self) -> SharedKeeper {
            self.base.state_keeper()
        }
        fn instance_keeper(&self) -> SharedInstances {
            self.base.instance_keeper()
        }
        fn back_handler(&self) -> SharedBack {
            self.base.back_handler()
        }
        fn context_factory(&self) -> ContextFactory<Self> {
            let service = self.service.clone();
            Rc::new(move |parts| Self {
                base: DefaultComponentContext::from_parts(parts),
                service: service.clone(),
            })
        }
    }
    #[derive(PartialEq, Eq)]
    struct Config(u32);
    let service = Rc::new(7);
    let parent = Custom {
        base: root(),
        service: service.clone(),
    };
    let one = child_slot(
        &parent,
        &SlotNavigation::new(),
        Some(Config(1)),
        SlotOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let two = child_slot(
        one.get().child.as_ref().unwrap().instance.as_ref(),
        &SlotNavigation::new(),
        Some(Config(2)),
        SlotOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let three = child_slot(
        two.get().child.as_ref().unwrap().instance.as_ref(),
        &SlotNavigation::new(),
        Some(Config(3)),
        SlotOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(Rc::ptr_eq(
        &service,
        &three.get().child.as_ref().unwrap().instance.service
    ));
    parent.lifecycle().destroy().unwrap();
    assert!(
        three
            .get()
            .child
            .as_ref()
            .unwrap()
            .instance
            .lifecycle()
            .state()
            .is_destroyed()
    );
}

#[test]
fn slot_key_is_reserved_before_a_factory_can_create_another_stack() {
    let root = root();
    let nested = root.clone();
    let rejected = Rc::new(std::cell::Cell::new(false));
    let peer = rejected.clone();
    let result = child_slot(
        &root,
        &SlotNavigation::new(),
        Some(1u32),
        SlotOptions::default(),
        move |_, ctx| {
            let inner = child_slot(
                &nested,
                &SlotNavigation::new(),
                Some(9u32),
                SlotOptions::default(),
                |_, ctx| ctx,
            );
            peer.set(matches!(
                inner,
                Err(NavigationError::Context(
                    DecomposeError::DuplicateKey { .. }
                ))
            ));
            ctx
        },
    );
    assert!(result.is_ok());
    assert!(rejected.get());
}

#[test]
fn no_op_navigation_does_not_stop_and_resume_the_active_component() {
    let root = root();
    let nav = SlotNavigation::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    let peer = log.clone();
    let stack = child_slot(
        &root,
        &nav,
        Some(1u32),
        SlotOptions::default(),
        move |_, ctx| {
            let log = peer.clone();
            let _ = ctx
                .lifecycle()
                .subscribe(move |state| log.borrow_mut().push(state))
                .detach();
            ctx
        },
    )
    .unwrap();
    log.borrow_mut().clear();
    nav.activate(1);
    nav.navigate(|old| old);
    assert!(log.borrow().is_empty());
    assert_eq!(
        stack
            .get()
            .child
            .as_ref()
            .unwrap()
            .instance
            .lifecycle()
            .state(),
        LifecycleState::Resumed
    );
}

#[test]
fn parent_lifecycle_bounds_populated_and_later_activated_children() {
    let parent = root();
    let nav = SlotNavigation::new();
    let slot = child_slot(
        &parent,
        &nav,
        None,
        SlotOptions::default(),
        |_: &u32, ctx| ctx,
    )
    .unwrap();
    parent.lifecycle().move_to(LifecycleState::Created).unwrap();
    nav.activate(1);
    let child = slot.get().child.as_ref().unwrap().clone();
    assert_eq!(child.instance.lifecycle().state(), LifecycleState::Created);
    parent.lifecycle().move_to(LifecycleState::Started).unwrap();
    assert_eq!(child.instance.lifecycle().state(), LifecycleState::Started);
    parent.lifecycle().move_to(LifecycleState::Resumed).unwrap();
    assert_eq!(child.instance.lifecycle().state(), LifecycleState::Resumed);
    parent.lifecycle().destroy().unwrap();
    assert_eq!(
        child.instance.lifecycle().state(),
        LifecycleState::Destroyed
    );
    assert_eq!(parent.back_handler().handler_count(), 0);
    nav.activate(2);
    assert!(Rc::ptr_eq(
        &child.instance,
        &slot.get().child.as_ref().unwrap().instance
    ));
}

#[test]
fn all_reentrant_origins_finish_current_publication_before_fifo_commands() {
    let parent = root();
    let nav = SlotNavigation::new();
    let source = nav.clone();
    let log = Rc::new(RefCell::new(Vec::new()));
    let events = log.clone();
    let slot = child_slot(
        &parent,
        &nav,
        Some(0),
        SlotOptions::default(),
        move |config: &u32, ctx| {
            if *config == 1 {
                source.activate(2);
                let nav = source.clone();
                let _ = ctx
                    .lifecycle()
                    .do_on_resume(move || nav.activate(3))
                    .detach();
            }
            if *config == 0 {
                let nav = source.clone();
                let _ = ctx
                    .lifecycle()
                    .do_on_pause(move || nav.activate(4))
                    .detach();
            }
            ctx
        },
    )
    .unwrap();
    let source = nav.clone();
    let _observer = slot.subscribe(move |state| {
        let config = state.child.as_ref().map(|c| *c.configuration);
        events.borrow_mut().push(config);
        if config == Some(1) {
            source.activate(5);
        }
    });
    let source = nav.clone();
    nav.activate_with(1, move |result| {
        assert_eq!(*result.unwrap().new_configuration.unwrap(), 1);
        source.activate(6);
    });
    assert_eq!(
        *log.borrow(),
        [
            Some(0),
            Some(1),
            Some(2),
            Some(4),
            Some(3),
            Some(5),
            Some(6)
        ]
    );
    assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 6);
}

#[test]
fn factory_panic_leaves_old_membership_and_releases_prepared_scope() {
    let parent = root();
    let nav = SlotNavigation::new();
    let captured = Rc::new(RefCell::new(None));
    let peer = captured.clone();
    let source = nav.clone();
    let slot = child_slot(
        &parent,
        &nav,
        Some(1),
        SlotOptions::default(),
        move |config: &u32, ctx| {
            if *config == 2 {
                *peer.borrow_mut() = Some((ctx.lifecycle(), ctx.instance_keeper()));
                source.activate(3);
                panic!("factory failed");
            }
            ctx
        },
    )
    .unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| nav.activate(2))).is_err());
    let captured = captured.borrow();
    let (lifecycle, instances) = captured.as_ref().unwrap();
    assert!(lifecycle.state().is_destroyed());
    assert!(instances.borrow().is_destroyed());
    assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 1);
    assert_eq!(
        slot.get()
            .child
            .as_ref()
            .unwrap()
            .instance
            .lifecycle()
            .state(),
        LifecycleState::Resumed
    );
    nav.activate(4);
    assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 4);
}

#[test]
fn observer_and_completion_panics_drop_nested_work_and_source_recovers() {
    for observer_panic in [true, false] {
        let parent = root();
        let nav = SlotNavigation::new();
        let slot =
            child_slot(&parent, &nav, Some(1), SlotOptions::default(), |_, ctx| ctx).unwrap();
        let source = nav.clone();
        let _subscription = slot.subscribe(move |state| {
            if observer_panic && *state.child.as_ref().unwrap().configuration == 2 {
                source.activate(3);
                panic!("observer failed");
            }
        });
        let source = nav.clone();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| nav.activate_with(
                2,
                move |_| {
                    source.activate(4);
                    panic!("completion failed");
                }
            )))
            .is_err()
        );
        assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 2);
        nav.activate(5);
        assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 5);
    }
}

#[test]
fn back_child_priority_predictive_cancel_auto_dismiss_and_empty_fallback() {
    let parent = root();
    let nav = SlotNavigation::new();
    let phases = Rc::new(RefCell::new(Vec::new()));
    let peer = phases.clone();
    let handle = Rc::new(Cell::new(0));
    let registered = handle.clone();
    let fallback = Rc::new(Cell::new(0));
    let output = fallback.clone();
    parent
        .back_handler()
        .register(-1, true, move |event, _| {
            if event.phase == BackPhase::Invoked {
                output.set(output.get() + 1);
            }
        })
        .unwrap();
    let slot = child_slot(
        &parent,
        &nav,
        Some(1),
        SlotOptions {
            handle_back_button: true,
            ..Default::default()
        },
        move |_: &u32, ctx| {
            let phases = peer.clone();
            let callback = ctx
                .back_handler()
                .register(0, true, move |event, _| {
                    phases.borrow_mut().push(event.phase)
                })
                .unwrap();
            registered.set(callback.id());
            ctx
        },
    )
    .unwrap();
    assert!(parent.back_handler().predictive_start());
    parent.back_handler().predictive_progress(0.5).unwrap();
    parent.back_handler().predictive_cancel().unwrap();
    assert_eq!(
        *phases.borrow(),
        [
            BackPhase::Started,
            BackPhase::Progressed,
            BackPhase::Cancelled
        ]
    );
    assert!(slot.get().child.is_some());
    assert!(parent.back_handler().back());
    assert!(slot.get().child.is_some());
    let child = slot.get().child.as_ref().unwrap().clone();
    child
        .instance
        .back_handler()
        .set_enabled(handle.get(), false);
    assert!(parent.back_handler().predictive_start());
    parent.back_handler().predictive_cancel().unwrap();
    assert!(slot.get().child.is_some());
    assert!(parent.back_handler().back());
    assert!(slot.get().child.is_none());
    assert!(parent.back_handler().back());
    assert_eq!(fallback.get(), 1);
}

#[test]
fn back_navigation_waits_for_dispatch_and_panic_discards_deferred_events() {
    let parent = root();
    let nav = SlotNavigation::new();
    let slot = child_slot(&parent, &nav, Some(1), SlotOptions::default(), |_, ctx| ctx).unwrap();
    let source = nav.clone();
    let view = slot.clone();
    let once = Rc::new(Cell::new(true));
    let peer = once.clone();
    parent
        .back_handler()
        .register(2, true, move |_, _| {
            source.activate(2);
            assert_eq!(*view.get().child.as_ref().unwrap().configuration, 1);
            if peer.replace(false) {
                panic!("back failed");
            }
        })
        .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parent
            .back_handler()
            .back()))
        .is_err()
    );
    assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 1);
    assert!(parent.back_handler().back());
    assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 2);
    nav.dismiss();
    assert!(slot.get().child.is_none());
}

#[test]
fn observer_triggered_back_sees_published_configuration_then_dismisses() {
    let parent = root();
    let nav = SlotNavigation::new();
    let slot = child_slot(
        &parent,
        &nav,
        None,
        SlotOptions {
            handle_back_button: true,
            ..Default::default()
        },
        |_: &u32, ctx| ctx,
    )
    .unwrap();
    let back = parent.back_handler();
    let _sub = slot.subscribe(move |state| {
        if state.child.is_some() {
            assert!(back.back());
        }
    });
    nav.activate(1);
    assert!(slot.get().child.is_none());
}

#[test]
fn stack_slot_siblings_coexist_and_conflicts_are_rejected_before_factories() {
    use decompose_rs::router::stack::{StackNavigation, StackOptions, child_stack};
    let parent = root();
    let stack_nav = StackNavigation::new();
    let stack = child_stack(
        &parent,
        &stack_nav,
        vec![1u32],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let slot_nav = SlotNavigation::new();
    let slot = child_slot(
        &parent,
        &slot_nav,
        Some(1u32),
        SlotOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(parent.state_keeper().borrow().provider_count(), 2);
    let same = child_slot(
        &parent,
        &SlotNavigation::new(),
        Some(2u32),
        SlotOptions {
            key: "DefaultChildStack".into(),
            ..Default::default()
        },
        |_, _| panic!("conflicting factory"),
    );
    assert!(matches!(
        same,
        Err(NavigationError::Context(
            DecomposeError::DuplicateKey { .. }
        ))
    ));
    slot_nav.dismiss();
    stack_nav.push(2);
    assert!(slot.get().child.is_none());
    assert_eq!(*stack.get().active.configuration, 2);
}

#[test]
fn transient_recreation_and_empty_restore_discard_old_retained_scopes() {
    for persistent in [false, true] {
        let instances = Rc::new(RefCell::new(InstanceKeeper::new()));
        let parent = restored_root(Default::default(), Some(instances.clone()));
        let nav = SlotNavigation::new();
        let options = if persistent {
            persistent_options()
        } else {
            SlotOptions::default()
        };
        let slot = child_slot(&parent, &nav, None, options, |_, ctx| ctx).unwrap();
        let saved = parent.state_keeper().borrow().save().unwrap();
        nav.activate(1);
        let old = slot
            .get()
            .child
            .as_ref()
            .unwrap()
            .instance
            .instance_keeper();
        parent.lifecycle().destroy().unwrap();
        assert!(!old.borrow().is_destroyed());
        let recreated = restored_root(saved, Some(instances));
        let options = if persistent {
            persistent_options()
        } else {
            SlotOptions::default()
        };
        let slot = child_slot(
            &recreated,
            &SlotNavigation::new(),
            Some(99),
            options,
            |_, ctx| ctx,
        )
        .unwrap();
        assert!(old.borrow().is_destroyed());
        if persistent {
            assert!(slot.get().child.is_none());
        } else {
            assert!(!Rc::ptr_eq(
                &old,
                &slot
                    .get()
                    .child
                    .as_ref()
                    .unwrap()
                    .instance
                    .instance_keeper()
            ));
        }
    }
}

#[test]
fn malformed_frames_reject_before_codec_and_remain_retryable() {
    let parent = root();
    let nav = SlotNavigation::new();
    child_slot(&parent, &nav, Some(1), persistent_options(), |_, ctx| ctx).unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    let original = saved["DefaultChildSlot"].clone();
    let mut malformed = Vec::new();
    for len in 0..original.len() {
        malformed.push(original[..len].to_vec());
    }
    let mut bytes = original.clone();
    bytes.push(0);
    malformed.push(bytes);
    let mut bytes = original.clone();
    bytes[0] = 0;
    malformed.push(bytes);
    let mut bytes = original.clone();
    bytes[12..16].copy_from_slice(&2u32.to_le_bytes());
    malformed.push(bytes);
    let mut bytes = original.clone();
    bytes[4..12].copy_from_slice(&0u64.to_le_bytes());
    malformed.push(bytes);
    let mut bytes = original.clone();
    bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    malformed.push(bytes);
    let key_offset = original.windows(7).position(|w| w == b"child/0").unwrap();
    let mut bytes = original.clone();
    bytes[key_offset] = 255;
    malformed.push(bytes);
    let mut bytes = original.clone();
    bytes[key_offset + 6] = b'1';
    malformed.push(bytes);
    let mut bytes = original.clone();
    let state_offset = bytes.windows(4).position(|w| w == b"DCS1").unwrap();
    bytes[state_offset] = 0;
    malformed.push(bytes);
    for bytes in malformed {
        let saved = std::collections::BTreeMap::from([("DefaultChildSlot".into(), bytes)]);
        let parent = restored_root(saved.clone(), None);
        for _ in 0..2 {
            let options = SlotOptions {
                codec: Some(SlotCodec::new(
                    |_: &u32| Ok(vec![]),
                    |_| panic!("codec must not see malformed framing"),
                )),
                ..Default::default()
            };
            let result = child_slot(&parent, &SlotNavigation::new(), None, options, |_, _| {
                panic!("malformed factory")
            });
            assert!(matches!(result, Err(NavigationError::State(_))));
            assert_eq!(parent.state_keeper().borrow().provider_count(), 0);
            assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
            assert_eq!(parent.back_handler().handler_count(), 0);
        }
    }
}

#[test]
fn codec_failures_and_initial_factory_panic_allow_restoration_retry() {
    let parent = root();
    let nav = SlotNavigation::new();
    child_slot(&parent, &nav, Some(1), persistent_options(), |_, ctx| ctx).unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    let restored = restored_root(saved.clone(), None);
    let options = SlotOptions {
        codec: Some(SlotCodec::new(
            |_: &u32| Ok(vec![]),
            |_| Err("decode failed".into()),
        )),
        ..Default::default()
    };
    assert!(
        matches!(child_slot(&restored,&nav,None,options,|_,ctx|ctx),Err(NavigationError::State(s)) if s=="decode failed")
    );
    assert_eq!(restored.state_keeper().borrow().save().unwrap(), saved);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| child_slot(
            &restored,
            &nav,
            None,
            persistent_options(),
            |_, _| panic!("init failed")
        )))
        .is_err()
    );
    assert_eq!(restored.state_keeper().borrow().save().unwrap(), saved);
    let slot = child_slot(&restored, &nav, None, persistent_options(), |_, ctx| ctx).unwrap();
    assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 1);
    let parent = root();
    let options = SlotOptions {
        codec: Some(SlotCodec::new(
            |_: &u32| Err("encode failed".into()),
            |_| Ok(1),
        )),
        ..Default::default()
    };
    child_slot(
        &parent,
        &SlotNavigation::new(),
        Some(1),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(matches!(
        parent.state_keeper().borrow().save(),
        Err(StateKeeperError::Encode { .. })
    ));
}

#[test]
fn state_key_reserved_before_reentrant_codec_runs() {
    let parent = root();
    child_slot(
        &parent,
        &SlotNavigation::new(),
        Some(1),
        persistent_options(),
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    let parent = restored_root(saved, None);
    let nested = parent.clone();
    let rejected = Rc::new(Cell::new(false));
    let peer = rejected.clone();
    let options = SlotOptions {
        codec: Some(SlotCodec::new(
            |_: &u32| Ok(vec![]),
            move |_| {
                peer.set(matches!(
                    child_slot(
                        &nested,
                        &SlotNavigation::new(),
                        Some(2u32),
                        SlotOptions::default(),
                        |_, _| panic!("nested factory")
                    ),
                    Err(NavigationError::Context(
                        DecomposeError::DuplicateKey { .. }
                    ))
                ));
                Ok(1)
            },
        )),
        ..Default::default()
    };
    child_slot(&parent, &SlotNavigation::new(), None, options, |_, ctx| ctx).unwrap();
    assert!(rejected.get());
}

#[test]
fn exhausted_restored_counter_reports_typed_transition_error_without_factory() {
    let mut bytes = b"DSL1".to_vec();
    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let parent = restored_root(
        std::collections::BTreeMap::from([("DefaultChildSlot".into(), bytes)]),
        None,
    );
    let nav = SlotNavigation::new();
    let slot = child_slot(&parent, &nav, None, persistent_options(), |_, _| {
        panic!("exhausted factory")
    })
    .unwrap();
    let errors = Rc::new(RefCell::new(Vec::new()));
    let peer = errors.clone();
    nav.activate_with(1, move |result| {
        peer.borrow_mut().push(result.err().unwrap())
    });
    assert!(matches!(&errors.borrow()[0], NavigationError::State(_)));
    assert!(slot.get().child.is_none());
}

#[test]
fn transient_slot_save_does_not_invoke_child_state_providers() {
    let parent = root();
    let nav = SlotNavigation::new();
    let slot = child_slot(
        &parent,
        &nav,
        Some(1u32),
        SlotOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    slot.get()
        .child
        .as_ref()
        .unwrap()
        .instance
        .state_keeper()
        .borrow_mut()
        .register("transient", || panic!("transient child save must not run"))
        .unwrap();
    assert!(
        !parent
            .state_keeper()
            .borrow()
            .save()
            .unwrap()
            .contains_key("DefaultChildSlot")
    );
}

#[test]
fn lifecycle_panic_releases_prepared_scope_and_processing_recovers() {
    let parent = root();
    let nav = SlotNavigation::new();
    let source = nav.clone();
    let captured = Rc::new(RefCell::new(None));
    let peer = captured.clone();
    let slot = child_slot(
        &parent,
        &nav,
        Some(1),
        SlotOptions::default(),
        move |config: &u32, ctx| {
            if *config == 2 {
                *peer.borrow_mut() = Some((ctx.lifecycle(), ctx.instance_keeper()));
                let nav = source.clone();
                let _ = ctx
                    .lifecycle()
                    .do_on_resume(move || {
                        nav.activate(3);
                        panic!("resume failed");
                    })
                    .detach();
            }
            ctx
        },
    )
    .unwrap();
    let old = slot.get().child.as_ref().unwrap().clone();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| nav.activate(2))).is_err());
    let captured = captured.borrow();
    let (lifecycle, instances) = captured.as_ref().unwrap();
    assert!(lifecycle.state().is_destroyed());
    assert!(instances.borrow().is_destroyed());
    // Lifecycle callback effects preceding the panic cannot be reversed.
    assert!(old.instance.lifecycle().state().is_destroyed());
    assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 1);
    nav.activate(4);
    assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 4);
    assert_eq!(
        slot.get()
            .child
            .as_ref()
            .unwrap()
            .instance
            .lifecycle()
            .state(),
        LifecycleState::Resumed
    );
}

#[test]
fn restoring_earlier_populated_membership_does_not_reuse_discarded_retention() {
    let instances = Rc::new(RefCell::new(InstanceKeeper::new()));
    let parent = restored_root(Default::default(), Some(instances.clone()));
    let nav = SlotNavigation::new();
    let slot = child_slot(&parent, &nav, Some(1), persistent_options(), |_, ctx| ctx).unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    nav.activate(2);
    let removed = slot
        .get()
        .child
        .as_ref()
        .unwrap()
        .instance
        .instance_keeper();
    parent.lifecycle().destroy().unwrap();
    let parent = restored_root(saved, Some(instances));
    let nav = SlotNavigation::new();
    let slot = child_slot(&parent, &nav, Some(99), persistent_options(), |_, ctx| ctx).unwrap();
    assert!(removed.borrow().is_destroyed());
    assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 1);
    nav.activate(3);
    assert!(!Rc::ptr_eq(
        &removed,
        &slot
            .get()
            .child
            .as_ref()
            .unwrap()
            .instance
            .instance_keeper()
    ));
}

#[test]
fn source_broadcast_finishes_each_slot_before_nested_event() {
    let parent = root();
    let nav = SlotNavigation::new();
    let one = child_slot(
        &parent,
        &nav,
        Some(1u32),
        SlotOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let two = child_slot(
        &parent,
        &nav,
        Some(10u32),
        SlotOptions {
            key: "second".into(),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let source = nav.clone();
    let log = Rc::new(RefCell::new(Vec::new()));
    let peer = log.clone();
    nav.navigate_with(
        |old| old.map(|c| Rc::new(*c + 1)),
        move |r| {
            let r = r.unwrap();
            peer.borrow_mut()
                .push((*r.old_configuration.unwrap(), *r.new_configuration.unwrap()));
            if peer.borrow().len() == 1 {
                let peer = peer.clone();
                source.navigate_with(
                    |old| old.map(|c| Rc::new(*c + 1)),
                    move |r| {
                        let r = r.unwrap();
                        peer.borrow_mut()
                            .push((*r.old_configuration.unwrap(), *r.new_configuration.unwrap()));
                    },
                );
            }
        },
    );
    assert_eq!(*log.borrow(), [(1, 2), (10, 11), (2, 3), (11, 12)]);
    assert_eq!(*one.get().child.as_ref().unwrap().configuration, 3);
    assert_eq!(*two.get().child.as_ref().unwrap().configuration, 12);
}

fn retry_old_configuration_after_lifecycle_panic(outgoing_destroy: bool) {
    let parent = root();
    let navigation = SlotNavigation::new();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let prepared = Rc::new(RefCell::new(None::<SharedInstances>));
    let captured = prepared.clone();
    let slot = child_slot(
        &parent,
        &navigation,
        Some(1u32),
        SlotOptions::default(),
        move |configuration, context| {
            count.set(count.get() + 1);
            if *configuration == 1 && outgoing_destroy && count.get() == 1 {
                let _ = context
                    .lifecycle()
                    .do_on_destroy(|| panic!("outgoing destroy failed"))
                    .detach();
            }
            if *configuration == 2 {
                *captured.borrow_mut() = Some(context.instance_keeper());
                if !outgoing_destroy {
                    let _ = context
                        .lifecycle()
                        .do_on_resume(|| panic!("incoming resume failed"))
                        .detach();
                }
            }
            context
        },
    )
    .unwrap();
    let old = slot.get().child.as_ref().unwrap().clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| navigation.activate(2))).is_err()
    );
    assert!(old.instance.lifecycle().state().is_destroyed());
    assert!(prepared.borrow().as_ref().unwrap().borrow().is_destroyed());
    let completed = Rc::new(Cell::new(false));
    let done = completed.clone();
    navigation.activate_with(1, move |result| {
        assert!(result.is_ok());
        done.set(true);
    });
    assert!(completed.get());
    let retried = slot.get().child.as_ref().unwrap().clone();
    assert_eq!(
        retried.instance.lifecycle().state(),
        LifecycleState::Resumed
    );
    assert!(!Rc::ptr_eq(&old.instance, &retried.instance));
    assert_ne!(old.key, retried.key);
    assert_eq!(calls.get(), 3);
    assert!(old.instance.instance_keeper().borrow().is_destroyed());
    assert!(!retried.instance.instance_keeper().borrow().is_destroyed());
    navigation.activate(1);
    assert!(Rc::ptr_eq(
        &retried.instance,
        &slot.get().child.as_ref().unwrap().instance
    ));
    assert_eq!(calls.get(), 3);
}

#[test]
fn retry_old_configuration_after_outgoing_destroy_panic_creates_live_slot_child() {
    retry_old_configuration_after_lifecycle_panic(true);
}

#[test]
fn retry_old_configuration_after_incoming_resume_panic_creates_live_slot_child() {
    retry_old_configuration_after_lifecycle_panic(false);
}

#[test]
fn dsl1_populated_bytes_remain_identical_to_frozen_pre_pages_fixture() {
    let parent = root();
    let options = SlotOptions {
        codec: Some(SlotCodec::new(|c: &u8| Ok(vec![*c]), |bytes| Ok(bytes[0]))),
        ..Default::default()
    };
    child_slot(
        &parent,
        &SlotNavigation::new(),
        Some(7u8),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    let fixture: &[u8] = &[
        68, 83, 76, 49, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 7, 7, 0, 0, 0, 99, 104,
        105, 108, 100, 47, 48, 8, 0, 0, 0, 68, 67, 83, 49, 0, 0, 0, 0,
    ];
    assert_eq!(
        parent.state_keeper().borrow().save().unwrap()["DefaultChildSlot"],
        fixture
    );
}
