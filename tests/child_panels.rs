use decompose_rs::router::NavigationError;
use decompose_rs::router::panels::*;
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
fn initial(details: bool, extra: bool, mode: ChildPanelsMode) -> Panels<u32, u32, u32> {
    Panels {
        main: Rc::new(1),
        details: details.then(|| Rc::new(1)),
        extra: extra.then(|| Rc::new(1)),
        mode,
    }
}

type ModeCase = (
    ChildPanelsMode,
    bool,
    bool,
    LifecycleState,
    Option<LifecycleState>,
    Option<LifecycleState>,
);
fn mode_cases() -> [ModeCase; 12] {
    use ChildPanelsMode::{Dual, Single, Triple};
    use LifecycleState::{Created, Resumed};
    [
        (Single, false, false, Resumed, None, None),
        (Single, false, true, Created, None, Some(Resumed)),
        (Single, true, false, Created, Some(Resumed), None),
        (Single, true, true, Created, Some(Created), Some(Resumed)),
        (Dual, false, false, Resumed, None, None),
        (Dual, false, true, Resumed, None, Some(Resumed)),
        (Dual, true, false, Resumed, Some(Resumed), None),
        (Dual, true, true, Resumed, Some(Created), Some(Resumed)),
        (Triple, false, false, Resumed, None, None),
        (Triple, false, true, Resumed, None, Some(Resumed)),
        (Triple, true, false, Resumed, Some(Resumed), None),
        (Triple, true, true, Resumed, Some(Resumed), Some(Resumed)),
    ]
}
#[test]
fn all_mode_and_presence_cases_keep_every_panel_live_with_expected_lifecycle() {
    for (mode, details, extra, main_status, details_status, extra_status) in mode_cases() {
        let parent = root();
        let value = child_panels(
            &parent,
            &PanelsNavigation::new(),
            initial(details, extra, mode),
            PanelsOptions::default(),
            |_, c| c,
            |_, c| c,
            |_, c| c,
        )
        .unwrap();
        let p = value.get();
        assert_eq!(p.main.instance.lifecycle().state(), main_status);
        assert_eq!(
            p.details.as_ref().map(|c| c.instance.lifecycle().state()),
            details_status
        );
        assert_eq!(
            p.extra.as_ref().map(|c| c.instance.lifecycle().state()),
            extra_status
        );
        assert_eq!(p.details.is_some(), details);
        assert_eq!(p.extra.is_some(), extra);
        assert_eq!(p.mode, mode);
        if let Some(d) = &p.details {
            assert_ne!(d.key, p.main.key);
        }
        if let Some(e) = &p.extra {
            assert_ne!(e.key, p.main.key);
        }
    }
}

#[test]
fn mode_transitions_and_equal_operations_preserve_typed_instances_and_keys() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let value = child_panels(
        &parent,
        &nav,
        initial(true, true, ChildPanelsMode::Single),
        PanelsOptions::default(),
        |_, c| c,
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    let before = value.get();
    for mode in [
        ChildPanelsMode::Triple,
        ChildPanelsMode::Dual,
        ChildPanelsMode::Single,
    ] {
        nav.set_mode(mode);
        nav.set_panels(1, Some(1), Some(1));
        let after = value.get();
        assert!(Rc::ptr_eq(&before.main.instance, &after.main.instance));
        assert!(Rc::ptr_eq(
            &before.details.as_ref().unwrap().instance,
            &after.details.as_ref().unwrap().instance
        ));
        assert!(Rc::ptr_eq(
            &before.extra.as_ref().unwrap().instance,
            &after.extra.as_ref().unwrap().instance
        ));
        assert_eq!(before.main.key, after.main.key);
    }
    nav.dismiss_details();
    assert!(value.get().extra.is_some());
    nav.activate_main(2);
    assert!(value.get().extra.is_some());
    assert!(before.main.instance.lifecycle().state().is_destroyed());
    nav.pop();
    assert!(value.get().extra.is_none());
    nav.pop();
    assert_eq!(*value.get().main.configuration, 2);
}

