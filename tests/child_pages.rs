use decompose_rs::router::pages::*;
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
fn dpg1_bytes_match_pre_panels_a1f675b_fixture() {
    // Literal layout from the a1f675b encoder: one selected u32 configuration,
    // next-key 1, child/0, and an empty DCS1 child-state frame.
    const FIXTURE: &[u8] = &[
        68, 80, 71, 49, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 7, 0, 0, 0, 1,
        0, 0, 0, 7, 0, 0, 0, 99, 104, 105, 108, 100, 47, 48, 8, 0, 0, 0, 68, 67, 83, 49, 0, 0, 0,
        0,
    ];
    assert_eq!(FIXTURE.len(), 55);
    let parent = restored_root(
        std::collections::BTreeMap::from([("DefaultChildPages".into(), FIXTURE.to_vec())]),
        None,
    );
    let value = child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let pages = value.get();
    assert_eq!(pages.selected_index, Some(0));
    assert_eq!(**pages.items[0].configuration(), 7);
    assert_eq!(pages.items[0].key(), "child/0");
    assert_eq!(
        parent.state_store().save().unwrap()["DefaultChildPages"],
        FIXTURE
    );
}
#[test]
fn pages_selection_lazy_children_and_numeric_set_items() {
    let parent = root();
    let nav = PagesNavigation::new();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3, 4], 0).unwrap(),
        PagesOptions::default(),
        move |_, ctx| {
            count.set(count.get() + 1);
            ctx
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(value.get().selected_index, Some(0));
    assert!(value.get().items[2].instance().is_none());
    nav.select_or_panic(2);
    assert_eq!(calls.get(), 4);
    assert!(value.get().items[0].instance().is_none());
    assert_eq!(
        value.get().items[2].instance().unwrap().lifecycle().state(),
        LifecycleState::Resumed
    );
    nav.set_items_or_panic(|mut items| {
        items.reverse();
        items
    });
    assert_eq!(value.get().selected_index, Some(2));
    nav.select_last_or_panic();
    nav.select_next_or_panic(false);
    assert_eq!(value.get().selected_index, Some(4));
    nav.select_next_or_panic(true);
    assert_eq!(value.get().selected_index, Some(0));
    nav.select_prev_or_panic(true);
    assert_eq!(value.get().selected_index, Some(4));
    nav.set_items_or_panic(|items| items.into_iter().take(2).collect());
    assert_eq!(value.get().selected_index, Some(1));
    nav.clear_or_panic();
    nav.select_or_panic(usize::MAX);
    nav.select_first_or_panic();
    nav.select_last_or_panic();
    assert_eq!(value.get().selected_index, None);
    assert!(value.get().items.is_empty());
}
#[test]
fn pages_invalid_selection_is_typed_and_noop_empty_is_normalized() {
    assert!(Pages::new(vec![1u32], 1).is_err());
    assert_eq!(
        Pages::<u32>::new(vec![], usize::MAX)
            .unwrap()
            .selected_index,
        None
    );
    let parent = root();
    let nav = PagesNavigation::new();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![1u32], 0).unwrap(),
        PagesOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let error = Rc::new(RefCell::new(None));
    let output = error.clone();
    nav.select_with(5, move |r| *output.borrow_mut() = r.err());
    assert!(matches!(
        *error.borrow(),
        Some(NavigationError::InvalidSelection { .. })
    ));
    assert_eq!(value.get().selected_index, Some(0));
}

