use decompose_rs::router::stack::*;
use decompose_rs::router::{children::*, items::*, pages::*, panels::*, slot::*};
use decompose_rs::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn recreate(parent: &DefaultComponentContext) -> DefaultComponentContext {
    let snapshot = parent.state_store().save_snapshot().unwrap();
    parent.lifecycle().destroy().unwrap();
    root(
        StateKeeper::from_snapshot(snapshot),
        parent.instance_store(),
    )
}
fn memory_root(key: &str) -> DefaultComponentContext {
    let mut state = StateKeeper::new();
    state.enable_memory_navigation(key).unwrap();
    root(state, InstanceStore::from(InstanceKeeper::new()))
}

fn root(state: StateKeeper, instances: InstanceStore) -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::with_stores(
        lifecycle,
        StateStore::from(state),
        instances,
        SharedBack::new(),
    )
}

#[derive(PartialEq, Eq)]
struct Route(u32); // deliberately no Clone or codec

#[test]
fn memory_recreation_keeps_identity_nested_bytes_and_retained_instances_but_disk_resets() {
    let instances = InstanceStore::from(InstanceKeeper::new());
    let mut keeper = StateKeeper::new();
    keeper
        .enable_memory_navigation("DefaultChildStack")
        .unwrap();
    let parent = root(keeper, instances.clone());
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &parent,
        &navigation,
        vec![Route(1)],
        StackOptions::default(),
        |_, ctx| {
            ctx.state_store().register("nested", || vec![7]).unwrap();
            let retained = ctx
                .instance_store()
                .get_or_create("retained", || 42u32)
                .unwrap();
            (ctx, retained)
        },
    )
    .unwrap();
    navigation.push_or_panic(Route(2));
    let saved_child = stack.get().active.clone();
    let snapshot = parent.state_store().save_snapshot().unwrap();
    assert!(!snapshot.persistent().contains_key("DefaultChildStack"));
    assert!(
        !parent
            .state_store()
            .save()
            .unwrap()
            .contains_key("DefaultChildStack")
    );
    parent.lifecycle().destroy().unwrap();
    let recreated = root(StateKeeper::from_snapshot(snapshot.clone()), instances);
    let stack2 = child_stack_with_initial(
        &recreated,
        &StackNavigation::new(),
        || panic!("restoration must skip initializer"),
        StackOptions::default(),
        |_, ctx| {
            assert_eq!(
                ctx.state_store().consume_bytes("nested").unwrap(),
                Some(vec![7])
            );
            let retained = ctx
                .instance_store()
                .get_or_create("retained", || 0u32)
                .unwrap();
            (ctx, retained)
        },
    )
    .unwrap();
    assert_eq!(stack2.get().active.key, saved_child.key);
    assert!(Rc::ptr_eq(
        &stack2.get().active.configuration,
        &saved_child.configuration
    ));
    assert!(Rc::ptr_eq(
        &stack2.get().active.instance.1,
        &saved_child.instance.1
    ));
    let disk = root(
        StateKeeper::with_restored(snapshot.into_persistent()),
        InstanceStore::from(InstanceKeeper::new()),
    );
    let disk_stack = child_stack(
        &disk,
        &StackNavigation::new(),
        vec![Route(9)],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    assert_eq!(disk_stack.get().active.configuration.0, 9);
}

#[test]
fn failed_creation_returns_memory_snapshot_and_cancels_source() {
    let mut keeper = StateKeeper::new();
    keeper
        .enable_memory_navigation("DefaultChildStack")
        .unwrap();
    let parent = root(keeper, InstanceStore::from(InstanceKeeper::new()));
    child_stack(
        &parent,
        &StackNavigation::new(),
        vec![Route(8)],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    let snapshot = parent.state_store().save_snapshot().unwrap();
    parent.lifecycle().destroy().unwrap();
    let next = root(
        StateKeeper::from_snapshot(snapshot),
        InstanceStore::from(InstanceKeeper::new()),
    );
    let source = StackNavigation::<Route>::new();
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        child_stack_with_initial(
            &next,
            &source,
            || panic!("initializer"),
            StackOptions::default(),
            |_, _| panic!("factory"),
        )
    }));
    assert!(failed.is_err());
    assert!(
        next.state_store()
            .has_consumable("DefaultChildStack")
            .unwrap()
    );
    let restored = child_stack_with_initial(
        &next,
        &source,
        || panic!("initializer"),
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    assert_eq!(restored.get().active.configuration.0, 8);
}