#[test]
fn distinct_nonclone_configuration_and_component_types_and_two_role_adapter() {
    #[derive(PartialEq, Eq)]
    struct Main(u8);
    #[derive(PartialEq, Eq)]
    struct Details(&'static str);
    #[derive(PartialEq, Eq)]
    struct Extra(bool);
    struct MainComponent(u8);
    struct DetailsComponent(usize);
    struct ExtraComponent(bool);
    let parent = root();
    let nav = PanelsNavigation::new();
    let p = Panels {
        main: Rc::new(Main(1)),
        details: Some(Rc::new(Details("foo"))),
        extra: Some(Rc::new(Extra(true))),
        mode: ChildPanelsMode::Triple,
    };
    let value = child_panels(
        &parent,
        &nav,
        p,
        PanelsOptions::default(),
        |m, _| MainComponent(m.0),
        |d, _| DetailsComponent(d.0.len()),
        |e, _| ExtraComponent(e.0),
    )
    .unwrap();
    let _copy = value.get().as_ref().clone();
    let _nav = nav.clone();
    assert_eq!(value.get().main.instance.0, 1);
    assert_eq!(value.get().details.as_ref().unwrap().instance.0, 3);
    assert!(value.get().extra.as_ref().unwrap().instance.0);
    let parent = root();
    let nav = PanelsNavigation::<Main, Details>::new();
    let value = child_panels_without_extra(
        &parent,
        &nav,
        Panels::new(Main(4)),
        PanelsOptions::default(),
        |m, _| MainComponent(m.0),
        |d, _| DetailsComponent(d.0.len()),
    )
    .unwrap();
    nav.activate_details(Details("x"));
    assert_eq!(value.get().details.as_ref().unwrap().instance.0, 1);
    assert!(value.get().extra.is_none());
}

#[test]
fn every_navigation_helper_completes_with_actual_receiver_state() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let value = child_panels(
        &parent,
        &nav,
        initial(false, false, ChildPanelsMode::Single),
        PanelsOptions::default(),
        |_, c| c,
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let completion = || {
        let seen = seen.clone();
        move |r: Result<NavigationResult<u32, u32, u32>, _>| {
            let r = r.unwrap();
            seen.borrow_mut().push((
                *r.old_panels.main,
                *r.new_panels.main,
                r.new_panels.details.as_deref().copied(),
                r.new_panels.extra.as_deref().copied(),
                r.new_panels.mode,
            ));
        }
    };
    nav.set_panels_with(2, Some(3), Some(4), completion());
    nav.set_details_extra_with(Some(5), Some(6), completion());
    nav.set_extra_with(Some(7), completion());
    nav.activate_main_with(8, completion());
    nav.activate_details_with(9, completion());
    nav.activate_extra_with(10, completion());
    nav.dismiss_details_with(completion());
    nav.dismiss_extra_with(completion());
    nav.pop_with(completion());
    nav.set_mode_with(ChildPanelsMode::Dual, completion());
    nav.navigate_with(|p| p, completion());
    assert_eq!(seen.borrow().len(), 11);
    assert_eq!(seen.borrow()[0].0, 1);
    assert_eq!(seen.borrow()[0].1, 2);
    assert_eq!(value.get().mode, ChildPanelsMode::Dual);
    nav.set_details_extra(Some(11), Some(12));
    nav.set_extra(None);
    nav.activate_details(13);
    nav.activate_extra(14);
    nav.dismiss_extra();
    nav.pop();
    assert!(value.get().details.is_none());
    assert!(value.get().extra.is_none());
}

#[test]
fn initial_hook_has_registered_provider_and_back_and_navigation_hooks_precede_completion() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let order = Rc::new(RefCell::new(Vec::new()));
    let out = order.clone();
    let source = nav.clone();
    let back = parent.back_handler();
    let keeper = parent.state_keeper();
    let options = PanelsOptions {
        handle_back_button: true,
        on_state_changed: Some(Rc::new(move |p: &Panels<u32, u32, u32>, old| {
            out.borrow_mut()
                .push(if old.is_none() { "initial" } else { "hook" });
            assert!(keeper.borrow().is_registered("DefaultChildPanels"));
            assert!(keeper.borrow().save().unwrap().is_empty());
            if old.is_none() {
                assert!(back.back());
                source.activate_details(3);
            } else {
                assert!(p.extra.is_none());
            }
        })),
        ..Default::default()
    };
    let value = child_panels(
        &parent,
        &nav,
        initial(true, true, ChildPanelsMode::Single),
        options,
        |_, c| c,
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    assert_eq!(*order.borrow(), ["initial", "hook", "hook"]);
    assert_eq!(*value.get().details.as_ref().unwrap().configuration, 3);
    let out = order.clone();
    let _token = value.subscribe(move |_| out.borrow_mut().push("publish"));
    order.borrow_mut().clear();
    let out = order.clone();
    nav.navigate_with(|p| p, move |_| out.borrow_mut().push("complete"));
    assert_eq!(*order.borrow(), ["publish", "hook", "complete"]);
}

#[test]
fn auto_back_only_in_single_pops_extra_first_and_child_back_has_priority() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let value = child_panels(
        &parent,
        &nav,
        initial(true, true, ChildPanelsMode::Dual),
        PanelsOptions {
            handle_back_button: true,
            ..Default::default()
        },
        |_, c| c,
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    let hits = Rc::new(Cell::new(0));
    let out = hits.clone();
    parent
        .back_handler()
        .register(-1, true, move |_, _| out.set(out.get() + 1))
        .unwrap();
    assert!(parent.back_handler().back());
    assert_eq!(hits.get(), 1);
    assert!(value.get().extra.is_some());
    nav.set_mode(ChildPanelsMode::Single);
    let child = value.get().extra.as_ref().unwrap().instance.back_handler();
    let out = hits.clone();
    let handle = child
        .register(0, true, move |_, _| out.set(out.get() + 10))
        .unwrap();
    parent.back_handler().back();
    assert_eq!(hits.get(), 11);
    assert!(value.get().extra.is_some());
    child.unregister(handle.id());
    parent.back_handler().back();
    assert!(value.get().extra.is_none());
    assert!(value.get().details.is_some());
    parent.back_handler().back();
    assert!(value.get().details.is_none());
    parent.back_handler().back();
    assert_eq!(hits.get(), 12);
}

fn codec() -> PanelsCodec<u32, u32, u32> {
    PanelsCodec::new(
        |m: &u32, d: Option<&u32>, e: Option<&u32>| {
            let mut b = m.to_le_bytes().to_vec();
            for c in [d, e].into_iter().flatten() {
                b.extend_from_slice(&c.to_le_bytes());
            }
            Ok(b)
        },
        |b| match b.len() {
            4 => Ok((u32::from_le_bytes(b.try_into().unwrap()), None, None)),
            12 => Ok((
                u32::from_le_bytes(b[..4].try_into().unwrap()),
                Some(u32::from_le_bytes(b[4..8].try_into().unwrap())),
                Some(u32::from_le_bytes(b[8..].try_into().unwrap())),
            )),
            _ => Err("bad configuration payload".into()),
        },
    )
}
fn persistent() -> PanelsOptions<u32, u32, u32> {
    PanelsOptions {
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
    DefaultComponentContext::with_all(
        lifecycle,
        Rc::new(RefCell::new(StateKeeper::with_restored(saved))),
        instances.unwrap_or_else(|| Rc::new(RefCell::new(InstanceKeeper::new()))),
        SharedBack::new(),
    )
}
#[test]
fn whole_membership_codec_restores_mode_keys_and_nested_state() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let value = child_panels(
        &parent,
        &nav,
        initial(true, true, ChildPanelsMode::Triple),
        persistent(),
        |_, c| c,
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    let p = value.get();
    p.details
        .as_ref()
        .unwrap()
        .instance
        .state_keeper()
        .borrow_mut()
        .register("count", || vec![23])
        .unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    assert_eq!(&saved["DefaultChildPanels"][..4], b"DPN1");
    parent.lifecycle().destroy().unwrap();
    let parent = restored_root(saved, None);
    let restored = child_panels(
        &parent,
        &PanelsNavigation::new(),
        Panels::new(99),
        persistent(),
        |_, c| c,
        |_, c| c,
        |_, c| c,
    )
    .unwrap()
    .get();
    assert_eq!(restored.mode, ChildPanelsMode::Triple);
    assert_eq!(restored.main.key, p.main.key);
    assert_eq!(
        restored.details.as_ref().unwrap().key,
        p.details.as_ref().unwrap().key
    );
    assert_eq!(*restored.main.configuration, 1);
    assert_eq!(
        restored
            .details
            .as_ref()
            .unwrap()
            .instance
            .state_keeper()
            .borrow_mut()
            .consume_bytes("count"),
        Some(vec![23])
    );
}

#[test]
fn two_role_codec_adapter_and_transient_save_skips_child_providers() {
    let parent = root();
    let nav = PanelsNavigation::<u8, u8>::new();
    let options = PanelsOptions {
        codec: Some(PanelsCodec::without_extra(
            |m: &u8, d: Option<&u8>| Ok(vec![*m, d.copied().unwrap_or(0)]),
            |b| Ok((b[0], Some(b[1]))),
        )),
        ..Default::default()
    };
    let value = child_panels_without_extra(
        &parent,
        &nav,
        Panels {
            main: Rc::new(1),
            details: Some(Rc::new(2)),
            extra: None,
            mode: ChildPanelsMode::Dual,
        },
        options,
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    assert!(!parent.state_keeper().borrow().save().unwrap().is_empty());
    assert!(value.get().extra.is_none());
    let parent = root();
    let value = child_panels_without_extra(
        &parent,
        &PanelsNavigation::<u8, u8>::new(),
        Panels::new(1),
        PanelsOptions::default(),
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    value
        .get()
        .main
        .instance
        .state_keeper()
        .borrow_mut()
        .register("panic", || panic!("transient provider called"))
        .unwrap();
    assert!(parent.state_keeper().borrow().save().unwrap().is_empty());
}

type PanelView = decompose_rs::value::Value<
    ChildPanels<
        u32,
        DefaultComponentContext,
        u32,
        DefaultComponentContext,
        u32,
        DefaultComponentContext,
    >,
>;
fn build(
    parent: &DefaultComponentContext,
    nav: &PanelsNavigation<u32, u32, u32>,
    p: Panels<u32, u32, u32>,
    options: PanelsOptions<u32, u32, u32>,
) -> PanelView {
    child_panels(parent, nav, p, options, |_, c| c, |_, c| c, |_, c| c).unwrap()
}
fn panic_result(f: impl FnOnce()) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err()
}

#[test]
fn creation_is_forward_destruction_reverse_and_demotions_precede_promotions() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    let factory = |role| {
        let log = log.clone();
        move |_: &u32, ctx: DefaultComponentContext| {
            log.borrow_mut().push((role, LifecycleState::Initialized));
            let out = log.clone();
            let _ = ctx
                .lifecycle()
                .subscribe(move |s| out.borrow_mut().push((role, s)))
                .detach();
            ctx
        }
    };
    let value = child_panels(
        &parent,
        &nav,
        initial(true, true, ChildPanelsMode::Triple),
        PanelsOptions::default(),
        factory("m"),
        factory("d"),
        factory("e"),
    )
    .unwrap();
    assert_eq!(
        log.borrow()
            .iter()
            .filter(|(_, s)| *s == LifecycleState::Initialized)
            .map(|(r, _)| *r)
            .collect::<Vec<_>>(),
        ["m", "d", "e"]
    );
    log.borrow_mut().clear();
    nav.set_mode(ChildPanelsMode::Single);
    let log_ref = log.borrow();
    let main_stop = log_ref
        .iter()
        .position(|x| *x == ("m", LifecycleState::Created))
        .unwrap();
    let details_stop = log_ref
        .iter()
        .position(|x| *x == ("d", LifecycleState::Created))
        .unwrap();
    assert!(main_stop < details_stop);
    drop(log_ref);
    log.borrow_mut().clear();
    nav.navigate(|mut p| {
        p.details = None;
        p.extra = None;
        p
    });
    let log_ref = log.borrow();
    let extra_destroy = log_ref
        .iter()
        .position(|x| *x == ("e", LifecycleState::Destroyed))
        .unwrap();
    let details_destroy = log_ref
        .iter()
        .position(|x| *x == ("d", LifecycleState::Destroyed))
        .unwrap();
    let main_resume = log_ref
        .iter()
        .position(|x| *x == ("m", LifecycleState::Resumed))
        .unwrap();
    assert!(extra_destroy < details_destroy);
    assert!(details_destroy < main_resume);
    assert_eq!(
        value.get().main.instance.lifecycle().state(),
        LifecycleState::Resumed
    );
}

#[test]
fn parent_lifecycle_bounds_every_resumed_panel_and_back_bridge() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let value = build(
        &parent,
        &nav,
        initial(true, true, ChildPanelsMode::Triple),
        PanelsOptions::default(),
    );
    for c in [
        &value.get().main,
        value.get().details.as_ref().unwrap(),
        value.get().extra.as_ref().unwrap(),
    ] {
        c.instance
            .back_handler()
            .register(0, true, |_, _| {})
            .unwrap();
    }
    for state in [
        LifecycleState::Created,
        LifecycleState::Started,
        LifecycleState::Resumed,
    ] {
        parent.lifecycle().move_to(state).unwrap();
        let p = value.get();
        for c in [
            &p.main,
            p.details.as_ref().unwrap(),
            p.extra.as_ref().unwrap(),
        ] {
            assert_eq!(c.instance.lifecycle().state(), state);
        }
        assert_eq!(
            parent.back_handler().back(),
            state != LifecycleState::Created
        );
    }
    let before = value.get();
    parent.lifecycle().destroy().unwrap();
    for c in [
        &before.main,
        before.details.as_ref().unwrap(),
        before.extra.as_ref().unwrap(),
    ] {
        assert!(c.instance.lifecycle().state().is_destroyed());
        assert!(c.instance.instance_keeper().borrow().is_destroyed());
    }
    assert_eq!(parent.back_handler().handler_count(), 0);
    nav.set_panels(4, Some(5), Some(6));
    assert_eq!(*value.get().main.configuration, 1);
}

#[test]
fn resumed_child_handlers_use_existing_lifo_order() {
    let parent = root();
    let value = build(
        &parent,
        &PanelsNavigation::new(),
        initial(true, true, ChildPanelsMode::Triple),
        PanelsOptions::default(),
    );
    let hits = Rc::new(RefCell::new(Vec::new()));
    let p = value.get();
    let mut handles = Vec::new();
    for (r, c) in [
        ("m", &p.main),
        ("d", p.details.as_ref().unwrap()),
        ("e", p.extra.as_ref().unwrap()),
    ] {
        let out = hits.clone();
        handles.push(
            c.instance
                .back_handler()
                .register(0, true, move |_, _| out.borrow_mut().push(r))
                .unwrap()
                .id(),
        );
    }
    parent.back_handler().back();
    assert_eq!(*hits.borrow(), ["e"]);
    p.extra
        .as_ref()
        .unwrap()
        .instance
        .back_handler()
        .unregister(handles[2]);
    parent.back_handler().back();
    assert_eq!(*hits.borrow(), ["e", "d"]);
}

#[test]
fn factory_lifecycle_observer_hook_and_completion_navigation_is_fifo() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    let source = nav.clone();
    let out = log.clone();
    let options = PanelsOptions {
        on_state_changed: Some(Rc::new(move |p: &Panels<u32, u32, u32>, old| {
            out.borrow_mut().push(*p.main);
            if old.is_none() {
                source.activate_main(2);
            }
            if *p.main == 3 {
                source.activate_main(6);
            }
        })),
        ..Default::default()
    };
    let source = nav.clone();
    let value = child_panels(
        &parent,
        &nav,
        initial(false, false, ChildPanelsMode::Single),
        options,
        move |m, c| {
            let source = source.clone();
            if *m == 1 {
                source.activate_main(3);
            }
            if *m == 3 {
                source.activate_main(4);
            }
            let m = *m;
            let _ = c
                .lifecycle()
                .subscribe(move |s| {
                    if m == 3 && s == LifecycleState::Resumed {
                        source.activate_main(5)
                    }
                })
                .detach();
            c
        },
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    assert_eq!(*log.borrow(), [1, 3, 2, 4, 5, 6]);
    let source = nav.clone();
    let once = Rc::new(Cell::new(true));
    let flag = once.clone();
    let _token = value.subscribe(move |p| {
        if *p.main.configuration == 7 && flag.replace(false) {
            source.activate_main(8)
        }
    });
    let source = nav.clone();
    nav.activate_main_with(7, move |_| source.activate_main(9));
    assert_eq!(*log.borrow(), [1, 3, 2, 4, 5, 6, 7, 8, 9]);
}

#[test]
fn event_hook_panic_keeps_committed_state_discards_nested_events_and_retries() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let source = nav.clone();
    let once = Rc::new(Cell::new(true));
    let flag = once.clone();
    let options = PanelsOptions {
        on_state_changed: Some(Rc::new(move |p: &Panels<u32, u32, u32>, old| {
            if old.is_some() && *p.main == 2 && flag.replace(false) {
                source.activate_main(3);
                panic!("hook panic")
            }
        })),
        ..Default::default()
    };
    let value = build(
        &parent,
        &nav,
        initial(false, false, ChildPanelsMode::Single),
        options,
    );
    let completed = Rc::new(Cell::new(false));
    let out = completed.clone();
    assert!(panic_result(
        || nav.activate_main_with(2, move |_| out.set(true))
    ));
    assert!(!completed.get());
    assert_eq!(*value.get().main.configuration, 2);
    nav.activate_main(4);
    assert_eq!(*value.get().main.configuration, 4);
}