fn restored_root(
    saved: std::collections::BTreeMap<String, Vec<u8>>,
    instances: Option<InstanceStore>,
) -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::with_stores(
        lifecycle,
        StateStore::from(StateKeeper::with_restored(saved)),
        instances.unwrap_or_else(|| InstanceStore::from(InstanceKeeper::new())),
        SharedBack::new(),
    )
}
fn codec() -> PagesCodec<u32> {
    PagesCodec::new(
        |items: &[Rc<u32>]| Ok(items.iter().flat_map(|c| c.to_le_bytes()).collect()),
        |bytes| {
            if bytes.len() % 4 != 0 {
                return Err("invalid u32 payload".into());
            }
            Ok(bytes
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                .collect())
        },
    )
}
fn persistent() -> PagesOptions<u32> {
    PagesOptions {
        codec: Some(codec()),
        ..Default::default()
    }
}
#[test]
fn tombstone_preserves_key_and_state_but_ends_old_component_and_keeper() {
    let parent = root();
    let nav = PagesNavigation::new();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        PagesOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let old = value.get().items[0].instance().unwrap().clone();
    let key = value.get().items[0].key().to_owned();
    old.state_store().register("counter", || vec![42]).unwrap();
    nav.select_or_panic(3);
    assert_eq!(value.get().items[0].key(), key);
    assert!(value.get().items[0].instance().is_none());
    assert!(old.lifecycle().state().is_destroyed());
    assert!(old.instance_store().is_destroyed().unwrap());
    nav.select_or_panic(0);
    let resurrected = value.get().items[0].instance().unwrap().clone();
    assert_eq!(value.get().items[0].key(), key);
    assert!(!Rc::ptr_eq(&old, &resurrected));
    assert!(!old.instance_store().ptr_eq(&resurrected.instance_store()));
    assert_eq!(
        resurrected.state_store().consume_bytes("counter").unwrap(),
        Some(vec![42])
    );
}
#[test]
fn custom_started_and_circular_policy_controls_lifecycle_and_back_with_parent_bound() {
    let parent = root();
    let nav = PagesNavigation::new();
    let options = PagesOptions {
        page_status: Some(Rc::new(|i, p: &Pages<u32>| {
            if i == p.selected_index.unwrap() {
                PageStatus::Started
            } else if i == 3 {
                PageStatus::Created
            } else {
                PageStatus::Destroyed
            }
        })),
        ..Default::default()
    };
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    let current = value.get().items[0].instance().unwrap().clone();
    let created = value.get().items[3].instance().unwrap().clone();
    let hits = Rc::new(Cell::new(0));
    let count = hits.clone();
    current
        .back_handler()
        .register(0, true, move |_, _| count.set(count.get() + 1))
        .unwrap();
    created
        .back_handler()
        .register(100, true, |_, _| panic!("created child intercepted"))
        .unwrap();
    assert_eq!(current.lifecycle().state(), LifecycleState::Started);
    assert!(parent.back_handler().back());
    assert_eq!(hits.get(), 1);
    parent.lifecycle().move_to(LifecycleState::Created).unwrap();
    assert!(!parent.back_handler().back());
    parent.lifecycle().move_to(LifecycleState::Resumed).unwrap();
    assert!(parent.back_handler().back());
    assert_eq!(hits.get(), 2);
}
#[test]
fn reorder_preserves_instances_keys_and_duplicate_occurrence_identity() {
    let parent = root();
    let nav = PagesNavigation::new();
    let options = PagesOptions {
        allow_duplicates: true,
        page_status: Some(Rc::new(|_, _| PageStatus::Created)),
        ..Default::default()
    };
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![1u32, 1, 2], 0).unwrap(),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    let before = value.get();
    nav.set_items_or_panic(|items| vec![items[2].clone(), items[1].clone(), items[0].clone()]);
    let after = value.get();
    for (old, new) in [(0, 1), (1, 2), (2, 0)] {
        assert_eq!(before.items[old].key(), after.items[new].key());
        assert!(Rc::ptr_eq(
            before.items[old].instance().unwrap(),
            after.items[new].instance().unwrap()
        ));
    }
    let parent = root();
    assert!(matches!(
        child_pages(
            &parent,
            &PagesNavigation::new(),
            Pages::new(vec![1u32, 1], 0).unwrap(),
            PagesOptions::default(),
            |_, ctx| ctx
        ),
        Err(NavigationError::DuplicateConfiguration)
    ));
}
#[test]
fn custom_circular_neighbours_are_created_at_both_ends() {
    let parent = root();
    let nav = PagesNavigation::new();
    let options = PagesOptions {
        page_status: Some(Rc::new(|i, p: &Pages<u32>| {
            let selected = p.selected_index.unwrap();
            if i == selected {
                PageStatus::Resumed
            } else if (i + 1) % p.items.len() == selected || (selected + 1) % p.items.len() == i {
                PageStatus::Created
            } else {
                PageStatus::Destroyed
            }
        })),
        ..Default::default()
    };
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(value.get().items[3].instance().is_some());
    assert!(value.get().items[2].instance().is_none());
    nav.select_prev_or_panic(true);
    assert_eq!(value.get().selected_index, Some(3));
    assert!(value.get().items[0].instance().is_some());
    assert!(value.get().items[1].instance().is_none());
}
#[test]
fn single_page_boundaries_and_all_completion_helpers_report_real_state() {
    let parent = root();
    let nav = PagesNavigation::new();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![7u32], 0).unwrap(),
        PagesOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let seen = Rc::new(Cell::new(0));
    for circular in [false, true] {
        let out = seen.clone();
        nav.select_next_with(circular, move |r| {
            let r = r.unwrap();
            assert_eq!(r.old_pages.selected_index, Some(0));
            assert_eq!(r.new_pages.selected_index, Some(0));
            out.set(out.get() + 1)
        });
        let out = seen.clone();
        nav.select_prev_with(circular, move |r| {
            assert_eq!(r.unwrap().new_pages.selected_index, Some(0));
            out.set(out.get() + 1)
        });
    }
    let out = seen.clone();
    nav.select_first_with(move |r| {
        assert!(r.is_ok());
        out.set(out.get() + 1)
    });
    let out = seen.clone();
    nav.select_last_with(move |r| {
        assert!(r.is_ok());
        out.set(out.get() + 1)
    });
    let out = seen.clone();
    nav.clear_with(move |r| {
        let r = r.unwrap();
        assert_eq!(r.old_pages.items.len(), 1);
        assert!(r.new_pages.items.is_empty());
        out.set(out.get() + 1)
    });
    assert_eq!(seen.get(), 7);
    assert_eq!(value.get().selected_index, None);
    nav.set_items_or_panic(|_| vec![Rc::new(9)]);
    assert_eq!(value.get().selected_index, Some(0));
}
#[test]
fn factories_are_forward_and_destruction_is_reverse_before_new_resume() {
    let parent = root();
    let nav = PagesNavigation::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    let output = log.clone();
    let options = PagesOptions {
        page_status: Some(Rc::new(|_, _| PageStatus::Resumed)),
        ..Default::default()
    };
    child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2], 0).unwrap(),
        options,
        move |c, ctx| {
            let c = *c;
            output.borrow_mut().push((c, "factory"));
            let out = output.clone();
            let _ = ctx
                .lifecycle()
                .do_on_destroy(move || out.borrow_mut().push((c, "destroy")))
                .detach();
            let out = output.clone();
            let _ = ctx
                .lifecycle()
                .do_on_resume(move || out.borrow_mut().push((c, "resume")))
                .detach();
            ctx
        },
    )
    .unwrap();
    assert_eq!(
        *log.borrow(),
        [
            (0, "factory"),
            (1, "factory"),
            (2, "factory"),
            (0, "resume"),
            (1, "resume"),
            (2, "resume")
        ]
    );
    log.borrow_mut().clear();
    nav.set_items_or_panic(|_| vec![Rc::new(3), Rc::new(4)]);
    assert_eq!(
        *log.borrow(),
        [
            (3, "factory"),
            (4, "factory"),
            (2, "destroy"),
            (1, "destroy"),
            (0, "destroy"),
            (3, "resume"),
            (4, "resume")
        ]
    );
}
#[test]
fn automatic_back_child_priority_predictive_cancel_and_fallback() {
    let parent = root();
    let nav = PagesNavigation::new();
    let options = PagesOptions {
        handle_back_button: true,
        ..Default::default()
    };
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2], 2).unwrap(),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    let current = value.get().items[2].instance().unwrap().clone();
    let log = Rc::new(RefCell::new(Vec::new()));
    let output = log.clone();
    let child = current
        .back_handler()
        .register(0, true, move |event, _| {
            output.borrow_mut().push(event.phase)
        })
        .unwrap();
    assert!(parent.back_handler().predictive_start());
    parent.back_handler().predictive_progress(0.4).unwrap();
    parent.back_handler().predictive_cancel().unwrap();
    assert_eq!(
        *log.borrow(),
        [
            BackPhase::Started,
            BackPhase::Progressed,
            BackPhase::Cancelled
        ]
    );
    assert_eq!(value.get().selected_index, Some(2));
    assert!(parent.back_handler().back());
    assert_eq!(value.get().selected_index, Some(2));
    current.back_handler().set_enabled(child.id(), false);
    assert!(parent.back_handler().predictive_start());
    parent.back_handler().predictive_cancel().unwrap();
    assert_eq!(value.get().selected_index, Some(2));
    assert!(parent.back_handler().back());
    assert_eq!(value.get().selected_index, Some(1));
    assert!(parent.back_handler().back());
    assert_eq!(value.get().selected_index, Some(0));
    assert!(!parent.back_handler().back());
}
#[test]
fn child_back_navigation_is_deferred_until_dispatch_finishes() {
    let parent = root();
    let nav = PagesNavigation::new();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1], 0).unwrap(),
        PagesOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let current = value.get().items[0].instance().unwrap().clone();
    let snapshot = value.clone();
    let source = nav.clone();
    let done = Rc::new(Cell::new(false));
    let output = done.clone();
    current
        .back_handler()
        .register(0, true, move |_, _| {
            let out = output.clone();
            source.select_with(1, move |r| {
                assert!(r.is_ok());
                out.set(true)
            });
            assert_eq!(snapshot.get().selected_index, Some(0));
            assert!(!output.get());
        })
        .unwrap();
    assert!(parent.back_handler().back());
    assert_eq!(value.get().selected_index, Some(1));
    assert!(done.get());
}
#[test]
fn broadcast_reports_independent_selection_and_completion_reentry_fifo() {
    let parent = root();
    let nav = PagesNavigation::new();
    let one = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2], 0).unwrap(),
        PagesOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let two = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 2).unwrap(),
        PagesOptions {
            key: "two".into(),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    let nested = nav.clone();
    nav.select_next_with(false, move |r| {
        let r = r.unwrap();
        output
            .borrow_mut()
            .push((r.old_pages.selected_index, r.new_pages.selected_index));
        if output.borrow().len() == 1 {
            let output = output.clone();
            nested.select_prev_with(false, move |r| {
                let r = r.unwrap();
                output
                    .borrow_mut()
                    .push((r.old_pages.selected_index, r.new_pages.selected_index));
            });
        }
    });
    assert_eq!(
        *seen.borrow(),
        [
            (Some(0), Some(1)),
            (Some(2), Some(3)),
            (Some(1), Some(0)),
            (Some(3), Some(2))
        ]
    );
    assert_eq!(one.get().selected_index, Some(0));
    assert_eq!(two.get().selected_index, Some(2));
}
#[test]
fn observer_factory_lifecycle_and_policy_reentry_are_fifo() {
    let parent = root();
    let nav = PagesNavigation::new();
    let source = nav.clone();
    let policy_source = nav.clone();
    let requested = Rc::new(Cell::new(false));
    let flag = requested.clone();
    let options = PagesOptions {
        page_status: Some(Rc::new(move |i, p: &Pages<u32>| {
            if p.selected_index == Some(1) && !flag.replace(true) {
                policy_source.select_or_panic(2);
            }
            default_page_status(i, p)
        })),
        ..Default::default()
    };
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        options,
        move |c, ctx| {
            if *c == 2 {
                let source = source.clone();
                let _ = ctx
                    .lifecycle()
                    .do_on_resume(move || source.select_or_panic(3))
                    .detach();
            }
            ctx
        },
    )
    .unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = seen.clone();
    let source = nav.clone();
    let _sub = value.subscribe(move |p| {
        out.borrow_mut().push(p.selected_index);
        if p.selected_index == Some(3) {
            source.select_or_panic(0);
        }
    });
    seen.borrow_mut().clear();
    nav.select_or_panic(1);
    assert_eq!(*seen.borrow(), [Some(1), Some(2), Some(3), Some(0)]);
    assert_eq!(value.get().selected_index, Some(0));
}
#[test]
fn process_death_restores_selection_keys_and_destroyed_page_state() {
    let parent = root();
    let nav = PagesNavigation::new();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    value.get().items[0]
        .instance()
        .unwrap()
        .state_store()
        .register("counter", || vec![19])
        .unwrap();
    let key = value.get().items[0].key().to_owned();
    nav.select_or_panic(3);
    let saved = parent.state_store().save().unwrap();
    assert_eq!(&saved["DefaultChildPages"][..4], b"DPG1");
    parent.lifecycle().destroy().unwrap();
    let parent = restored_root(saved, None);
    let nav = PagesNavigation::new();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![99u32], 0).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(value.get().selected_index, Some(3));
    assert!(value.get().items[0].instance().is_none());
    assert_eq!(value.get().items[0].key(), key);
    nav.select_or_panic(0);
    assert_eq!(
        value.get().items[0]
            .instance()
            .unwrap()
            .state_store()
            .consume_bytes("counter")
            .unwrap(),
        Some(vec![19])
    );
    nav.set_items_or_panic(|mut items| {
        items.push(Rc::new(4));
        items
    });
    assert_eq!(value.get().items[4].key(), "child/4");
}
#[test]
fn empty_restored_pages_override_nonempty_initial_and_transient_save_skips_providers() {
    let parent = root();
    let nav = PagesNavigation::<u32>::new();
    child_pages(&parent, &nav, Pages::default(), persistent(), |_, ctx| ctx).unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = restored_root(saved, None);
    let value = child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![9u32], 0).unwrap(),
        persistent(),
        |_, _| panic!("empty factory"),
    )
    .unwrap();
    assert!(value.get().items.is_empty());
    let parent = root();
    let value = child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![1u32], 0).unwrap(),
        PagesOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    value.get().items[0]
        .instance()
        .unwrap()
        .state_store()
        .register("transient", || panic!("transient save invoked"))
        .unwrap();
    assert!(
        !parent
            .state_store()
            .save()
            .unwrap()
            .contains_key("DefaultChildPages")
    );
}
#[test]
fn external_instances_retain_only_live_desired_scopes_and_transient_discards_them() {
    for persistent_mode in [true, false] {
        let instances = InstanceStore::from(InstanceKeeper::new());
        let parent = restored_root(Default::default(), Some(instances.clone()));
        let nav = PagesNavigation::new();
        let options = if persistent_mode {
            persistent()
        } else {
            PagesOptions::default()
        };
        let value = child_pages(
            &parent,
            &nav,
            Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
            options,
            |_, ctx| ctx,
        )
        .unwrap();
        let old = value.get().items[0].instance().unwrap().instance_store();
        let neighbour = value.get().items[1].instance().unwrap().instance_store();
        let saved = parent.state_store().save().unwrap();
        nav.select_or_panic(3);
        let far = value.get().items[3].instance().unwrap().instance_store();
        parent.lifecycle().destroy().unwrap();
        assert!(!far.is_destroyed().unwrap());
        let parent = restored_root(saved, Some(instances));
        let options = if persistent_mode {
            persistent()
        } else {
            PagesOptions::default()
        };
        let value = child_pages(
            &parent,
            &PagesNavigation::new(),
            Pages::new(vec![99u32], 0).unwrap(),
            options,
            |_, ctx| ctx,
        )
        .unwrap();
        assert!(far.is_destroyed().unwrap());
        assert!(old.is_destroyed().unwrap());
        assert!(neighbour.is_destroyed().unwrap());
        assert_eq!(
            *value.get().items[0].configuration(),
            Rc::new(if persistent_mode { 0 } else { 99 })
        );
    }
    let instances = InstanceStore::from(InstanceKeeper::new());
    let parent = restored_root(Default::default(), Some(instances.clone()));
    let nav = PagesNavigation::new();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let old = value.get().items[0].instance().unwrap().instance_store();
    let saved = parent.state_store().save().unwrap();
    parent.lifecycle().destroy().unwrap();
    let parent = restored_root(saved, Some(instances));
    let value = child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(old.ptr_eq(&value.get().items[0].instance().unwrap().instance_store()));
}

