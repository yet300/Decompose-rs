use decompose_rs::router::stack::*;
use decompose_rs::*;
use std::{cell::RefCell, rc::Rc};

fn root() -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::new(lifecycle)
}

#[test]
fn stack_preserves_survivors_and_destroys_removed_snapshots() {
    let root = root();
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &root,
        &navigation,
        vec![1u32],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let first = stack.get();
    let first_instance = first.active.instance.clone();
    navigation.push(2);
    let second = stack.get();
    assert_eq!(*second.active.configuration, 2);
    assert!(Rc::ptr_eq(&second.back_stack[0].instance, &first_instance));
    assert_eq!(first_instance.lifecycle().state(), LifecycleState::Created);
    assert_eq!(
        second.active.instance.lifecycle().state(),
        LifecycleState::Resumed
    );
    navigation.pop();
    assert!(Rc::ptr_eq(&stack.get().active.instance, &first_instance));
    assert_eq!(
        second.active.instance.lifecycle().state(),
        LifecycleState::Destroyed
    );
    assert!(
        second
            .active
            .instance
            .instance_keeper()
            .borrow()
            .is_destroyed()
    );
}

#[test]
fn validation_leaves_the_old_stack_intact() {
    let root = root();
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &root,
        &navigation,
        vec![1u32],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let outcome = Rc::new(RefCell::new(None));
    let peer = outcome.clone();
    navigation.navigate_with(|_| vec![], move |result| *peer.borrow_mut() = Some(result));
    assert!(outcome.borrow().as_ref().unwrap().is_err());
    assert_eq!(*stack.get().active.configuration, 1);
    let peer = outcome.clone();
    navigation.navigate_with(
        |old| vec![old[0].clone(), old[0].clone()],
        move |result| *peer.borrow_mut() = Some(result),
    );
    assert!(outcome.borrow().as_ref().unwrap().is_err());
    assert_eq!(stack.get().items().count(), 1);
}

#[test]
fn observer_navigation_is_queued_until_the_current_stack_is_published() {
    let root = root();
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &root,
        &navigation,
        vec![1u32],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let log = Rc::new(RefCell::new(Vec::new()));
    let peer = log.clone();
    let nav = navigation.clone();
    let _subscription = stack.subscribe(move |state| {
        peer.borrow_mut().push(*state.active.configuration);
        if *state.active.configuration == 2 {
            nav.push(3);
        }
    });
    navigation.push(2);
    assert_eq!(&*log.borrow(), &[1, 2, 3]);
    assert_eq!(stack.get().items().count(), 3);
}

#[test]
fn back_prefers_child_then_pops_and_allows_callback_navigation() {
    let root = root();
    let navigation = StackNavigation::new();
    let nav = navigation.clone();
    let options = StackOptions {
        handle_back_button: true,
        ..StackOptions::default()
    };
    let stack = child_stack(&root, &navigation, vec![1u32, 2], options, move |_, ctx| {
        let nav = nav.clone();
        ctx.back_handler()
            .register(0, true, move |_, back| {
                back.set_enabled(0, false);
                nav.push(3);
            })
            .unwrap();
        ctx
    })
    .unwrap();
    assert!(root.back_handler().back());
    assert_eq!(*stack.get().active.configuration, 3);
}

#[test]
fn parent_lifecycle_bounds_all_children() {
    let root = root();
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &root,
        &navigation,
        vec![1u32, 2],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    root.lifecycle().move_to(LifecycleState::Created).unwrap();
    assert_eq!(
        stack.get().active.instance.lifecycle().state(),
        LifecycleState::Created
    );
    root.lifecycle().move_to(LifecycleState::Resumed).unwrap();
    assert_eq!(
        stack.get().active.instance.lifecycle().state(),
        LifecycleState::Resumed
    );
    root.lifecycle().destroy().unwrap();
    assert!(
        stack
            .get()
            .items()
            .all(|item| item.instance.lifecycle().state().is_destroyed())
    );
    navigation.push(3);
    assert_eq!(*stack.get().active.configuration, 2);
}