#[test]
fn observer_and_completion_panics_discard_reentry_and_leave_source_usable() {
    for observer in [false, true] {
        let parent = root();
        let nav = PanelsNavigation::new();
        let value = build(
            &parent,
            &nav,
            initial(false, false, ChildPanelsMode::Single),
            PanelsOptions::default(),
        );
        let source = nav.clone();
        let _token = value.subscribe(move |p| {
            if observer && *p.main.configuration == 2 {
                source.activate_main(3);
                panic!("observer panic")
            }
        });
        let source = nav.clone();
        assert!(panic_result(|| nav.activate_main_with(2, move |_| {
            if !observer {
                source.activate_main(3);
                panic!("completion panic")
            }
        })));
        assert_eq!(*value.get().main.configuration, 2);
        nav.activate_main(4);
        assert_eq!(*value.get().main.configuration, 4);
    }
}

#[test]
fn constructor_hook_panic_restores_input_and_cleans_new_scopes() {
    let original = root();
    build(
        &original,
        &PanelsNavigation::new(),
        initial(true, true, ChildPanelsMode::Triple),
        persistent(),
    );
    let saved = original.state_keeper().borrow().save().unwrap();
    let parent = restored_root(saved.clone(), None);
    let captured = Rc::new(RefCell::new(Vec::new()));
    let factory = || {
        let out = captured.clone();
        move |_: &u32, c: DefaultComponentContext| {
            out.borrow_mut().push((c.lifecycle(), c.instance_keeper()));
            c
        }
    };
    let options = PanelsOptions {
        on_state_changed: Some(Rc::new(|_, _| panic!("initial hook"))),
        ..persistent()
    };
    assert!(panic_result(|| {
        let _ = child_panels(
            &parent,
            &PanelsNavigation::new(),
            Panels::new(99),
            options,
            factory(),
            factory(),
            factory(),
        );
    }));
    for (l, i) in captured.borrow().iter() {
        assert!(l.state().is_destroyed());
        assert!(i.borrow().is_destroyed());
    }
    assert_eq!(parent.back_handler().handler_count(), 0);
    assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
    let value = build(
        &parent,
        &PanelsNavigation::new(),
        Panels::new(99),
        persistent(),
    );
    assert_eq!(*value.get().main.configuration, 1);
    assert!(value.get().extra.is_some());
}