#[test]
fn malformed_dpg1_is_rejected_before_codec_factory_and_input_is_returned_for_retry() {
    let parent = root();
    child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![0u32, 1], 0).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let original = parent.state_store().save().unwrap()["DefaultChildPages"].clone();
    let mut malformed = Vec::new();
    for len in 0..original.len() {
        malformed.push(original[..len].to_vec());
    }
    for (range, data) in [
        (0..4, vec![0; 4]),
        (4..12, 0u64.to_le_bytes().to_vec()),
        (12..20, 9u64.to_le_bytes().to_vec()),
        (12..20, u64::MAX.to_le_bytes().to_vec()),
        (20..24, u32::MAX.to_le_bytes().to_vec()),
        (32..36, u32::MAX.to_le_bytes().to_vec()),
    ] {
        let mut bytes = original.clone();
        bytes[range].copy_from_slice(&data);
        malformed.push(bytes);
    }
    let mut bytes = original.clone();
    bytes.push(0);
    malformed.push(bytes);
    let key = original.windows(7).position(|w| w == b"child/0").unwrap();
    let mut bytes = original.clone();
    bytes[key] = 255;
    malformed.push(bytes);
    let mut bytes = original.clone();
    bytes[key + 6] = b'9';
    malformed.push(bytes);
    let second = original.windows(7).position(|w| w == b"child/1").unwrap();
    let mut bytes = original.clone();
    bytes[second + 6] = b'0';
    malformed.push(bytes);
    let nested = original.windows(4).position(|w| w == b"DCS1").unwrap();
    let mut bytes = original.clone();
    bytes[nested] = 0;
    malformed.push(bytes);
    for bytes in malformed {
        let saved = std::collections::BTreeMap::from([("DefaultChildPages".into(), bytes)]);
        let parent = restored_root(saved.clone(), None);
        for _ in 0..2 {
            let options = PagesOptions {
                codec: Some(PagesCodec::new(
                    |_: &[Rc<u32>]| Ok(vec![]),
                    |_| panic!("malformed codec"),
                )),
                ..Default::default()
            };
            assert!(
                child_pages(
                    &parent,
                    &PagesNavigation::new(),
                    Pages::default(),
                    options,
                    |_, _| panic!("malformed factory")
                )
                .is_err()
            );
            assert_eq!(parent.state_store().save().unwrap(), saved);
            assert_eq!(parent.state_store().provider_count().unwrap(), 0);
            assert_eq!(parent.back_handler().handler_count(), 0);
        }
    }
}
#[test]
fn invalid_empty_restored_selection_count_and_codec_errors_remain_retryable() {
    let parent = root();
    child_pages(
        &parent,
        &PagesNavigation::<u32>::new(),
        Pages::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let mut saved = parent.state_store().save().unwrap();
    saved.get_mut("DefaultChildPages").unwrap()[12..20].copy_from_slice(&0u64.to_le_bytes());
    let parent = restored_root(saved.clone(), None);
    assert!(matches!(
        child_pages(
            &parent,
            &PagesNavigation::new(),
            Pages::default(),
            persistent(),
            |_, ctx| ctx
        ),
        Err(NavigationError::InvalidSelection { .. })
    ));
    assert_eq!(parent.state_store().save().unwrap(), saved);
    let parent = root();
    child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![1u32], 0).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = parent.state_store().save().unwrap();
    for decode_failure in [true, false] {
        let parent = restored_root(saved.clone(), None);
        let options = PagesOptions {
            codec: Some(PagesCodec::new(
                |_: &[Rc<u32>]| Ok(vec![]),
                move |_| {
                    if decode_failure {
                        Err("decode failed".into())
                    } else {
                        Ok(vec![])
                    }
                },
            )),
            ..Default::default()
        };
        assert!(matches!(
            child_pages(
                &parent,
                &PagesNavigation::new(),
                Pages::default(),
                options,
                |_, _| panic!("codec failure factory")
            ),
            Err(NavigationError::State(_))
        ));
        assert_eq!(parent.state_store().save().unwrap(), saved);
    }
    let parent = root();
    let options = PagesOptions {
        codec: Some(PagesCodec::new(
            |_: &[Rc<u32>]| Err("encode failed".into()),
            |_| Ok(vec![1u32]),
        )),
        ..Default::default()
    };
    child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![1u32], 0).unwrap(),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(matches!(
        parent.state_store().save(),
        Err(decompose_rs::StateAccessError::Keeper(
            StateKeeperError::Encode { .. }
        ))
    ));
}
#[test]
fn duplicate_restored_configurations_reject_before_factory_and_allow_opt_in_retry() {
    let parent = root();
    let options = PagesOptions {
        allow_duplicates: true,
        ..persistent()
    };
    child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![1u32, 1], 0).unwrap(),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = restored_root(saved.clone(), None);
    assert!(matches!(
        child_pages(
            &parent,
            &PagesNavigation::new(),
            Pages::default(),
            persistent(),
            |_, _| panic!("duplicate factory")
        ),
        Err(NavigationError::DuplicateConfiguration)
    ));
    assert_eq!(parent.state_store().save().unwrap(), saved);
    let value = child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::default(),
        PagesOptions {
            allow_duplicates: true,
            ..persistent()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(value.get().items.len(), 2);
}
#[test]
fn state_provider_is_reserved_before_codec_status_and_factory_callbacks() {
    let parent = root();
    child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![1u32], 0).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = restored_root(saved, None);
    let called = Rc::new(Cell::new(0));
    let probe = Rc::new({
        let parent = parent.clone();
        let called = called.clone();
        move || {
            let result = child_pages(
                &parent,
                &PagesNavigation::new(),
                Pages::new(vec![9u32], 0).unwrap(),
                PagesOptions::default(),
                |_, _| panic!("nested factory"),
            );
            assert!(matches!(
                result,
                Err(NavigationError::Context(
                    DecomposeError::DuplicateKey { .. }
                ))
            ));
            called.set(called.get() + 1);
        }
    });
    let decode = probe.clone();
    let status = probe.clone();
    let factory = probe.clone();
    let options = PagesOptions {
        codec: Some(PagesCodec::new(
            |_: &[Rc<u32>]| Ok(vec![]),
            move |_| {
                decode();
                Ok(vec![1u32])
            },
        )),
        page_status: Some(Rc::new(move |_, _| {
            status();
            PageStatus::Resumed
        })),
        ..Default::default()
    };
    child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::default(),
        options,
        move |_, ctx| {
            factory();
            ctx
        },
    )
    .unwrap();
    assert_eq!(called.get(), 3);
}
#[test]
fn retained_type_conflict_returns_input_and_does_not_run_factory() {
    let parent = root();
    child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![1u32], 0).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = restored_root(saved.clone(), None);
    parent
        .instance_store()
        .get_or_create("DefaultChildPages", || 17u32)
        .unwrap();
    assert!(matches!(
        child_pages(
            &parent,
            &PagesNavigation::new(),
            Pages::default(),
            persistent(),
            |_, _| panic!("conflict factory")
        ),
        Err(NavigationError::Context(_))
    ));
    assert_eq!(parent.state_store().save().unwrap(), saved);
}
#[test]
fn destroyed_status_policy_on_restore_clears_previously_retained_live_scope() {
    let instances = InstanceStore::from(InstanceKeeper::new());
    let parent = restored_root(Default::default(), Some(instances.clone()));
    let value = child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![1u32], 0).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let old = value.get().items[0].instance().unwrap().instance_store();
    let saved = parent.state_store().save().unwrap();
    parent.lifecycle().destroy().unwrap();
    assert!(!old.is_destroyed().unwrap());
    let parent = restored_root(saved, Some(instances));
    let options = PagesOptions {
        page_status: Some(Rc::new(|_, _| PageStatus::Destroyed)),
        ..persistent()
    };
    let value = child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::default(),
        options,
        |_, _| panic!("destroyed factory"),
    )
    .unwrap();
    assert!(old.is_destroyed().unwrap());
    assert!(value.get().items[0].instance().is_none());
}
#[test]
fn failed_child_state_save_leaves_selection_all_old_lifecycles_and_factories_untouched() {
    let parent = root();
    let nav = PagesNavigation::new();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        PagesOptions::default(),
        move |_, ctx| {
            count.set(count.get() + 1);
            ctx
        },
    )
    .unwrap();
    let before = value.get();
    let old = before.items[0].instance().unwrap().clone();
    decompose_rs::router::slot::child_slot(
        &*old,
        &decompose_rs::router::slot::SlotNavigation::new(),
        Some(9u32),
        decompose_rs::router::slot::SlotOptions {
            key: "fail".into(),
            codec: Some(decompose_rs::router::slot::SlotCodec::new(
                |_: &u32| Err("provider failure".into()),
                |_| Ok(9),
            )),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let error = Rc::new(RefCell::new(None));
    let out = error.clone();
    nav.select_with(3, move |r| *out.borrow_mut() = r.err());
    assert!(matches!(*error.borrow(), Some(NavigationError::State(_))));
    assert_eq!(calls.get(), 2);
    assert_eq!(value.get().selected_index, Some(0));
    assert_eq!(old.lifecycle().state(), LifecycleState::Resumed);
    assert!(!old.instance_store().is_destroyed().unwrap());
    old.state_store().unregister("fail").unwrap();
    nav.select_or_panic(3);
    assert_eq!(value.get().selected_index, Some(3));
}
#[test]
fn child_state_save_borrow_failure_is_typed_and_retries() {
    let parent = root();
    let nav = PagesNavigation::new();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        PagesOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let error = Rc::new(RefCell::new(None));
    let out = error.clone();
    nav.select_with(3, move |r| *out.borrow_mut() = r.err());
    assert!(error.borrow().is_none());
    assert_eq!(value.get().selected_index, Some(3));
}
#[test]
fn factory_panic_cleans_prepared_scopes_and_source_can_retry() {
    let parent = root();
    let nav = PagesNavigation::new();
    let should_panic = Rc::new(Cell::new(true));
    let flag = should_panic.clone();
    let prepared = Rc::new(RefCell::new(Vec::new()));
    let out = prepared.clone();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        PagesOptions::default(),
        move |c, ctx| {
            if *c >= 2 {
                out.borrow_mut()
                    .push((ctx.lifecycle(), ctx.instance_store()));
                if *c == 3 && flag.replace(false) {
                    panic!("factory panic")
                }
            }
            ctx
        },
    )
    .unwrap();
    let before = value.get();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| nav.select_or_panic(3))).is_err()
    );
    assert_eq!(value.get().selected_index, Some(0));
    assert_eq!(
        before.items[0].instance().unwrap().lifecycle().state(),
        LifecycleState::Resumed
    );
    for (lifecycle, instances) in prepared.borrow().iter() {
        assert!(lifecycle.state().is_destroyed());
        assert!(instances.is_destroyed().unwrap());
    }
    nav.select_or_panic(3);
    assert_eq!(value.get().selected_index, Some(3));
    assert_eq!(
        value.get().items[3].instance().unwrap().lifecycle().state(),
        LifecycleState::Resumed
    );
}
#[test]
fn status_callback_panic_releases_constructor_input_and_navigation_remains_usable() {
    let parent = root();
    child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![1u32], 0).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = restored_root(saved.clone(), None);
    let options = PagesOptions {
        page_status: Some(Rc::new(|_, _| panic!("policy panic"))),
        ..persistent()
    };
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| child_pages(
            &parent,
            &PagesNavigation::new(),
            Pages::default(),
            options,
            |_, _| panic!("policy factory")
        )))
        .is_err()
    );
    assert_eq!(parent.state_store().save().unwrap(), saved);
    assert_eq!(parent.back_handler().handler_count(), 0);
    let value = child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(value.get().selected_index, Some(0));
    let parent = root();
    let nav = PagesNavigation::new();
    let flag = Rc::new(Cell::new(true));
    let panic_flag = flag.clone();
    let options = PagesOptions {
        page_status: Some(Rc::new(move |i, p: &Pages<u32>| {
            if p.selected_index == Some(1) && panic_flag.replace(false) {
                panic!("transition policy panic")
            }
            default_page_status(i, p)
        })),
        ..Default::default()
    };
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1], 0).unwrap(),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| nav.select_or_panic(1))).is_err()
    );
    assert_eq!(value.get().selected_index, Some(0));
    nav.select_or_panic(1);
    assert_eq!(value.get().selected_index, Some(1));
}
fn retry_after_lifecycle_panic(outgoing: bool) {
    let parent = root();
    let nav = PagesNavigation::new();
    let captured = Rc::new(RefCell::new(Vec::new()));
    let output = captured.clone();
    let flag = Rc::new(Cell::new(true));
    let panicker = flag.clone();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        PagesOptions::default(),
        move |c, ctx| {
            if *c == 0 && outgoing {
                let flag = panicker.clone();
                let _ = ctx
                    .lifecycle()
                    .do_on_destroy(move || {
                        if flag.replace(false) {
                            panic!("outgoing destroy panic")
                        }
                    })
                    .detach();
            }
            if *c == 3 {
                output.borrow_mut().push(ctx.instance_store());
                if !outgoing {
                    let flag = panicker.clone();
                    let _ = ctx
                        .lifecycle()
                        .do_on_resume(move || {
                            if flag.replace(false) {
                                panic!("incoming resume panic")
                            }
                        })
                        .detach();
                }
            }
            ctx
        },
    )
    .unwrap();
    let old = value.get().items[0].clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| nav.select_or_panic(3))).is_err()
    );
    assert_eq!(value.get().selected_index, Some(0));
    assert!(old.instance().unwrap().lifecycle().state().is_destroyed());
    assert!(captured.borrow()[0].is_destroyed().unwrap());
    nav.select_or_panic(0);
    let retried = value.get().items[0].clone();
    assert!(!Rc::ptr_eq(
        old.instance().unwrap(),
        retried.instance().unwrap()
    ));
    assert_ne!(old.key(), retried.key());
    assert_eq!(
        retried.instance().unwrap().lifecycle().state(),
        LifecycleState::Resumed
    );
    assert!(
        !retried
            .instance()
            .unwrap()
            .instance_store()
            .is_destroyed()
            .unwrap()
    );
    assert!(
        old.instance()
            .unwrap()
            .instance_store()
            .is_destroyed()
            .unwrap()
    );
}
#[test]
fn retry_after_outgoing_destroy_panic_never_reuses_an_ended_live_scope() {
    retry_after_lifecycle_panic(true);
}
#[test]
fn retry_after_incoming_resume_panic_never_reuses_an_ended_live_scope() {
    retry_after_lifecycle_panic(false);
}
#[test]
fn successful_tombstones_keep_identity_after_a_later_failed_navigation() {
    let parent = root();
    let nav = PagesNavigation::new();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        PagesOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let key = value.get().items[0].key().to_owned();
    nav.select_or_panic(3);
    let out = Rc::new(Cell::new(false));
    let done = out.clone();
    nav.select_with(10, move |r| done.set(r.is_err()));
    assert!(out.get());
    nav.select_or_panic(0);
    assert_eq!(value.get().items[0].key(), key);
}
#[test]
fn exhausted_key_counter_reports_error_without_running_factory() {
    let parent = root();
    child_pages(
        &parent,
        &PagesNavigation::<u32>::new(),
        Pages::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let mut saved = parent.state_store().save().unwrap();
    saved.get_mut("DefaultChildPages").unwrap()[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    let parent = restored_root(saved, None);
    let nav = PagesNavigation::new();
    let value = child_pages(&parent, &nav, Pages::default(), persistent(), |_, _| {
        panic!("exhausted factory")
    })
    .unwrap();
    let error = Rc::new(RefCell::new(None));
    let out = error.clone();
    nav.set_items_with(
        |_| vec![Rc::new(1u32)],
        move |r| *out.borrow_mut() = r.err(),
    );
    assert!(matches!(*error.borrow(), Some(NavigationError::State(_))));
    assert!(value.get().items.is_empty());
}
#[test]
fn save_captures_coherent_membership_selection_keys_counter_before_reentrant_providers() {
    let parent = root();
    let nav = PagesNavigation::new();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1], 1).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let source = nav.clone();
    let once = Rc::new(Cell::new(true));
    let flag = once.clone();
    value.get().items[0]
        .instance()
        .unwrap()
        .state_store()
        .register("navigate", move || {
            if flag.replace(false) {
                source.clear_or_panic();
            }
            vec![3]
        })
        .unwrap();
    let saved = parent.state_store().save().unwrap();
    assert!(value.get().items.is_empty());
    let parent = restored_root(saved, None);
    let nav = PagesNavigation::new();
    let value = child_pages(&parent, &nav, Pages::default(), persistent(), |_, ctx| ctx).unwrap();
    assert_eq!(value.get().selected_index, Some(1));
    assert_eq!(value.get().items[0].key(), "child/0");
    assert_eq!(value.get().items[1].key(), "child/1");
    nav.set_items_or_panic(|mut items| {
        items.push(Rc::new(2));
        items
    });
    assert_eq!(value.get().items[2].key(), "child/2");
}
#[test]
fn parent_destroy_during_factory_returns_typed_error_and_initial_input_rolls_back() {
    let parent = root();
    child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![1u32], 0).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = restored_root(saved.clone(), None);
    let lifecycle = parent.lifecycle();
    assert!(matches!(
        child_pages(
            &parent,
            &PagesNavigation::new(),
            Pages::default(),
            persistent(),
            move |_, ctx| {
                lifecycle.destroy().unwrap();
                ctx
            }
        ),
        Err(NavigationError::Destroyed)
    ));
    assert_eq!(parent.state_store().save().unwrap(), saved);
    assert_eq!(parent.back_handler().handler_count(), 0);
}