fn codec() -> StackCodec<u32> {
    StackCodec::new(
        |configs: &[Rc<u32>]| Ok(configs.iter().flat_map(|c| c.to_le_bytes()).collect()),
        |bytes| {
            if bytes.len() % 4 != 0 {
                return Err("invalid route bytes".into());
            }
            Ok(bytes
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
                .collect())
        },
    )
}

#[test]
fn persistent_stack_restores_order_child_state_keys_and_retained_instances() {
    let retained = Rc::new(RefCell::new(InstanceKeeper::new()));
    let old_root =
        DefaultComponentContext::with_instance_keeper(root().lifecycle(), retained.clone());
    let navigation = StackNavigation::new();
    let options = StackOptions {
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let stack = child_stack(&old_root, &navigation, vec![1u32, 2], options, |_, ctx| ctx).unwrap();
    let before = stack.get();
    let service = before
        .active
        .instance
        .instance_keeper()
        .borrow_mut()
        .get_or_create("service", || 5u32)
        .unwrap();
    before
        .active
        .instance
        .state_keeper()
        .borrow_mut()
        .register("counter", || vec![9])
        .unwrap();
    let saved = old_root.state_keeper().borrow().save().unwrap();
    old_root.lifecycle().destroy().unwrap();
    let restored = DefaultComponentContext::with_all(
        root().lifecycle(),
        Rc::new(RefCell::new(StateKeeper::with_restored(saved.clone()))),
        retained.clone(),
        SharedBack::new(),
    );
    let options = StackOptions {
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let nav = StackNavigation::new();
    let after = child_stack(&restored, &nav, vec![99], options, |_, ctx| ctx).unwrap();
    assert_eq!(
        after
            .get()
            .items()
            .map(|child| *child.configuration)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(after.get().active.key, before.active.key);
    assert!(!Rc::ptr_eq(
        &after.get().active.instance,
        &before.active.instance
    ));
    assert!(Rc::ptr_eq(
        &service,
        &after
            .get()
            .active
            .instance
            .instance_keeper()
            .borrow_mut()
            .get_or_create("service", || 99u32)
            .unwrap()
    ));
    assert_eq!(
        after
            .get()
            .active
            .instance
            .state_keeper()
            .borrow_mut()
            .consume_bytes("counter"),
        Some(vec![9])
    );
    nav.push(3);
    assert_ne!(after.get().active.key, before.active.key);
    let fresh = DefaultComponentContext::with_state_keeper(
        root().lifecycle(),
        Rc::new(RefCell::new(StateKeeper::with_restored(saved))),
    );
    let options = StackOptions {
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let reborn = child_stack(
        &fresh,
        &StackNavigation::new(),
        vec![99],
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(!Rc::ptr_eq(
        &service,
        &reborn
            .get()
            .active
            .instance
            .instance_keeper()
            .borrow_mut()
            .get_or_create("service", || 99u32)
            .unwrap()
    ));
}

#[test]
fn corrupt_saved_key_counter_is_rejected_without_consuming_state() {
    let root = root();
    let options = StackOptions {
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let _stack = child_stack(
        &root,
        &StackNavigation::new(),
        vec![1u32, 2],
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    let mut saved = root.state_keeper().borrow().save().unwrap();
    saved.get_mut("DefaultChildStack").unwrap()[4..12].copy_from_slice(&0u64.to_le_bytes());
    let restored = DefaultComponentContext::with_state_keeper(
        self::root().lifecycle(),
        Rc::new(RefCell::new(StateKeeper::with_restored(saved.clone()))),
    );
    let options = StackOptions {
        codec: Some(codec()),
        ..StackOptions::default()
    };
    assert!(
        child_stack(
            &restored,
            &StackNavigation::new(),
            vec![99],
            options,
            |_, ctx| ctx
        )
        .is_err()
    );
    assert_eq!(restored.state_keeper().borrow().save().unwrap(), saved);
    assert!(restored.instance_keeper().borrow().is_empty());
}

#[test]
fn back_pops_only_when_child_declines_and_never_pops_the_root() {
    let root = root();
    let nav = StackNavigation::new();
    let options = StackOptions {
        handle_back_button: true,
        ..StackOptions::default()
    };
    let stack = child_stack(&root, &nav, vec![1u32, 2], options, |_, ctx| ctx).unwrap();
    assert!(root.back_handler().back());
    assert_eq!(*stack.get().active.configuration, 1);
    assert!(!root.back_handler().back());
}

#[test]
fn no_op_navigation_does_not_stop_and_resume_the_active_component() {
    let root = root();
    let nav = StackNavigation::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    let peer = log.clone();
    let stack = child_stack(
        &root,
        &nav,
        vec![1u32],
        StackOptions::default(),
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
    nav.push_new(1);
    nav.pop();
    assert!(log.borrow().is_empty());
    assert_eq!(
        stack.get().active.instance.lifecycle().state(),
        LifecycleState::Resumed
    );
}

#[test]
fn factory_panic_during_initialization_rolls_back_restored_bytes_and_observers() {
    let root = root();
    let options = StackOptions {
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let _stack = child_stack(
        &root,
        &StackNavigation::new(),
        vec![1u32],
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = root.state_keeper().borrow().save().unwrap();
    let restored = DefaultComponentContext::with_state_keeper(
        self::root().lifecycle(),
        Rc::new(RefCell::new(StateKeeper::with_restored(saved.clone()))),
    );
    let before = restored.lifecycle().subscriber_count();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let options = StackOptions {
            codec: Some(codec()),
            ..StackOptions::default()
        };
        child_stack(
            &restored,
            &StackNavigation::new(),
            vec![99],
            options,
            |_, _| -> () { panic!("factory failure") },
        )
    }));
    assert!(outcome.is_err());
    assert_eq!(restored.state_keeper().borrow().save().unwrap(), saved);
    assert!(restored.instance_keeper().borrow().is_empty());
    assert_eq!(restored.lifecycle().subscriber_count(), before);
    assert_eq!(restored.back_handler().handler_count(), 0);
}

#[test]
fn codec_failure_surfaces_as_statekeeper_encode_error() {
    let root = root();
    let options = StackOptions {
        codec: Some(StackCodec::new(
            |_| Err("codec failure".into()),
            |_| Ok(vec![1u32]),
        )),
        ..StackOptions::default()
    };
    let _stack = child_stack(
        &root,
        &StackNavigation::new(),
        vec![1u32],
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(matches!(
        root.state_keeper().borrow().save(),
        Err(StateKeeperError::Encode { .. })
    ));
}

#[test]
fn panicking_back_callback_discards_deferred_navigation() {
    let root = root();
    let nav = StackNavigation::new();
    let stack = child_stack(
        &root,
        &nav,
        vec![1u32],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let first = Rc::new(std::cell::Cell::new(true));
    let peer = first.clone();
    let source = nav.clone();
    root.back_handler()
        .register(2, true, move |_, _| {
            if peer.replace(false) {
                source.push(2);
                panic!("back failed");
            }
            source.push(3);
        })
        .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| root.back_handler().back()))
            .is_err()
    );
    nav.push(4);
    assert_eq!(
        stack
            .get()
            .items()
            .map(|c| *c.configuration)
            .collect::<Vec<_>>(),
        [1, 4]
    );
    root.back_handler().back();
    assert_eq!(
        stack
            .get()
            .items()
            .map(|c| *c.configuration)
            .collect::<Vec<_>>(),
        [1, 4, 3]
    );
}

#[test]
fn panic_in_later_factory_releases_prepared_retained_services() {
    struct Service(Rc<std::cell::Cell<u32>>);
    impl Drop for Service {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let root = root();
    let nav = StackNavigation::new();
    let drops = Rc::new(std::cell::Cell::new(0));
    let peer = drops.clone();
    let stack = child_stack(
        &root,
        &nav,
        vec![1u32],
        StackOptions::default(),
        move |config, ctx| {
            if *config == 3 {
                panic!("later factory failure");
            }
            if *config == 2 {
                let drops = peer.clone();
                ctx.instance_keeper()
                    .borrow_mut()
                    .get_or_create("service", || Service(drops))
                    .unwrap();
            }
            ctx
        },
    )
    .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| nav.replace_all(vec![2, 3])))
            .is_err()
    );
    assert_eq!(drops.get(), 1);
    assert_eq!(*stack.get().active.configuration, 1);
    nav.push(4);
    assert_eq!(*stack.get().active.configuration, 4);
}

#[test]
fn inactive_child_cannot_reenable_its_back_bridge_while_pausing() {
    let root = root();
    let nav = StackNavigation::new();
    let handled = Rc::new(std::cell::Cell::new(0));
    let peer = handled.clone();
    let options = StackOptions {
        handle_back_button: true,
        ..StackOptions::default()
    };
    let stack = child_stack(&root, &nav, vec![1u32], options, move |config, ctx| {
        if *config == 1 {
            let back = ctx.back_handler();
            let handled = peer.clone();
            let _ = ctx
                .lifecycle()
                .do_on_pause(move || {
                    let handled = handled.clone();
                    back.register(0, true, move |_, _| handled.set(handled.get() + 1))
                        .unwrap();
                })
                .detach();
        }
        ctx
    })
    .unwrap();
    nav.push(2);
    assert!(root.back_handler().back());
    assert_eq!(handled.get(), 0);
    assert_eq!(*stack.get().active.configuration, 1);
}

#[test]
fn stack_key_is_reserved_before_a_factory_can_create_another_stack() {
    let root = root();
    let nested = root.clone();
    let rejected = Rc::new(std::cell::Cell::new(false));
    let peer = rejected.clone();
    let result = child_stack(
        &root,
        &StackNavigation::new(),
        vec![1u32],
        StackOptions::default(),
        move |_, ctx| {
            let inner = child_stack(
                &nested,
                &StackNavigation::new(),
                vec![9u32],
                StackOptions::default(),
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
fn earlier_saved_snapshot_does_not_reuse_a_removed_childs_retained_scope() {
    let retained = Rc::new(RefCell::new(InstanceKeeper::new()));
    let root = DefaultComponentContext::with_instance_keeper(root().lifecycle(), retained.clone());
    let nav = StackNavigation::new();
    let options = StackOptions {
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let stack = child_stack(&root, &nav, vec![1u32], options, |_, ctx| ctx).unwrap();
    let saved = root.state_keeper().borrow().save().unwrap();
    nav.push(2);
    let removed = stack.get().active.instance.instance_keeper();
    root.lifecycle().destroy().unwrap();
    let restored = DefaultComponentContext::with_all(
        self::root().lifecycle(),
        Rc::new(RefCell::new(StateKeeper::with_restored(saved))),
        retained,
        SharedBack::new(),
    );
    let nav = StackNavigation::new();
    let options = StackOptions {
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let stack = child_stack(&restored, &nav, vec![99], options, |_, ctx| ctx).unwrap();
    assert!(removed.borrow().is_destroyed());
    nav.push(3);
    assert!(!Rc::ptr_eq(
        &removed,
        &stack.get().active.instance.instance_keeper()
    ));
}

#[test]
fn destroying_parent_in_initial_factory_aborts_construction() {
    let root = root();
    let lifecycle = root.lifecycle();
    let result = child_stack(
        &root,
        &StackNavigation::new(),
        vec![1u32],
        StackOptions::default(),
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
            .is_registered("DefaultChildStack")
    );
}

#[test]
fn back_enabled_state_is_ready_when_a_value_observer_runs() {
    let root = root();
    let nav = StackNavigation::new();
    let options = StackOptions {
        handle_back_button: true,
        ..StackOptions::default()
    };
    let stack = child_stack(&root, &nav, vec![1u32], options, |_, ctx| ctx).unwrap();
    let back = root.back_handler();
    let handled = Rc::new(std::cell::Cell::new(false));
    let peer = handled.clone();
    let _sub = stack.subscribe(move |state| {
        if *state.active.configuration == 2 {
            peer.set(back.back());
        }
    });
    nav.push(2);
    assert!(handled.get());
    assert_eq!(*stack.get().active.configuration, 1);
}

#[test]
fn parent_destroyed_while_old_active_pauses_releases_uncommitted_children() {
    let root = DefaultComponentContext::with_instance_keeper(
        root().lifecycle(),
        Rc::new(RefCell::new(InstanceKeeper::new())),
    );
    let nav = StackNavigation::new();
    let parent = root.lifecycle();
    let created = Rc::new(RefCell::new(None::<SharedInstances>));
    let peer = created.clone();
    let stack = child_stack(
        &root,
        &nav,
        vec![1u32],
        StackOptions::default(),
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
        |mut old| {
            old.push(Rc::new(2));
            old
        },
        move |value| *result.borrow_mut() = Some(value),
    );
    assert!(matches!(
        &*outcome.borrow(),
        Some(Err(NavigationError::Destroyed))
    ));
    assert!(created.borrow().as_ref().unwrap().borrow().is_destroyed());
    assert_eq!(*stack.get().active.configuration, 1);
    assert_eq!(root.back_handler().handler_count(), 0);
}

#[test]
fn initial_resume_destroying_parent_is_a_construction_error() {
    let root = root();
    let lifecycle = root.lifecycle();
    let result = child_stack(
        &root,
        &StackNavigation::new(),
        vec![1u32],
        StackOptions::default(),
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
            .is_registered("DefaultChildStack")
    );
    assert_eq!(root.back_handler().handler_count(), 0);
}

#[test]
fn panicking_navigation_queued_by_initial_factory_rolls_back_constructor() {
    let root = root();
    let nav = StackNavigation::new();
    let source = nav.clone();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        child_stack(
            &root,
            &nav,
            vec![1u32],
            StackOptions::default(),
            move |config, ctx| {
                if *config == 1 {
                    source.push(2);
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
            .is_registered("DefaultChildStack")
    );
    assert!(root.instance_keeper().borrow().is_empty());
    assert_eq!(root.back_handler().handler_count(), 0);
}

#[test]
fn custom_nonclone_context_and_configuration_work_through_three_stacks() {
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
    let one = child_stack(
        &parent,
        &StackNavigation::new(),
        vec![Config(1)],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let two = child_stack(
        one.get().active.instance.as_ref(),
        &StackNavigation::new(),
        vec![Config(2)],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let three = child_stack(
        two.get().active.instance.as_ref(),
        &StackNavigation::new(),
        vec![Config(3)],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(Rc::ptr_eq(&service, &three.get().active.instance.service));
    parent.lifecycle().destroy().unwrap();
    assert!(
        three
            .get()
            .active
            .instance
            .lifecycle()
            .state()
            .is_destroyed()
    );
}

#[test]
fn dropping_views_keeps_navigation_alive_until_parent_destroy_without_a_cycle() {
    let root = root();
    let nav = StackNavigation::new();
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let stack = child_stack(
        &root,
        &nav,
        vec![1u32],
        StackOptions::default(),
        move |_, ctx| {
            let _ = &capture;
            ctx
        },
    )
    .unwrap();
    drop(stack);
    nav.push(2);
    assert!(weak.upgrade().is_some());
    root.lifecycle().destroy().unwrap();
    assert!(weak.upgrade().is_none());
    assert_eq!(root.back_handler().handler_count(), 0);
}

#[test]
fn saves_use_one_membership_snapshot_even_when_a_provider_navigates() {
    let root = root();
    let nav = StackNavigation::new();
    let source = nav.clone();
    let once = Rc::new(std::cell::Cell::new(true));
    let options = StackOptions {
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let stack = child_stack(&root, &nav, vec![1u32], options, move |config, ctx| {
        if *config == 1 {
            let nav = source.clone();
            let once = once.clone();
            ctx.state_keeper()
                .borrow_mut()
                .register("state", move || {
                    if once.replace(false) {
                        nav.replace_current(2);
                    }
                    vec![11]
                })
                .unwrap();
        }
        ctx
    })
    .unwrap();
    let original_key = stack.get().active.key.clone();
    let saved = root.state_keeper().borrow().save().unwrap();
    assert_eq!(*stack.get().active.configuration, 2);
    let restored = DefaultComponentContext::with_state_keeper(
        self::root().lifecycle(),
        Rc::new(RefCell::new(StateKeeper::with_restored(saved))),
    );
    let options = StackOptions {
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let stack = child_stack(
        &restored,
        &StackNavigation::new(),
        vec![99],
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(*stack.get().active.configuration, 1);
    assert_eq!(stack.get().active.key, original_key);
    assert_eq!(
        stack
            .get()
            .active
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
    let nav = StackNavigation::new();
    let source = nav.clone();
    let parent = root.lifecycle();
    let result = child_stack(
        &root,
        &nav,
        vec![1u32],
        StackOptions::default(),
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