#[test]
fn existing_retained_instance_identity_survives_hook_failure_and_retry() {
    let instances = Rc::new(RefCell::new(InstanceKeeper::new()));
    let parent = restored_root(Default::default(), Some(instances.clone()));
    let value = build(
        &parent,
        &PanelsNavigation::new(),
        initial(true, true, ChildPanelsMode::Triple),
        persistent(),
    );
    let old = value.get();
    let scope = old.main.instance.instance_keeper();
    let service = scope
        .borrow_mut()
        .get_or_create("service", || Rc::new(23))
        .unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    parent.lifecycle().destroy().unwrap();
    assert!(!scope.borrow().is_destroyed());
    let parent = restored_root(saved.clone(), Some(instances));
    assert!(panic_result(|| {
        let _ = child_panels(
            &parent,
            &PanelsNavigation::new(),
            Panels::new(99),
            PanelsOptions {
                on_state_changed: Some(Rc::new(|_, _| panic!("initial hook"))),
                ..persistent()
            },
            |_, c| c,
            |_, c| c,
            |_, c| c,
        );
    }));
    assert!(!scope.borrow().is_destroyed());
    assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
    let value = build(
        &parent,
        &PanelsNavigation::new(),
        Panels::new(99),
        persistent(),
    );
    assert!(Rc::ptr_eq(
        &scope,
        &value.get().main.instance.instance_keeper()
    ));
    let restored = scope
        .borrow_mut()
        .get_or_create("service", || Rc::new(99))
        .unwrap();
    assert!(Rc::ptr_eq(&service, &restored));
}