#[test]
fn non_clone_configuration_component_and_all_public_snapshots_support_manual_clone() {
    #[derive(PartialEq, Eq)]
    struct Config(u32);
    struct Component(DefaultComponentContext);
    let parent = root();
    let nav = PagesNavigation::<Config>::default();
    let _nav_copy = nav.clone();
    let pages = Pages::new(vec![Config(0), Config(1), Config(2)], 0).unwrap();
    let _pages_copy = pages.clone();
    let value = child_pages(&parent, &nav, pages, PagesOptions::default(), |_, ctx| {
        Component(ctx)
    })
    .unwrap();
    let children = value.get();
    let _copy = children.clone();
    let _child_copy = children.items[0].clone();
    let empty = ChildPages::<Config, Component>::default();
    assert!(empty.items.is_empty());
    let result = Rc::new(RefCell::new(None));
    let output = result.clone();
    nav.select_with(2, move |r| *output.borrow_mut() = Some(r.unwrap().clone()));
    assert_eq!(
        result.borrow().as_ref().unwrap().new_pages.selected_index,
        Some(2)
    );
    assert_eq!(
        value
            .get()
            .selected()
            .unwrap()
            .instance()
            .unwrap()
            .0
            .lifecycle()
            .state(),
        LifecycleState::Resumed
    );
    let codec = PagesCodec::<Config>::new(|_| Ok(vec![]), |_| Ok(vec![]));
    let _copy = codec.clone();
}
#[test]
fn parent_bounds_each_live_status_and_destroy_ends_all_old_snapshot_handles() {
    let lifecycle = LifecycleRegistry::new();
    let parent = DefaultComponentContext::new(lifecycle.clone());
    let nav = PagesNavigation::new();
    let options = PagesOptions {
        page_status: Some(Rc::new(|i, _| match i {
            0 => PageStatus::Created,
            1 => PageStatus::Started,
            2 => PageStatus::Resumed,
            _ => PageStatus::Destroyed,
        })),
        ..Default::default()
    };
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 2).unwrap(),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    let before = value.get();
    for child in &before.items[..3] {
        assert_eq!(
            child.instance().unwrap().lifecycle().state(),
            LifecycleState::Initialized
        );
    }
    lifecycle.move_to(LifecycleState::Created).unwrap();
    for child in &before.items[..3] {
        assert_eq!(
            child.instance().unwrap().lifecycle().state(),
            LifecycleState::Created
        );
    }
    lifecycle.move_to(LifecycleState::Started).unwrap();
    assert_eq!(
        before.items[0].instance().unwrap().lifecycle().state(),
        LifecycleState::Created
    );
    assert_eq!(
        before.items[1].instance().unwrap().lifecycle().state(),
        LifecycleState::Started
    );
    assert_eq!(
        before.items[2].instance().unwrap().lifecycle().state(),
        LifecycleState::Started
    );
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    assert_eq!(
        before.items[2].instance().unwrap().lifecycle().state(),
        LifecycleState::Resumed
    );
    lifecycle.destroy().unwrap();
    for child in &before.items[..3] {
        assert!(child.instance().unwrap().lifecycle().state().is_destroyed());
        assert!(
            child
                .instance()
                .unwrap()
                .instance_store()
                .is_destroyed()
                .unwrap()
        );
    }
    assert_eq!(parent.back_handler().handler_count(), 0);
    nav.clear_or_panic();
    assert_eq!(value.get().items.len(), 4);
}
#[test]
fn outgoing_pause_stop_and_destroy_finish_before_new_selected_resume() {
    let parent = root();
    let nav = PagesNavigation::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    let output = log.clone();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        PagesOptions::default(),
        move |c, ctx| {
            let c = *c;
            let out = output.clone();
            let _ = ctx
                .lifecycle()
                .subscribe(move |s| out.borrow_mut().push((c, s)))
                .detach();
            ctx
        },
    )
    .unwrap();
    log.borrow_mut().clear();
    nav.select_or_panic(3);
    let log = log.borrow();
    let incoming = log
        .iter()
        .position(|e| *e == (3, LifecycleState::Resumed))
        .unwrap();
    for state in [
        LifecycleState::Started,
        LifecycleState::Created,
        LifecycleState::Destroyed,
    ] {
        assert!(log.iter().position(|e| *e == (0, state)).unwrap() < incoming);
    }
    assert_eq!(value.get().selected_index, Some(3));
}
#[test]
fn factory_navigation_during_initial_construction_is_queued_and_published_after_initial_state() {
    let parent = root();
    let nav = PagesNavigation::new();
    let source = nav.clone();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        PagesOptions::default(),
        move |c, ctx| {
            count.set(count.get() + 1);
            if *c == 0 {
                source.select_or_panic(3);
            }
            ctx
        },
    )
    .unwrap();
    assert_eq!(value.get().selected_index, Some(3));
    assert_eq!(calls.get(), 4);
}
#[test]
fn completion_and_observer_panics_drop_nested_events_and_navigation_can_retry() {
    for observer_panics in [false, true] {
        let parent = root();
        let nav = PagesNavigation::new();
        let value = child_pages(
            &parent,
            &nav,
            Pages::new(vec![0u32, 1, 2], 0).unwrap(),
            PagesOptions::default(),
            |_, ctx| ctx,
        )
        .unwrap();
        let source = nav.clone();
        let flag = Rc::new(Cell::new(true));
        let panic_flag = flag.clone();
        let _subscription = value.subscribe(move |p| {
            if observer_panics && p.selected_index == Some(1) && panic_flag.replace(false) {
                source.select_or_panic(2);
                panic!("observer panic");
            }
        });
        let source = nav.clone();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| nav.select_with(
                1,
                move |_| {
                    if !observer_panics {
                        source.select_or_panic(2);
                        panic!("completion panic")
                    }
                }
            )))
            .is_err()
        );
        assert_eq!(value.get().selected_index, Some(1));
        nav.select_or_panic(0);
        assert_eq!(value.get().selected_index, Some(0));
    }
}
#[test]
fn nested_mixed_routers_restore_through_pages_tombstone_and_process_death() {
    use decompose_rs::router::{slot::*, stack::*};
    struct Nested {
        context: DefaultComponentContext,
        slot_nav: SlotNavigation<u32>,
        slot: decompose_rs::value::Value<ChildSlot<u32, DefaultComponentContext>>,
        stack: decompose_rs::value::Value<ChildStack<u32, DefaultComponentContext>>,
    }
    fn build(_: &u32, context: DefaultComponentContext) -> Nested {
        let slot_nav = SlotNavigation::new();
        let slot = child_slot(
            &context,
            &slot_nav,
            Some(10u32),
            SlotOptions {
                codec: Some(SlotCodec::new(
                    |c: &u32| Ok(c.to_le_bytes().to_vec()),
                    |b| Ok(u32::from_le_bytes(b.try_into().map_err(|_| "bad slot")?)),
                )),
                ..Default::default()
            },
            |_, ctx| ctx,
        )
        .unwrap();
        let stack = child_stack(
            &context,
            &StackNavigation::new(),
            vec![20u32, 21],
            StackOptions {
                codec: Some(StackCodec::new(
                    |items: &[Rc<u32>]| Ok(items.iter().flat_map(|c| c.to_le_bytes()).collect()),
                    |b| {
                        Ok(b.chunks_exact(4)
                            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                            .collect())
                    },
                )),
                ..Default::default()
            },
            |_, ctx| ctx,
        )
        .unwrap();
        Nested {
            context,
            slot_nav,
            slot,
            stack,
        }
    }
    let parent = root();
    let nav = PagesNavigation::new();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32, 1, 2, 3], 0).unwrap(),
        persistent(),
        build,
    )
    .unwrap();
    let before = value.get().items[0].instance().unwrap().clone();
    before.slot_nav.activate_or_panic(99);
    let key = value.get().items[0].key().to_owned();
    nav.select_or_panic(3);
    assert!(before.context.lifecycle().state().is_destroyed());
    let saved = parent.state_store().save().unwrap();
    parent.lifecycle().destroy().unwrap();
    let parent = restored_root(saved, None);
    let nav = PagesNavigation::new();
    let value = child_pages(&parent, &nav, Pages::default(), persistent(), build).unwrap();
    nav.select_or_panic(0);
    let after = value.get().items[0].instance().unwrap().clone();
    assert_eq!(value.get().items[0].key(), key);
    assert_eq!(*after.slot.get().child.as_ref().unwrap().configuration, 99);
    assert_eq!(*after.stack.get().active.configuration, 21);
    assert_eq!(
        after
            .slot
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
fn dropping_view_keeps_engine_alive_until_parent_end_without_rc_cycle() {
    struct Component {
        context: DefaultComponentContext,
        capture: Rc<()>,
    }
    let parent = root();
    let nav = PagesNavigation::new();
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let out = capture.clone();
    let value = child_pages(
        &parent,
        &nav,
        Pages::new(vec![0u32], 0).unwrap(),
        PagesOptions::default(),
        move |_, ctx| Component {
            context: ctx,
            capture: out.clone(),
        },
    )
    .unwrap();
    drop(capture);
    let component = value.get().items[0].instance().unwrap().clone();
    assert_eq!(Rc::strong_count(&component.capture), 2);
    assert_eq!(
        component.context.lifecycle().state(),
        LifecycleState::Resumed
    );
    drop(component);
    drop(value);
    nav.set_items_or_panic(|_| vec![Rc::new(1)]);
    assert!(weak.upgrade().is_some());
    parent.lifecycle().destroy().unwrap();
    drop(parent);
    drop(nav);
    assert!(weak.upgrade().is_none());
}
#[test]
fn initial_factory_panic_returns_restored_input_and_destroys_fresh_prepared_keepers() {
    let parent = root();
    child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::new(vec![0u32, 1], 0).unwrap(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = restored_root(saved.clone(), None);
    let captured = Rc::new(RefCell::new(Vec::new()));
    let output = captured.clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| child_pages(
            &parent,
            &PagesNavigation::new(),
            Pages::default(),
            persistent(),
            move |c, ctx| {
                output
                    .borrow_mut()
                    .push((ctx.lifecycle(), ctx.instance_store()));
                if *c == 1 {
                    panic!("initial factory panic")
                }
                ctx
            }
        )))
        .is_err()
    );
    for (l, i) in captured.borrow().iter() {
        assert!(l.state().is_destroyed());
        assert!(i.is_destroyed().unwrap());
    }
    assert_eq!(parent.state_store().save().unwrap(), saved);
    let value = child_pages(
        &parent,
        &PagesNavigation::new(),
        Pages::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(value.get().items.len(), 2);
}