#[test]
fn memory_router_inside_fixed_child_context_restores_through_root_snapshot() {
    let parent = root(
        StateKeeper::new(),
        InstanceStore::from(InstanceKeeper::new()),
    );
    let child = parent.child_context("fixed").unwrap();
    child
        .state_store()
        .enable_memory_navigation("DefaultChildStack")
        .unwrap();
    let nav = StackNavigation::new();
    child_stack(
        &child,
        &nav,
        vec![Route(1)],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    nav.push_or_panic(Route(7));
    let snapshot = parent.state_store().save_snapshot().unwrap();
    parent.lifecycle().destroy().unwrap();
    let next = root(
        StateKeeper::from_snapshot(snapshot),
        InstanceStore::from(InstanceKeeper::new()),
    );
    let child = next.child_context("fixed").unwrap();
    let restored = child_stack_with_initial(
        &child,
        &StackNavigation::<Route>::new(),
        || panic!("nested initializer"),
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    assert_eq!(restored.get().active.configuration.0, 7);
}

#[test]
fn slot_empty_is_restored_without_initializer() {
    let parent = memory_root(&SlotOptions::<Route>::default().key);
    let nav = SlotNavigation::new();
    child_slot(
        &parent,
        &nav,
        Some(Route(1)),
        SlotOptions::default(),
        |_, _| (),
    )
    .unwrap();
    nav.dismiss_or_panic();
    let next = recreate(&parent);
    let value = child_slot_with_initial(
        &next,
        &SlotNavigation::<Route>::new(),
        || panic!("empty is state"),
        SlotOptions::default(),
        |_, _| (),
    )
    .unwrap();
    assert!(value.get().child.is_none());
}

#[test]
fn pages_preserve_destroyed_entries_and_nested_memory_until_resurrection() {
    let parent = memory_root(&PagesOptions::<Route>::default().key);
    let nav = PagesNavigation::new();
    let pages = child_pages(
        &parent,
        &nav,
        Pages::new(vec![Route(1), Route(2)], 0).unwrap(),
        PagesOptions {
            page_status: Some(Rc::new(|index, state| {
                if Some(index) == state.selected_index {
                    PageStatus::Resumed
                } else {
                    PageStatus::Destroyed
                }
            })),
            ..PagesOptions::default()
        },
        |route, ctx| {
            let nested_nav = StackNavigation::new();
            ctx.state_store()
                .enable_memory_navigation("DefaultChildStack")
                .unwrap();
            let nested = child_stack_with_initial(
                &ctx,
                &nested_nav,
                || vec![Route(route.0 * 10)],
                StackOptions::default(),
                |_, _| (),
            )
            .unwrap();
            (ctx, nested_nav, nested)
        },
    )
    .unwrap();
    pages.get().items[0]
        .instance()
        .unwrap()
        .1
        .push_or_panic(Route(99));
    let key = pages.get().items[0].key().to_string();
    nav.select_or_panic(1);
    let next = recreate(&parent);
    let nav2 = PagesNavigation::<Route>::new();
    let restored = child_pages_with_initial(
        &next,
        &nav2,
        || panic!("pages initializer"),
        PagesOptions {
            page_status: Some(Rc::new(|index, state| {
                if Some(index) == state.selected_index {
                    PageStatus::Resumed
                } else {
                    PageStatus::Destroyed
                }
            })),
            ..PagesOptions::default()
        },
        |route, ctx| {
            let nested_nav = StackNavigation::new();
            ctx.state_store()
                .enable_memory_navigation("DefaultChildStack")
                .unwrap();
            let nested = child_stack_with_initial(
                &ctx,
                &nested_nav,
                || vec![Route(route.0 * 10)],
                StackOptions::default(),
                |_, _| (),
            )
            .unwrap();
            (ctx, nested_nav, nested)
        },
    )
    .unwrap();
    assert_eq!(restored.get().selected_index, Some(1));
    assert!(restored.get().items[0].instance().is_none());
    nav2.select_or_panic(0);
    assert_eq!(restored.get().items[0].key(), key);
    assert_eq!(
        restored.get().items[0]
            .instance()
            .unwrap()
            .2
            .get()
            .active
            .configuration
            .0,
        99
    );
}

#[test]
fn panels_mode_and_roles_and_items_activation_restore_without_codecs() {
    let parent = memory_root(&PanelsOptions::<Route, Route>::default().key);
    parent
        .state_store()
        .enable_memory_navigation(&ItemsOptions::<Route>::default().key)
        .unwrap();
    let panels_nav = PanelsNavigation::<Route, Route>::new();
    child_panels_without_extra(
        &parent,
        &panels_nav,
        Panels {
            main: Rc::new(Route(1)),
            details: Some(Rc::new(Route(2))),
            extra: None,
            mode: ChildPanelsMode::Dual,
        },
        PanelsOptions::default(),
        |_, _| (),
        |_, _| (),
    )
    .unwrap();
    let items_nav = ItemsNavigation::new();
    let items = child_items(
        &parent,
        &items_nav,
        Items::new(vec![Route(3), Route(4)]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    // Inactive entries become live through public lazy activation.
    let _ = items.get_or_create(&Route(4)).unwrap();
    let key = items
        .get()
        .active_items
        .get(&Route(4))
        .unwrap()
        .child
        .key
        .clone();
    let next = recreate(&parent);
    let panels = child_panels_without_extra_with_initial(
        &next,
        &PanelsNavigation::<Route, Route>::new(),
        || panic!("panels initializer"),
        PanelsOptions::default(),
        |_, _| (),
        |_, _| (),
    )
    .unwrap();
    assert_eq!(panels.get().mode, ChildPanelsMode::Dual);
    assert_eq!(panels.get().details.as_ref().unwrap().configuration.0, 2);
    let items = child_items_with_initial(
        &next,
        &ItemsNavigation::<Route>::new(),
        || panic!("items initializer"),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let _ = items.get_or_create(&Route(4)).unwrap();
    assert_eq!(
        items.get().active_items.get(&Route(4)).unwrap().child.key,
        key
    );
}

#[test]
fn generic_state_recreation_preserves_custom_projection() {
    let parent = memory_root("generic");
    let source = SimpleNavigation::<u32>::new();
    let options = || ChildrenOptions {
        key: "generic".into(),
        ..ChildrenOptions::default()
    };
    let value = children(
        &parent,
        &source,
        || vec![ChildNavState::new(Route(1), ChildStatus::Resumed)],
        options(),
        |mut state, event| {
            state.push(ChildNavState::new(Route(*event), ChildStatus::Created));
            state
        },
        |_, children| children,
        |_, ctx| ctx,
    )
    .unwrap();
    source.navigate(7);
    let saved = value.get()[1].configuration().clone();
    let next = recreate(&parent);
    let restored = children(
        &next,
        &SimpleNavigation::<u32>::new(),
        || panic!("generic initializer"),
        options(),
        |state: Vec<ChildNavState<Route>>, _| state,
        |_, children| children,
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(Rc::ptr_eq(restored.get()[1].configuration(), &saved));
}

#[test]
fn type_mismatch_is_typed_and_returns_consumed_snapshot() {
    let parent = memory_root("router");
    child_stack(
        &parent,
        &StackNavigation::<Route>::new(),
        vec![Route(1)],
        StackOptions {
            key: "router".into(),
            ..StackOptions::default()
        },
        |_, _| (),
    )
    .unwrap();
    let next = recreate(&parent);
    let failed = child_slot(
        &next,
        &SlotNavigation::<Route>::new(),
        Some(Route(1)),
        SlotOptions {
            key: "router".into(),
            ..SlotOptions::default()
        },
        |_, _| (),
    );
    assert!(matches!(failed, Err(NavigationError::State(_))));
    assert!(next.state_store().has_consumable("router").unwrap());
    let restored = child_stack_with_initial(
        &next,
        &StackNavigation::<Route>::new(),
        || panic!("initializer"),
        StackOptions {
            key: "router".into(),
            ..StackOptions::default()
        },
        |_, _| (),
    )
    .unwrap();
    assert_eq!(restored.get().active.configuration.0, 1);
}

#[test]
fn snapshot_calls_each_nested_provider_once_and_bytes_agree_with_memory() {
    let parent = root(
        StateKeeper::new(),
        InstanceStore::from(InstanceKeeper::new()),
    );
    let child = parent.child_context("fixed").unwrap();
    let calls = Rc::new(Cell::new(0u8));
    let count = calls.clone();
    child
        .state_store()
        .register("counter", move || {
            count.set(count.get() + 1);
            vec![count.get()]
        })
        .unwrap();
    let snapshot = parent.state_store().save_snapshot().unwrap();
    assert_eq!(calls.get(), 1);
    let byte_parent = root(
        StateKeeper::with_restored(snapshot.persistent().clone()),
        InstanceStore::from(InstanceKeeper::new()),
    );
    let memory_parent = root(
        StateKeeper::from_snapshot(snapshot),
        InstanceStore::from(InstanceKeeper::new()),
    );
    let bytes = byte_parent
        .child_context("fixed")
        .unwrap()
        .state_store()
        .consume_bytes("counter")
        .unwrap();
    let memory = memory_parent
        .child_context("fixed")
        .unwrap()
        .state_store()
        .consume_bytes("counter")
        .unwrap();
    assert_eq!(bytes, memory);
}

#[test]
fn dropping_snapshot_and_ended_provider_releases_memory_configuration() {
    let parent = memory_root("DefaultChildStack");
    let value = child_stack(
        &parent,
        &StackNavigation::new(),
        vec![Route(1)],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    let weak = Rc::downgrade(&value.get().active.configuration);
    let snapshot = parent.state_store().save_snapshot().unwrap();
    parent.lifecycle().destroy().unwrap();
    drop(value);
    drop(parent);
    assert!(weak.upgrade().is_some());
    drop(snapshot);
    assert!(weak.upgrade().is_none());
}

#[test]
fn memory_selection_bypasses_byte_codec_callbacks_and_predicates() {
    let parent = memory_root("DefaultChildStack");
    let codec = || {
        StackCodec::new(|_: &[Rc<Route>]| panic!("encode"), |_| panic!("decode"))
            .should_save(|_| panic!("save predicate"))
            .should_restore(|_| panic!("restore predicate"))
    };
    child_stack(
        &parent,
        &StackNavigation::new(),
        vec![Route(1)],
        StackOptions {
            codec: Some(codec()),
            ..StackOptions::default()
        },
        |_, _| (),
    )
    .unwrap();
    let next = recreate(&parent);
    let value = child_stack_with_initial(
        &next,
        &StackNavigation::<Route>::new(),
        || panic!("initializer"),
        StackOptions {
            codec: Some(codec()),
            ..StackOptions::default()
        },
        |_, _| (),
    )
    .unwrap();
    assert_eq!(value.get().active.configuration.0, 1);
}

#[test]
fn skipped_outer_codec_does_not_save_nested_memory_providers() {
    let parent = root(
        StateKeeper::new(),
        InstanceStore::from(InstanceKeeper::new()),
    );
    let calls = Rc::new(Cell::new(0));
    let out = calls.clone();
    child_stack(
        &parent,
        &StackNavigation::new(),
        vec![Route(1)],
        StackOptions {
            codec: Some(
                StackCodec::new(|_: &[Rc<Route>]| Ok(vec![1]), |_| Ok(vec![Route(1)]))
                    .should_save(|_| false),
            ),
            ..StackOptions::default()
        },
        move |_, ctx| {
            ctx.state_store()
                .enable_memory_navigation("nested")
                .unwrap();
            let count = out.clone();
            child_stack(
                &ctx,
                &StackNavigation::new(),
                vec![Route(2)],
                StackOptions {
                    key: "nested".into(),
                    ..StackOptions::default()
                },
                move |_, nested| {
                    let count = count.clone();
                    nested
                        .state_store()
                        .register("count", move || {
                            count.set(count.get() + 1);
                            vec![1]
                        })
                        .unwrap();
                },
            )
            .unwrap();
        },
    )
    .unwrap();
    let snapshot = parent.state_store().save_snapshot().unwrap();
    assert!(!snapshot.persistent().contains_key("DefaultChildStack"));
    assert_eq!(snapshot.memory_count(), 0);
    assert_eq!(calls.get(), 0);
}

#[test]
fn snapshot_during_restored_construction_preserves_provisional_memory_input() {
    let parent = memory_root("DefaultChildStack");
    child_stack(
        &parent,
        &StackNavigation::new(),
        vec![Route(7)],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    let next = recreate(&parent);
    let saved = Rc::new(RefCell::new(None));
    let output = saved.clone();
    let keeper = next.state_store();
    child_stack_with_initial(
        &next,
        &StackNavigation::<Route>::new(),
        || panic!("initializer"),
        StackOptions::default(),
        move |_, _| {
            *output.borrow_mut() = Some(keeper.save_snapshot().unwrap());
        },
    )
    .unwrap();
    let provisional = saved.borrow_mut().take().unwrap();
    assert_eq!(provisional.memory_count(), 1);
    let retry = root(
        StateKeeper::from_snapshot(provisional),
        InstanceStore::from(InstanceKeeper::new()),
    );
    let value = child_stack_with_initial(
        &retry,
        &StackNavigation::<Route>::new(),
        || panic!("lost original memory input"),
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    assert_eq!(value.get().active.configuration.0, 7);
}

#[test]
fn successful_fixed_child_snapshot_refreshes_last_good_durable_frame() {
    for seed_byte_cache in [false, true] {
        let parent = root(
            StateKeeper::new(),
            InstanceStore::from(InstanceKeeper::new()),
        );
        let child = parent.child_context("fixed").unwrap();
        let fail = Rc::new(Cell::new(false));
        let value = Rc::new(Cell::new(1u8));
        let calls = Rc::new(Cell::new(0));
        let output = value.clone();
        let failure = fail.clone();
        let count = calls.clone();
        child
            .state_store()
            .register_fallible("value", move || {
                count.set(count.get() + 1);
                if failure.get() {
                    Err("nested encode failed".into())
                } else {
                    Ok(vec![output.get()])
                }
            })
            .unwrap();
        if seed_byte_cache {
            parent.state_store().save().unwrap();
        }
        value.set(2);
        let before = calls.get();
        let snapshot = parent.state_store().save_snapshot().unwrap();
        assert_eq!(calls.get(), before + 1);
        fail.set(true);
        let saved = parent.state_store().save().unwrap();
        assert_eq!(saved.get("fixed"), snapshot.persistent().get("fixed"));
        let recovered = root(
            StateKeeper::with_restored(saved),
            InstanceStore::from(InstanceKeeper::new()),
        );
        assert_eq!(
            recovered
                .child_context("fixed")
                .unwrap()
                .state_store()
                .consume_bytes("value")
                .unwrap(),
            Some(vec![2])
        );
    }
}

#[test]
fn persistent_outer_keeps_nested_memory_in_snapshot_but_durable_restore_resets_it() {
    let codec = StackCodec::new(
        |configs: &[Rc<Route>]| Ok(configs.iter().flat_map(|c| c.0.to_le_bytes()).collect()),
        |bytes| {
            if bytes.len() % 4 != 0 {
                return Err("bad route payload".into());
            }
            Ok(bytes
                .chunks_exact(4)
                .map(|b| Route(u32::from_le_bytes(b.try_into().unwrap())))
                .collect())
        },
    );
    let factory = |route: &Route, context: DefaultComponentContext| {
        context
            .state_store()
            .enable_memory_navigation("nested")
            .unwrap();
        let nav = StackNavigation::new();
        let inner = child_stack_with_initial(
            &context,
            &nav,
            || vec![Route(route.0 * 10)],
            StackOptions {
                key: "nested".into(),
                ..Default::default()
            },
            |_, _| (),
        )
        .unwrap();
        (nav, inner)
    };
    let parent = root(
        StateKeeper::new(),
        InstanceStore::from(InstanceKeeper::new()),
    );
    let nav = StackNavigation::new();
    let outer = child_stack(
        &parent,
        &nav,
        vec![Route(1)],
        StackOptions {
            codec: Some(codec.clone()),
            ..Default::default()
        },
        factory,
    )
    .unwrap();
    nav.push_or_panic(Route(2));
    outer.get().active.instance.0.push_or_panic(Route(99));
    let original = outer.get().active.instance.1.get().active.clone();
    let snapshot = parent.state_store().save_snapshot().unwrap();
    assert!(snapshot.persistent().contains_key("DefaultChildStack"));
    let bytes = snapshot.persistent().clone();
    parent.lifecycle().destroy().unwrap();
    let memory = root(
        StateKeeper::from_snapshot(snapshot),
        InstanceStore::from(InstanceKeeper::new()),
    );
    let disk = root(
        StateKeeper::with_restored(bytes),
        InstanceStore::from(InstanceKeeper::new()),
    );
    for (parent, memory) in [(&memory, true), (&disk, false)] {
        let outer = child_stack_with_initial(
            parent,
            &StackNavigation::<Route>::new(),
            || panic!("outer byte membership is authoritative"),
            StackOptions {
                codec: Some(codec.clone()),
                ..Default::default()
            },
            factory,
        )
        .unwrap();
        assert_eq!(outer.get().active.configuration.0, 2);
        let inner = outer.get().active.instance.1.get();
        if memory {
            assert_eq!(inner.active.configuration.0, 99);
            assert_eq!(inner.active.key, original.key);
            assert!(Rc::ptr_eq(
                &inner.active.configuration,
                &original.configuration
            ));
        } else {
            assert_eq!(inner.active.configuration.0, 20);
            assert!(inner.back_stack.is_empty());
        }
    }
}