#[test]
fn new_scope_factory_failure_preserves_committed_children_and_cleans_provisional_scope() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let captured = Rc::new(RefCell::new(None));
    let out = captured.clone();
    let value = child_panels(
        &parent,
        &nav,
        initial(false, false, ChildPanelsMode::Single),
        PanelsOptions::default(),
        |_, c| c,
        |_, c| c,
        move |e, c| {
            if *e == 2 {
                *out.borrow_mut() = Some((c.lifecycle(), c.instance_keeper()));
                panic!("factory")
            }
            c
        },
    )
    .unwrap();
    let before = value.get();
    assert!(panic_result(|| nav.activate_extra(2)));
    assert!(Rc::ptr_eq(
        &before.main.instance,
        &value.get().main.instance
    ));
    assert!(value.get().extra.is_none());
    let (l, i) = captured.borrow().as_ref().unwrap().clone();
    assert!(l.state().is_destroyed());
    assert!(i.borrow().is_destroyed());
    nav.activate_extra(3);
    assert_eq!(value.get().extra.as_ref().unwrap().key, "child/1");
}

#[test]
fn predictive_cancel_child_deferral_and_back_callback_panic_recovery() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let value = build(
        &parent,
        &nav,
        initial(true, true, ChildPanelsMode::Single),
        PanelsOptions {
            handle_back_button: true,
            ..Default::default()
        },
    );
    parent.back_handler().predictive_start();
    parent.back_handler().predictive_progress(0.2).unwrap();
    parent.back_handler().predictive_cancel().unwrap();
    assert!(value.get().extra.is_some());
    let child = value.get().extra.as_ref().unwrap().instance.back_handler();
    let source = nav.clone();
    let view = value.clone();
    let phases = Rc::new(RefCell::new(Vec::new()));
    let out = phases.clone();
    let once = Rc::new(Cell::new(true));
    let flag = once.clone();
    child
        .register(0, true, move |e, _| {
            out.borrow_mut().push(e.phase);
            if e.phase == BackPhase::Invoked {
                source.dismiss_extra();
                assert!(view.get().extra.is_some());
                if flag.replace(false) {
                    panic!("child back")
                }
            }
        })
        .unwrap();
    parent.back_handler().predictive_start();
    parent.back_handler().predictive_cancel().unwrap();
    assert_eq!(*phases.borrow(), [BackPhase::Started, BackPhase::Cancelled]);
    assert!(panic_result(|| {
        parent.back_handler().back();
    }));
    assert!(value.get().extra.is_some());
    parent.back_handler().back();
    assert!(value.get().extra.is_none());
    assert!(value.get().details.is_some());
}

#[test]
fn provider_reservation_precedes_decoder_factory_and_initial_hook() {
    let original = root();
    build(
        &original,
        &PanelsNavigation::new(),
        initial(false, false, ChildPanelsMode::Single),
        persistent(),
    );
    let parent = restored_root(original.state_keeper().borrow().save().unwrap(), None);
    let count = Rc::new(Cell::new(0));
    let probe = Rc::new({
        let parent = parent.clone();
        let count = count.clone();
        move || {
            assert!(matches!(
                child_panels(
                    &parent,
                    &PanelsNavigation::new(),
                    Panels::new(99u32),
                    PanelsOptions::<u32, u32, u32>::default(),
                    |_, _| panic!("nested"),
                    |_, _| panic!("nested"),
                    |_, _| panic!("nested")
                ),
                Err(NavigationError::Context(
                    DecomposeError::DuplicateKey { .. }
                ))
            ));
            count.set(count.get() + 1);
        }
    });
    let decode = probe.clone();
    let factory = probe.clone();
    let hook = probe.clone();
    let options = PanelsOptions {
        codec: Some(PanelsCodec::new(
            |_: &u32, _: Option<&u32>, _: Option<&u32>| Ok(vec![]),
            move |_| {
                decode();
                Ok((1, None, None))
            },
        )),
        on_state_changed: Some(Rc::new(move |_, _| hook())),
        ..Default::default()
    };
    child_panels(
        &parent,
        &PanelsNavigation::new(),
        Panels::new(99),
        options,
        move |_, c| {
            factory();
            c
        },
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    assert_eq!(count.get(), 3);
}

#[test]
fn retained_type_conflict_returns_input_without_factory() {
    let original = root();
    build(
        &original,
        &PanelsNavigation::new(),
        initial(true, true, ChildPanelsMode::Triple),
        persistent(),
    );
    let saved = original.state_keeper().borrow().save().unwrap();
    let parent = restored_root(saved.clone(), None);
    parent
        .instance_keeper()
        .borrow_mut()
        .get_or_create("DefaultChildPanels", || 23u32)
        .unwrap();
    assert!(matches!(
        child_panels(
            &parent,
            &PanelsNavigation::new(),
            Panels::new(99),
            persistent(),
            |_, _| panic!("factory"),
            |_, _| panic!("factory"),
            |_, _| panic!("factory")
        ),
        Err(NavigationError::Context(_))
    ));
    assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
}

#[test]
fn codec_errors_preserve_input_and_serialize_errors_propagate() {
    let original = root();
    build(
        &original,
        &PanelsNavigation::new(),
        initial(true, true, ChildPanelsMode::Triple),
        persistent(),
    );
    let saved = original.state_keeper().borrow().save().unwrap();
    let parent = restored_root(saved.clone(), None);
    let options = PanelsOptions {
        codec: Some(PanelsCodec::new(
            |_: &u32, _: Option<&u32>, _: Option<&u32>| Err("encode".into()),
            |_| Err("decode".into()),
        )),
        ..Default::default()
    };
    let result = child_panels(
        &parent,
        &PanelsNavigation::new(),
        Panels::new(99),
        options,
        |_, _| panic!("factory"),
        |_, _| panic!("factory"),
        |_, _| panic!("factory"),
    );
    assert!(matches!(result,Err(NavigationError::State(ref e)) if e=="decode"));
    assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
    let parent = root();
    let options = PanelsOptions {
        codec: Some(PanelsCodec::new(
            |_: &u32, _: Option<&u32>, _: Option<&u32>| Err("encode".into()),
            |_| Ok((1, None, None)),
        )),
        ..Default::default()
    };
    build(
        &parent,
        &PanelsNavigation::new(),
        initial(false, false, ChildPanelsMode::Single),
        options,
    );
    assert!(parent.state_keeper().borrow().save().is_err());
}

#[test]
fn save_captures_membership_mode_keys_and_counter_before_provider_reentry() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let value = build(
        &parent,
        &nav,
        initial(true, true, ChildPanelsMode::Triple),
        persistent(),
    );
    let source = nav.clone();
    let once = Rc::new(Cell::new(true));
    let flag = once.clone();
    value
        .get()
        .main
        .instance
        .state_keeper()
        .borrow_mut()
        .register("navigate", move || {
            if flag.replace(false) {
                source.set_panels(9, None, None);
                source.set_mode(ChildPanelsMode::Single);
            }
            vec![23]
        })
        .unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    assert_eq!(*value.get().main.configuration, 9);
    let parent = restored_root(saved, None);
    let nav = PanelsNavigation::new();
    let restored = build(&parent, &nav, Panels::new(99), persistent());
    let p = restored.get();
    assert_eq!(p.mode, ChildPanelsMode::Triple);
    assert_eq!(*p.main.configuration, 1);
    assert_eq!(p.extra.as_ref().unwrap().key, "child/2");
    nav.activate_extra(4);
    assert_eq!(restored.get().extra.as_ref().unwrap().key, "child/3");
}

#[test]
fn broadcast_receivers_keep_independent_state_and_share_owned_configuration_once() {
    #[derive(PartialEq, Eq)]
    struct NoClone(u32);
    let one = root();
    let two = root();
    let nav = PanelsNavigation::<NoClone, NoClone, NoClone>::new();
    let a = child_panels(
        &one,
        &nav,
        Panels::new(NoClone(1)),
        PanelsOptions::default(),
        |_, c| c,
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    let b = child_panels(
        &two,
        &nav,
        Panels::new(NoClone(2)),
        PanelsOptions::default(),
        |_, c| c,
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = seen.clone();
    let source = nav.clone();
    nav.activate_details_with(NoClone(3), move |r| {
        let r = r.unwrap();
        out.borrow_mut().push(r.old_panels.main.0);
        if r.old_panels.main.0 == 1 {
            source.activate_extra(NoClone(4));
        }
    });
    assert_eq!(*seen.borrow(), [1, 2]);
    assert!(Rc::ptr_eq(
        &a.get().details.as_ref().unwrap().configuration,
        &b.get().details.as_ref().unwrap().configuration
    ));
    assert!(a.get().extra.is_some() && b.get().extra.is_some());
    assert_eq!(a.get().main.configuration.0, 1);
    assert_eq!(b.get().main.configuration.0, 2);
}

#[test]
fn custom_nonclone_context_services_reach_all_three_factories() {
    struct Context {
        delegate: DefaultComponentContext,
        service: Rc<Cell<u32>>,
    }
    impl ComponentContext for Context {
        fn lifecycle(&self) -> LifecycleRegistry {
            self.delegate.lifecycle()
        }
        fn state_keeper(&self) -> SharedKeeper {
            self.delegate.state_keeper()
        }
        fn instance_keeper(&self) -> SharedInstances {
            self.delegate.instance_keeper()
        }
        fn back_handler(&self) -> SharedBack {
            self.delegate.back_handler()
        }
        fn context_factory(&self) -> ContextFactory<Self> {
            let service = self.service.clone();
            Rc::new(move |parts| Self {
                delegate: DefaultComponentContext::from_parts(parts),
                service: service.clone(),
            })
        }
    }
    let service = Rc::new(Cell::new(0));
    let parent = Context {
        delegate: root(),
        service: service.clone(),
    };
    let value = child_panels(
        &parent,
        &PanelsNavigation::new(),
        initial(true, true, ChildPanelsMode::Triple),
        PanelsOptions::default(),
        |_, c| {
            c.service.set(c.service.get() + 1);
            c
        },
        |_, c| {
            c.service.set(c.service.get() + 10);
            c
        },
        |_, c| {
            c.service.set(c.service.get() + 100);
            c
        },
    )
    .unwrap();
    assert_eq!(service.get(), 111);
    assert!(Rc::ptr_eq(
        &service,
        &value.get().extra.as_ref().unwrap().instance.service
    ));
}

#[test]
fn dropped_view_and_source_leave_parent_owned_engine_without_cycles() {
    struct Component {
        context: DefaultComponentContext,
        capture: Rc<()>,
    }
    let parent = root();
    let nav = PanelsNavigation::<u32, u32, u32>::new();
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let out = capture.clone();
    let value = child_panels(
        &parent,
        &nav,
        Panels::new(1),
        PanelsOptions::default(),
        move |_, c| Component {
            context: c,
            capture: out.clone(),
        },
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    let child = value.get().main.instance.clone();
    assert_eq!(Rc::strong_count(&child.capture), 3);
    assert_eq!(child.context.lifecycle().state(), LifecycleState::Resumed);
    drop(child);
    drop(capture);
    drop(value);
    drop(nav);
    assert!(weak.upgrade().is_some());
    parent.lifecycle().destroy().unwrap();
    drop(parent);
    assert!(weak.upgrade().is_none());
}

#[test]
fn every_mode_to_mode_transition_preserves_every_present_role_identity() {
    for from in [
        ChildPanelsMode::Single,
        ChildPanelsMode::Dual,
        ChildPanelsMode::Triple,
    ] {
        for (to, d, e, main_status, details_status, extra_status) in mode_cases() {
            let parent = root();
            let nav = PanelsNavigation::new();
            let value = build(&parent, &nav, initial(d, e, from), PanelsOptions::default());
            let old = value.get();
            nav.set_mode(to);
            let p = value.get();
            assert!(Rc::ptr_eq(&old.main.instance, &p.main.instance));
            assert!(Rc::ptr_eq(
                &old.main.instance.instance_keeper(),
                &p.main.instance.instance_keeper()
            ));
            assert_eq!(old.main.key, p.main.key);
            if d {
                let (a, b) = (old.details.as_ref().unwrap(), p.details.as_ref().unwrap());
                assert_eq!(a.key, b.key);
                assert!(Rc::ptr_eq(&a.instance, &b.instance));
            }
            if e {
                let (a, b) = (old.extra.as_ref().unwrap(), p.extra.as_ref().unwrap());
                assert_eq!(a.key, b.key);
                assert!(Rc::ptr_eq(&a.instance, &b.instance));
            }
            assert_eq!(p.main.instance.lifecycle().state(), main_status);
            assert_eq!(
                p.details.as_ref().map(|c| c.instance.lifecycle().state()),
                details_status
            );
            assert_eq!(
                p.extra.as_ref().map(|c| c.instance.lifecycle().state()),
                extra_status
            );
        }
    }
}

#[test]
fn lifecycle_callback_panic_cleans_fresh_scopes_and_next_navigation_reconciles() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let once = Rc::new(Cell::new(false));
    let flag = once.clone();
    let captured = Rc::new(RefCell::new(None));
    let out = captured.clone();
    let value = child_panels(
        &parent,
        &nav,
        initial(false, false, ChildPanelsMode::Single),
        PanelsOptions::default(),
        move |_, c| {
            let flag = flag.clone();
            let _ = c
                .lifecycle()
                .subscribe(move |s| {
                    if s == LifecycleState::Created && flag.replace(false) {
                        panic!("lifecycle")
                    }
                })
                .detach();
            c
        },
        move |_, c| {
            *out.borrow_mut() = Some((c.lifecycle(), c.instance_keeper()));
            c
        },
        |_, c| c,
    )
    .unwrap();
    once.set(true);
    assert!(panic_result(|| nav.activate_details(2)));
    assert!(value.get().details.is_none());
    let (l, i) = captured.borrow().as_ref().unwrap().clone();
    assert!(l.state().is_destroyed());
    assert!(i.borrow().is_destroyed());
    nav.activate_details(3);
    assert_eq!(*value.get().details.as_ref().unwrap().configuration, 3);
    assert_eq!(
        value.get().main.instance.lifecycle().state(),
        LifecycleState::Created
    );
}

#[test]
fn initial_factory_failure_returns_restored_state_and_cleans_all_fresh_scopes() {
    let original = root();
    build(
        &original,
        &PanelsNavigation::new(),
        initial(true, true, ChildPanelsMode::Triple),
        persistent(),
    );
    let saved = original.state_keeper().borrow().save().unwrap();
    let parent = restored_root(saved.clone(), None);
    let scopes = Rc::new(RefCell::new(Vec::new()));
    let factory = |fail: bool| {
        let out = scopes.clone();
        move |_: &u32, c: DefaultComponentContext| {
            out.borrow_mut().push((c.lifecycle(), c.instance_keeper()));
            assert!(!fail, "factory");
            c
        }
    };
    assert!(panic_result(|| {
        let _ = child_panels(
            &parent,
            &PanelsNavigation::new(),
            Panels::new(99),
            persistent(),
            factory(false),
            factory(false),
            factory(true),
        );
    }));
    for (l, i) in scopes.borrow().iter() {
        assert!(l.state().is_destroyed());
        assert!(i.borrow().is_destroyed());
    }
    assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
    assert_eq!(parent.back_handler().handler_count(), 0);
    assert!(
        build(
            &parent,
            &PanelsNavigation::new(),
            Panels::new(99),
            persistent()
        )
        .get()
        .extra
        .is_some()
    );
}

#[test]
fn malformed_restoration_returns_exact_input_and_allows_same_key_retry() {
    let original = root();
    build(
        &original,
        &PanelsNavigation::new(),
        initial(true, true, ChildPanelsMode::Triple),
        persistent(),
    );
    let saved = original.state_keeper().borrow().save().unwrap();
    let mut invalid = saved.clone();
    invalid.get_mut("DefaultChildPanels").unwrap()[12] = 3;
    let parent = restored_root(invalid.clone(), None);
    assert!(matches!(
        child_panels(
            &parent,
            &PanelsNavigation::new(),
            Panels::new(99),
            persistent(),
            |_, _| panic!("factory"),
            |_, _| panic!("factory"),
            |_, _| panic!("factory")
        ),
        Err(NavigationError::State(_))
    ));
    assert_eq!(parent.state_keeper().borrow().save().unwrap(), invalid);
    assert!(
        !parent
            .state_keeper()
            .borrow()
            .is_registered("DefaultChildPanels")
    );
    let value = build(
        &parent,
        &PanelsNavigation::new(),
        Panels::new(99),
        PanelsOptions::default(),
    );
    assert_eq!(*value.get().main.configuration, 99);
}

#[test]
fn next_key_exhaustion_reports_error_before_factory_and_keeps_current_state() {
    let original = root();
    build(
        &original,
        &PanelsNavigation::new(),
        initial(true, true, ChildPanelsMode::Triple),
        persistent(),
    );
    let mut saved = original.state_keeper().borrow().save().unwrap();
    saved.get_mut("DefaultChildPanels").unwrap()[4..12].copy_from_slice(&u64::MAX.to_le_bytes());
    let parent = restored_root(saved, None);
    let nav = PanelsNavigation::new();
    let calls = Rc::new(Cell::new(0));
    let out = calls.clone();
    let value = child_panels(
        &parent,
        &nav,
        Panels::new(99),
        persistent(),
        move |_, c| {
            out.set(out.get() + 1);
            c
        },
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    let before = value.get();
    let error = Rc::new(RefCell::new(None));
    let out = error.clone();
    nav.activate_main_with(4, move |r| *out.borrow_mut() = r.err());
    assert!(matches!(*error.borrow(), Some(NavigationError::State(_))));
    assert_eq!(calls.get(), 1);
    assert!(Rc::ptr_eq(
        &before.main.instance,
        &value.get().main.instance
    ));
}

#[test]
fn initial_hook_parent_destroy_returns_destroyed_without_pumping_queued_events() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let source = nav.clone();
    let life = parent.lifecycle();
    let options = PanelsOptions {
        on_state_changed: Some(Rc::new(move |_: &Panels<u32, u32, u32>, _| {
            source.activate_main(2);
            life.destroy().unwrap();
        })),
        ..Default::default()
    };
    assert!(matches!(
        child_panels(
            &parent,
            &nav,
            initial(false, false, ChildPanelsMode::Single),
            options,
            |_, c| c,
            |_, c| c,
            |_, c| c
        ),
        Err(NavigationError::Destroyed)
    ));
    assert_eq!(parent.back_handler().handler_count(), 0);
    assert!(parent.state_keeper().borrow().save().unwrap().is_empty());
}

#[test]
fn panels_restore_nested_stack_slot_and_pages_after_process_death() {
    use decompose_rs::router::{pages::*, slot::*, stack::*};
    struct Nested {
        context: DefaultComponentContext,
        slot_nav: SlotNavigation<u32>,
        slot: decompose_rs::value::Value<ChildSlot<u32, DefaultComponentContext>>,
        stack: decompose_rs::value::Value<ChildStack<u32, DefaultComponentContext>>,
        pages: decompose_rs::value::Value<ChildPages<u32, DefaultComponentContext>>,
    }
    fn nested(_: &u32, c: DefaultComponentContext) -> Nested {
        let slot_nav = SlotNavigation::new();
        let slot = child_slot(
            &c,
            &slot_nav,
            Some(10),
            SlotOptions {
                codec: Some(SlotCodec::new(
                    |x: &u32| Ok(x.to_le_bytes().to_vec()),
                    |b| Ok(u32::from_le_bytes(b.try_into().map_err(|_| "slot")?)),
                )),
                ..Default::default()
            },
            |_, c| c,
        )
        .unwrap();
        let list_encode = |xs: &[Rc<u32>]| Ok(xs.iter().flat_map(|x| x.to_le_bytes()).collect());
        let list_decode = |b: &[u8]| {
            Ok(b.chunks_exact(4)
                .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                .collect())
        };
        let stack = child_stack(
            &c,
            &StackNavigation::new(),
            vec![20, 21],
            StackOptions {
                codec: Some(StackCodec::new(list_encode, list_decode)),
                ..Default::default()
            },
            |_, c| c,
        )
        .unwrap();
        let pages = child_pages(
            &c,
            &PagesNavigation::new(),
            Pages::new(vec![30, 31], 1).unwrap(),
            PagesOptions {
                codec: Some(PagesCodec::new(list_encode, list_decode)),
                ..Default::default()
            },
            |_, c| c,
        )
        .unwrap();
        Nested {
            context: c,
            slot_nav,
            slot,
            stack,
            pages,
        }
    }
    let parent = root();
    let nav = PanelsNavigation::new();
    let value = child_panels(
        &parent,
        &nav,
        initial(true, true, ChildPanelsMode::Triple),
        persistent(),
        nested,
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    value.get().main.instance.slot_nav.activate(99);
    let key = value.get().main.key.clone();
    let saved = parent.state_keeper().borrow().save().unwrap();
    parent.lifecycle().destroy().unwrap();
    let parent = restored_root(saved, None);
    let value = child_panels(
        &parent,
        &PanelsNavigation::new(),
        Panels::new(99),
        persistent(),
        nested,
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    let p = value.get();
    assert_eq!(p.main.key, key);
    assert_eq!(
        *p.main
            .instance
            .slot
            .get()
            .child
            .as_ref()
            .unwrap()
            .configuration,
        99
    );
    assert_eq!(*p.main.instance.stack.get().active.configuration, 21);
    assert_eq!(p.main.instance.pages.get().selected_index, Some(1));
    assert_eq!(
        p.main.instance.context.lifecycle().state(),
        LifecycleState::Resumed
    );
}

#[test]
fn automatic_back_hook_panic_keeps_pop_committed_and_dispatch_can_retry() {
    let parent = root();
    let nav = PanelsNavigation::new();
    let source = nav.clone();
    let once = Rc::new(Cell::new(true));
    let flag = once.clone();
    let options = PanelsOptions {
        handle_back_button: true,
        on_state_changed: Some(Rc::new(move |p: &Panels<u32, u32, u32>, old| {
            if old.is_some() && p.extra.is_none() && flag.replace(false) {
                source.activate_extra(3);
                panic!("automatic Back state hook");
            }
        })),
        ..Default::default()
    };
    let value = build(
        &parent,
        &nav,
        initial(true, true, ChildPanelsMode::Single),
        options,
    );
    assert!(panic_result(|| {
        parent.back_handler().back();
    }));
    assert!(value.get().extra.is_none());
    assert!(value.get().details.is_some());
    parent.back_handler().back();
    assert!(value.get().details.is_none());
    nav.activate_details(4);
    assert!(parent.back_handler().back());
    assert!(value.get().details.is_none());
}

#[test]
fn two_role_adapter_restores_authoritative_membership_and_mode() {
    fn options() -> PanelsOptions<u8, u8> {
        PanelsOptions {
            codec: Some(PanelsCodec::without_extra(
                |m: &u8, d: Option<&u8>| Ok(vec![*m, d.copied().unwrap_or(0)]),
                |b| Ok((b[0], (b[1] != 0).then_some(b[1]))),
            )),
            ..Default::default()
        }
    }
    let parent = root();
    let nav = PanelsNavigation::<u8, u8>::new();
    let value =
        child_panels_without_extra(&parent, &nav, Panels::new(1), options(), |_, c| c, |_, c| c)
            .unwrap();
    nav.activate_details(2);
    nav.set_mode(ChildPanelsMode::Dual);
    let before = value.get();
    let saved = parent.state_keeper().borrow().save().unwrap();
    parent.lifecycle().destroy().unwrap();
    let parent = restored_root(saved, None);
    let value = child_panels_without_extra(
        &parent,
        &PanelsNavigation::new(),
        Panels::new(99),
        options(),
        |_, c| c,
        |_, c| c,
    )
    .unwrap();
    let after = value.get();
    assert_eq!(*after.main.configuration, 1);
    assert_eq!(*after.details.as_ref().unwrap().configuration, 2);
    assert_eq!(after.mode, ChildPanelsMode::Dual);
    assert_eq!(after.main.key, before.main.key);
    assert_eq!(
        after.details.as_ref().unwrap().key,
        before.details.as_ref().unwrap().key
    );
    assert!(after.extra.is_none());
}

#[test]
fn parent_destroy_during_factory_returns_destroyed_and_restored_input() {
    let original = root();
    build(
        &original,
        &PanelsNavigation::new(),
        initial(true, true, ChildPanelsMode::Triple),
        persistent(),
    );
    let saved = original.state_keeper().borrow().save().unwrap();
    let parent = restored_root(saved.clone(), None);
    let life = parent.lifecycle();
    let result = child_panels(
        &parent,
        &PanelsNavigation::new(),
        Panels::new(99),
        persistent(),
        move |_, c| {
            life.destroy().unwrap();
            c
        },
        |_, c| c,
        |_, c| c,
    );
    assert!(matches!(result, Err(NavigationError::Destroyed)));
    assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
    assert_eq!(parent.back_handler().handler_count(), 0);
}
