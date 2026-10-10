use decompose_rs::router::{
    children::NavigationSource, items::*, pages::*, panels::*, slot::*, stack::*,
};
use decompose_rs::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

fn root() -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::new(lifecycle)
}

#[derive(Debug, PartialEq, Eq)]
struct Route(u32);
struct Component;

#[test]
fn all_six_signatures_accept_dyn_sources_and_borrowed_non_clone_fn_once() {
    let parent = root();
    let mut calls = 0;
    let nav = StackNavigation::new();
    let source: &dyn NavigationSource<StackEvent<Route>> = &nav;
    let route = Route(1);
    let stack = child_stack_with_initial::<DefaultComponentContext, Route, Component>(
        &parent,
        source,
        || {
            calls += 1;
            vec![route]
        },
        StackOptions::default(),
        |_, _| Component,
    )
    .unwrap();
    assert_eq!(stack.get().active.configuration.0, 1);
    let nav = SlotNavigation::new();
    let source: &dyn NavigationSource<SlotEvent<Route>> = &nav;
    let route = Route(2);
    child_slot_with_initial::<DefaultComponentContext, Route, Component>(
        &parent,
        source,
        || {
            calls += 1;
            Some(route)
        },
        SlotOptions::default(),
        |_, _| Component,
    )
    .unwrap();
    let nav = PagesNavigation::new();
    let source: &dyn NavigationSource<PagesEvent<Route>> = &nav;
    let route = Route(3);
    child_pages_with_initial::<DefaultComponentContext, Route, Component>(
        &parent,
        source,
        || {
            calls += 1;
            Pages::new(vec![route], 0).unwrap()
        },
        PagesOptions::default(),
        |_, _| Component,
    )
    .unwrap();
    let nav = ItemsNavigation::new();
    let source: &dyn NavigationSource<ItemsEvent<Route>> = &nav;
    let route = Route(4);
    child_items_with_initial::<DefaultComponentContext, Route, Component>(
        &parent,
        source,
        || {
            calls += 1;
            Items::new(vec![route]).unwrap()
        },
        ItemsOptions::default(),
        |_, _| Component,
    )
    .unwrap();
    let nav = PanelsNavigation::new();
    let source: &dyn NavigationSource<PanelsEvent<Route, Route, Route>> = &nav;
    let route = Route(5);
    child_panels_with_initial::<
        DefaultComponentContext,
        Route,
        Component,
        Route,
        Component,
        Route,
        Component,
    >(
        &parent,
        source,
        || {
            calls += 1;
            Panels {
                main: Rc::new(route),
                details: None,
                extra: None,
                mode: ChildPanelsMode::Triple,
            }
        },
        PanelsOptions::default(),
        |_, _| Component,
        |_, _| Component,
        |_, _| Component,
    )
    .unwrap();
    let nav = PanelsNavigation::new();
    let source: &dyn NavigationSource<PanelsEvent<Route, Route>> = &nav;
    let route = Route(6);
    child_panels_without_extra_with_initial::<
        DefaultComponentContext,
        Route,
        Component,
        Route,
        Component,
    >(
        &root(),
        source,
        || {
            calls += 1;
            Panels {
                main: Rc::new(route),
                details: None,
                extra: None,
                mode: ChildPanelsMode::Dual,
            }
        },
        PanelsOptions::default(),
        |_, _| Component,
        |_, _| Component,
    )
    .unwrap();
    assert_eq!(calls, 6);
}

#[derive(Clone, Copy, Debug)]
enum Family {
    Stack,
    Slot,
    Pages,
    Items,
    Panels,
    Panels2,
}
const FAMILIES: [Family; 6] = [
    Family::Stack,
    Family::Slot,
    Family::Pages,
    Family::Items,
    Family::Panels,
    Family::Panels2,
];

struct Seed {
    members: Vec<u32>,
    selected: Option<usize>,
    active: bool,
    mode: ChildPanelsMode,
}
impl Seed {
    fn populated() -> Self {
        Self {
            members: vec![7, 8, 9],
            selected: Some(1),
            active: true,
            mode: ChildPanelsMode::Dual,
        }
    }
    fn empty() -> Self {
        Self {
            members: vec![],
            selected: None,
            active: false,
            mode: ChildPanelsMode::Single,
        }
    }
}
#[derive(Debug, PartialEq)]
struct Snapshot {
    members: Vec<u32>,
    selected: Option<usize>,
    active: usize,
    mode: Option<ChildPanelsMode>,
}
fn restored(saved: BTreeMap<String, Vec<u8>>) -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::with_stores(
        lifecycle,
        StateStore::from(StateKeeper::with_restored(saved)),
        InstanceStore::from(InstanceKeeper::new()),
        SharedBack::new(),
    )
}
fn encode_list(configurations: &[Rc<u32>]) -> Result<Vec<u8>, String> {
    Ok(configurations
        .iter()
        .flat_map(|c| c.to_le_bytes())
        .collect())
}
fn decode_list(bytes: &[u8], fail: bool) -> Result<Vec<u32>, String> {
    if fail {
        return Err("decoder rejected membership".into());
    }
    if !bytes.len().is_multiple_of(4) {
        return Err("bad test payload".into());
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect())
}
fn construct(
    family: Family,
    parent: &DefaultComponentContext,
    initial: impl FnOnce() -> Seed,
    persistent: bool,
    decoder_fails: bool,
) -> Result<Snapshot, decompose_rs::router::NavigationError> {
    let mut snapshot = Snapshot {
        members: vec![],
        selected: None,
        active: 0,
        mode: None,
    };
    match family {
        Family::Stack => {
            let value = child_stack_with_initial(
                parent,
                &StackNavigation::new(),
                || initial().members,
                StackOptions {
                    codec: persistent.then(|| {
                        StackCodec::new(encode_list, move |b| decode_list(b, decoder_fails))
                    }),
                    ..Default::default()
                },
                |_, ctx| ctx,
            )?;
            snapshot.members = value.get().items().map(|c| *c.configuration).collect();
        }
        Family::Slot => {
            let value = child_slot_with_initial(
                parent,
                &SlotNavigation::new(),
                || initial().members.into_iter().next(),
                SlotOptions {
                    codec: persistent.then(|| {
                        SlotCodec::new(
                            |c: &u32| Ok(c.to_le_bytes().to_vec()),
                            move |b| Ok(decode_list(b, decoder_fails)?[0]),
                        )
                    }),
                    ..Default::default()
                },
                |_, ctx| ctx,
            )?;
            snapshot.members = value.get().child.iter().map(|c| *c.configuration).collect();
        }
        Family::Pages => {
            let value = child_pages_with_initial(
                parent,
                &PagesNavigation::new(),
                || {
                    let seed = initial();
                    Pages {
                        items: seed.members.into_iter().map(Rc::new).collect(),
                        selected_index: seed.selected,
                    }
                },
                PagesOptions {
                    codec: persistent.then(|| {
                        PagesCodec::new(encode_list, move |b| decode_list(b, decoder_fails))
                    }),
                    ..Default::default()
                },
                |_, ctx| ctx,
            )?;
            snapshot.members = value
                .get()
                .items
                .iter()
                .map(|c| **c.configuration())
                .collect();
            snapshot.selected = value.get().selected_index;
        }
        Family::Items => {
            let value = child_items_with_initial(
                parent,
                &ItemsNavigation::new(),
                || {
                    let seed = initial();
                    let mut items = Items {
                        items: seed.members.into_iter().map(Rc::new).collect(),
                        active_items: ItemMap::new(),
                    };
                    if seed.active {
                        for c in &items.items {
                            items
                                .active_items
                                .insert(c.clone(), ActiveLifecycleState::Started);
                        }
                    }
                    items
                },
                ItemsOptions {
                    codec: persistent.then(|| {
                        ItemsCodec::new(encode_list, move |b| decode_list(b, decoder_fails))
                    }),
                    ..Default::default()
                },
                |_, ctx| ctx,
            )?;
            snapshot.members = value.get().items.iter().map(|c| **c).collect();
            snapshot.active = value.get().active_items.len();
        }
        Family::Panels => {
            let value = child_panels_with_initial(
                parent,
                &PanelsNavigation::new(),
                || {
                    let seed = initial();
                    Panels {
                        main: Rc::new(seed.members[0]),
                        details: seed.members.get(1).copied().map(Rc::new),
                        extra: seed.members.get(2).copied().map(Rc::new),
                        mode: seed.mode,
                    }
                },
                PanelsOptions {
                    codec: persistent.then(|| {
                        PanelsCodec::new(
                            |m: &u32, d: Option<&u32>, e: Option<&u32>| {
                                Ok([
                                    *m,
                                    d.copied().unwrap_or(u32::MAX),
                                    e.copied().unwrap_or(u32::MAX),
                                ]
                                .into_iter()
                                .flat_map(u32::to_le_bytes)
                                .collect())
                            },
                            move |b| {
                                let c = decode_list(b, decoder_fails)?;
                                Ok((
                                    c[0],
                                    (c[1] != u32::MAX).then_some(c[1]),
                                    (c[2] != u32::MAX).then_some(c[2]),
                                ))
                            },
                        )
                    }),
                    ..Default::default()
                },
                |_, ctx| ctx,
                |_, ctx| ctx,
                |_, ctx| ctx,
            )?;
            let p = value.get();
            snapshot.members = std::iter::once(*p.main.configuration)
                .chain(p.details.iter().map(|c| *c.configuration))
                .chain(p.extra.iter().map(|c| *c.configuration))
                .collect();
            snapshot.mode = Some(p.mode);
        }
        Family::Panels2 => {
            let value = child_panels_without_extra_with_initial(
                parent,
                &PanelsNavigation::new(),
                || {
                    let seed = initial();
                    Panels {
                        main: Rc::new(seed.members[0]),
                        details: seed.members.get(1).copied().map(Rc::new),
                        extra: None,
                        mode: seed.mode,
                    }
                },
                PanelsOptions {
                    codec: persistent.then(|| {
                        PanelsCodec::without_extra(
                            |m: &u32, d: Option<&u32>| {
                                Ok([*m, d.copied().unwrap_or(u32::MAX)]
                                    .into_iter()
                                    .flat_map(u32::to_le_bytes)
                                    .collect())
                            },
                            move |b| {
                                let c = decode_list(b, decoder_fails)?;
                                Ok((c[0], (c[1] != u32::MAX).then_some(c[1])))
                            },
                        )
                    }),
                    ..Default::default()
                },
                |_, ctx| ctx,
                |_, ctx| ctx,
            )?;
            let p = value.get();
            snapshot.members = std::iter::once(*p.main.configuration)
                .chain(p.details.iter().map(|c| *c.configuration))
                .collect();
            snapshot.mode = Some(p.mode);
        }
    }
    Ok(snapshot)
}

#[test]
fn every_family_initializes_once_and_restore_suppresses_initialization() {
    for family in FAMILIES {
        let parent = root();
        let calls = Cell::new(0);
        let expected = construct(
            family,
            &parent,
            || {
                calls.set(calls.get() + 1);
                Seed::populated()
            },
            true,
            false,
        )
        .unwrap();
        assert_eq!(calls.get(), 1, "{family:?}");
        let saved = parent.state_store().save().unwrap();
        let restored_parent = restored(saved.clone());
        let actual = construct(
            family,
            &restored_parent,
            || panic!("restored {family:?} invoked initializer"),
            true,
            false,
        )
        .unwrap();
        assert_eq!(actual, expected, "{family:?}");
        assert_eq!(
            restored_parent.state_store().save().unwrap(),
            saved,
            "{family:?}"
        );
        // With no codec, saved membership has no authority over the transient initializer.
        let transient_parent = root();
        construct(
            family,
            &transient_parent,
            || {
                calls.set(calls.get() + 1);
                Seed::populated()
            },
            false,
            false,
        )
        .unwrap();
        let transient_parent = restored(transient_parent.state_store().save().unwrap());
        construct(
            family,
            &transient_parent,
            || {
                calls.set(calls.get() + 1);
                Seed::populated()
            },
            false,
            false,
        )
        .unwrap();
        assert_eq!(calls.get(), 3, "{family:?}");
    }
}

#[test]
fn empty_and_all_inactive_restoration_is_authoritative() {
    for family in [Family::Slot, Family::Pages, Family::Items] {
        let parent = root();
        let expected = construct(family, &parent, Seed::empty, true, false).unwrap();
        let parent = restored(parent.state_store().save().unwrap());
        assert_eq!(
            construct(family, &parent, || panic!("empty state reset"), true, false).unwrap(),
            expected
        );
    }
    let parent = root();
    let expected = construct(
        Family::Items,
        &parent,
        || Seed {
            active: false,
            ..Seed::populated()
        },
        true,
        false,
    )
    .unwrap();
    assert_eq!(expected.active, 0);
    let parent = restored(parent.state_store().save().unwrap());
    assert_eq!(
        construct(
            Family::Items,
            &parent,
            || panic!("inactive state reset"),
            true,
            false
        )
        .unwrap(),
        expected
    );
}

#[test]
fn panels_membership_and_every_mode_restore_for_both_adapters() {
    for family in [Family::Panels, Family::Panels2] {
        for mode in [
            ChildPanelsMode::Single,
            ChildPanelsMode::Dual,
            ChildPanelsMode::Triple,
        ] {
            for members in [vec![1], vec![1, 2], vec![1, 2, 3]] {
                let parent = root();
                let expected = construct(
                    family,
                    &parent,
                    || Seed {
                        members,
                        mode,
                        ..Seed::populated()
                    },
                    true,
                    false,
                )
                .unwrap();
                let parent = restored(parent.state_store().save().unwrap());
                assert_eq!(
                    construct(family, &parent, || panic!("panels reset"), true, false).unwrap(),
                    expected
                );
            }
        }
    }
}

#[test]
fn destroyed_and_duplicate_preflight_never_call_initializers() {
    use decompose_rs::router::NavigationError;
    for family in FAMILIES {
        let parent = root();
        parent.lifecycle().destroy().unwrap();
        assert!(matches!(
            construct(
                family,
                &parent,
                || panic!("destroyed initialized"),
                true,
                false
            ),
            Err(NavigationError::Destroyed)
        ));
        let parent = root();
        construct(family, &parent, Seed::populated, true, false).unwrap();
        assert!(
            construct(
                family,
                &parent,
                || panic!("duplicate initialized"),
                true,
                false
            )
            .is_err()
        );
    }
}

#[test]
fn malformed_and_decoder_error_restore_never_initialize_and_keep_retry_bytes() {
    use decompose_rs::router::NavigationError;
    for family in FAMILIES {
        let parent = root();
        let expected = construct(family, &parent, Seed::populated, true, false).unwrap();
        let saved = parent.state_store().save().unwrap();
        let parent = restored(saved.clone());
        assert!(
            matches!(construct(family, &parent, || panic!("decoder failure initialized"), true, true), Err(NavigationError::State(message)) if message == "decoder rejected membership")
        );
        assert_eq!(parent.state_store().save().unwrap(), saved);
        assert_eq!(
            construct(family, &parent, || panic!("retry reset"), true, false).unwrap(),
            expected
        );
        let bad: BTreeMap<_, _> = saved
            .keys()
            .map(|k| (k.clone(), b"broken frame".to_vec()))
            .collect();
        let parent = restored(bad.clone());
        for _ in 0..2 {
            assert!(matches!(
                construct(
                    family,
                    &parent,
                    || panic!("malformed initialized"),
                    true,
                    false
                ),
                Err(NavigationError::State(_))
            ));
            assert_eq!(parent.state_store().save().unwrap(), bad);
        }
    }
}

#[test]
fn invalid_initial_models_release_registration_and_allow_retry() {
    use decompose_rs::router::NavigationError;
    for (family, seed, expected) in [
        (Family::Stack, Seed::empty(), NavigationError::EmptyStack),
        (
            Family::Pages,
            Seed {
                selected: Some(99),
                ..Seed::populated()
            },
            NavigationError::InvalidSelection {
                selected_index: Some(99),
                item_count: 3,
            },
        ),
        (
            Family::Items,
            Seed {
                members: vec![1, 1],
                ..Seed::populated()
            },
            NavigationError::DuplicateConfiguration,
        ),
    ] {
        let parent = root();
        assert_eq!(
            construct(family, &parent, || seed, true, false).unwrap_err(),
            expected
        );
        assert!(parent.state_store().save().unwrap().is_empty());
        construct(family, &parent, Seed::populated, true, false).unwrap();
    }
}

struct DropProbe {
    parent: DefaultComponentContext,
    dropped: Rc<Cell<bool>>,
    destroy: bool,
}
impl Drop for DropProbe {
    fn drop(&mut self) {
        // These mutable borrows prove captures are released outside obstructing
        // state/retention borrows, including when restoration skips the closure.
        drop(self.parent.state_store());
        drop(self.parent.instance_store());
        if self.destroy {
            self.parent.lifecycle().destroy().unwrap();
        } else if !self.parent.lifecycle().state().is_destroyed() {
            child_slot(
                &self.parent,
                &SlotNavigation::new(),
                None::<u32>,
                SlotOptions {
                    key: "drop-reentry".into(),
                    ..Default::default()
                },
                |_, ctx| ctx,
            )
            .unwrap();
        }
        self.dropped.set(true);
    }
}
fn probe(parent: &DefaultComponentContext, destroy: bool) -> (DropProbe, Rc<Cell<bool>>) {
    let dropped = Rc::new(Cell::new(false));
    (
        DropProbe {
            parent: parent.clone(),
            dropped: dropped.clone(),
            destroy,
        },
        dropped,
    )
}

#[test]
fn captured_resources_drop_before_return_on_evaluated_skipped_and_rejected_paths() {
    for family in FAMILIES {
        for scenario in [
            "evaluated",
            "restored",
            "destroyed",
            "duplicate",
            "malformed",
            "panic",
        ] {
            let parent = root();
            let parent = match scenario {
                "restored" | "malformed" => {
                    construct(family, &parent, Seed::populated, true, false).unwrap();
                    let mut saved = parent.state_store().save().unwrap();
                    if scenario == "malformed" {
                        for bytes in saved.values_mut() {
                            *bytes = vec![0];
                        }
                    }
                    restored(saved)
                }
                "duplicate" => {
                    construct(family, &parent, Seed::populated, true, false).unwrap();
                    parent
                }
                "destroyed" => {
                    parent.lifecycle().destroy().unwrap();
                    parent
                }
                _ => parent,
            };
            let (probe, dropped) = probe(&parent, false);
            let result = catch_unwind(AssertUnwindSafe(|| {
                construct(
                    family,
                    &parent,
                    || {
                        drop(probe);
                        if scenario == "panic" {
                            panic!("initializer panic");
                        }
                        assert_eq!(
                            scenario, "evaluated",
                            "skipped initializer unexpectedly invoked"
                        );
                        Seed::populated()
                    },
                    true,
                    false,
                )
            }));
            assert!(dropped.get(), "{family:?} {scenario}");
            match scenario {
                "evaluated" | "restored" => assert!(result.unwrap().is_ok()),
                "panic" => {
                    assert_eq!(
                        result.unwrap_err().downcast_ref::<&str>(),
                        Some(&"initializer panic")
                    );
                    construct(family, &parent, Seed::populated, true, false).unwrap();
                }
                _ => assert!(result.unwrap().is_err()),
            }
        }
    }
}

#[test]
fn initializer_or_skipped_capture_drop_destroying_parent_cleans_registration() {
    use decompose_rs::router::NavigationError;
    for family in FAMILIES {
        for skip in [false, true] {
            let parent = root();
            let parent = if skip {
                construct(family, &parent, Seed::populated, true, false).unwrap();
                restored(parent.state_store().save().unwrap())
            } else {
                parent
            };
            let (probe, dropped) = probe(&parent, true);
            let result = construct(
                family,
                &parent,
                || {
                    drop(probe);
                    Seed::populated()
                },
                true,
                false,
            );
            assert!(
                matches!(result, Err(NavigationError::Destroyed)),
                "{family:?} skipped={skip}: {result:?}"
            );
            assert!(dropped.get());
            // Saved input is restored on failure, but the failed provider is gone.
            let key = match family {
                Family::Stack => "DefaultChildStack",
                Family::Slot => "DefaultChildSlot",
                Family::Pages => "DefaultChildPages",
                Family::Items => "DefaultChildItems",
                Family::Panels | Family::Panels2 => "DefaultChildPanels",
            };
            assert!(!parent.state_store().is_registered(key).unwrap());
            assert!(!parent.instance_store().contains(key).unwrap());
        }
    }
}

struct ReplaySource<E> {
    navigation: decompose_rs::router::children::SimpleNavigation<E>,
    replay: Option<Rc<E>>,
    subscribed: Cell<bool>,
    cancelled: Rc<Cell<usize>>,
    cancel_panics: bool,
}
impl<E: 'static> ReplaySource<E> {
    fn new(replay: Option<E>) -> Self {
        Self {
            navigation: Default::default(),
            replay: replay.map(Rc::new),
            subscribed: Cell::new(false),
            cancelled: Rc::new(Cell::new(0)),
            cancel_panics: false,
        }
    }
    fn emit(&self, event: E) {
        self.navigation.navigate(event);
    }
}
impl<E: 'static> NavigationSource<E> for ReplaySource<E> {
    fn subscribe(&self, observer: Rc<dyn Fn(Rc<E>)>) -> decompose_rs::value::Subscription {
        self.subscribed.set(true);
        let token = self.navigation.subscribe(observer.clone());
        if let Some(event) = &self.replay {
            observer(event.clone());
        }
        let cancelled = self.cancelled.clone();
        let panics = self.cancel_panics;
        decompose_rs::value::Subscription::from_cancel(move || {
            drop(token);
            cancelled.set(cancelled.get() + 1);
            if panics {
                panic!("custom source cancellation");
            }
        })
    }
}

#[test]
fn initializer_and_cancel_panic_preserve_original_and_retry() {
    const ENV: &str = "DECOMPOSE_LAZY_CANCEL_PANIC_CHILD";
    if std::env::var_os(ENV).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "initializer_and_cancel_panic_preserve_original_and_retry",
                "--nocapture",
            ])
            .env(ENV, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "isolated panic case failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let parent = root();
    construct(Family::Stack, &parent, Seed::populated, true, false).unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = restored(saved.clone());
    let mut source = ReplaySource::<StackEvent<u32>>::new(None);
    source.cancel_panics = true;
    let (probe, dropped) = probe(&parent, false);
    let panic = catch_unwind(AssertUnwindSafe(|| {
        child_stack_with_initial(
            &parent,
            &source,
            || -> Vec<u32> {
                drop(probe);
                panic!("original initializer panic");
            },
            StackOptions::default(),
            |_, ctx| ctx,
        )
    }))
    .err()
    .expect("initializer must panic");
    assert_eq!(
        panic.downcast_ref::<&str>(),
        Some(&"original initializer panic")
    );
    assert!(dropped.get());
    assert_eq!(source.cancelled.get(), 1);
    assert!(
        !parent
            .state_store()
            .is_registered("DefaultChildStack")
            .unwrap()
    );
    assert!(
        !parent
            .instance_store()
            .contains("DefaultChildStack")
            .unwrap()
    );
    // Drop reentry has an independent transient provider, so compare the saved router bytes.
    assert_eq!(
        parent.state_store().save().unwrap()["DefaultChildStack"],
        saved["DefaultChildStack"]
    );
    construct(
        Family::Stack,
        &parent,
        || panic!("lost saved retry input"),
        true,
        false,
    )
    .unwrap();
}

type Log = Rc<RefCell<Vec<String>>>;
fn stack_event(number: u32, log: &Log) -> StackEvent<u32> {
    let log = log.clone();
    StackEvent::with_completion(
        move |_| vec![Rc::new(number)],
        move |r| {
            let result = r.unwrap();
            assert_eq!(*result.new_stack[0], number);
            log.borrow_mut().push(format!("complete {number}"));
        },
    )
}

#[test]
fn subscribe_initializer_factory_events_are_fifo_with_completion_after_creation() {
    let parent = root();
    let log: Log = Default::default();
    let source = Rc::new(ReplaySource::new(Some(stack_event(1, &log))));
    let factory_source = source.clone();
    let factory_log = log.clone();
    let value = child_stack_with_initial(
        &parent,
        &*source,
        || {
            assert!(source.subscribed.get());
            assert!(
                parent
                    .state_store()
                    .is_registered("DefaultChildStack")
                    .unwrap()
            );
            log.borrow_mut().push("initial".into());
            source.emit(stack_event(2, &log));
            assert_eq!(&*log.borrow(), &["initial"]);
            vec![0]
        },
        StackOptions::default(),
        move |c, ctx| {
            factory_log.borrow_mut().push(format!("factory {c}"));
            if *c == 0 {
                factory_source.emit(stack_event(3, &factory_log));
            }
            ctx
        },
    )
    .unwrap();
    assert_eq!(
        &*log.borrow(),
        &[
            "initial",
            "factory 0",
            "factory 1",
            "complete 1",
            "factory 2",
            "complete 2",
            "factory 3",
            "complete 3"
        ]
    );
    assert_eq!(*value.get().active.configuration, 3);
    // A shared event completes separately for each receiver's committed state.
    let second_parent = root();
    let second = child_stack_with_initial(
        &second_parent,
        &*source,
        || vec![5],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    log.borrow_mut().clear();
    let first_view = value.clone();
    let second_view = second.clone();
    let completions = Rc::new(RefCell::new(Vec::new()));
    let out = completions.clone();
    source.emit(StackEvent::with_completion(
        |_| vec![Rc::new(4)],
        move |r| {
            let r = r.unwrap();
            // First callback sees the first published view; second sees both published.
            assert_eq!(*first_view.get().active.configuration, 4);
            out.borrow_mut().push((*r.old_stack[0], *r.new_stack[0]));
            if out.borrow().len() == 2 {
                assert_eq!(*second_view.get().active.configuration, 4);
            }
        },
    ));
    assert_eq!(&*completions.borrow(), &[(3, 4), (1, 4)]);
    parent.lifecycle().destroy().unwrap();
    second_parent.lifecycle().destroy().unwrap();
    assert_eq!(source.cancelled.get(), 2);
}

#[test]
fn decoder_events_share_fifo_without_invoking_initializer() {
    let parent = root();
    construct(
        Family::Stack,
        &parent,
        || Seed {
            members: vec![9],
            ..Seed::populated()
        },
        true,
        false,
    )
    .unwrap();
    let parent = restored(parent.state_store().save().unwrap());
    let log: Log = Default::default();
    let source = Rc::new(ReplaySource::new(Some(stack_event(1, &log))));
    let decoder_source = source.clone();
    let decoder_log = log.clone();
    let factory_source = source.clone();
    let factory_log = log.clone();
    let value = child_stack_with_initial(
        &parent,
        &*source,
        || panic!("restored initializer ran"),
        StackOptions {
            codec: Some(StackCodec::new(encode_list, move |b| {
                decoder_log.borrow_mut().push("decode".into());
                decoder_source.emit(stack_event(2, &decoder_log));
                decode_list(b, false)
            })),
            ..Default::default()
        },
        move |c, ctx| {
            factory_log.borrow_mut().push(format!("factory {c}"));
            if *c == 9 {
                factory_source.emit(stack_event(3, &factory_log));
            }
            ctx
        },
    )
    .unwrap();
    assert_eq!(
        &*log.borrow(),
        &[
            "decode",
            "factory 9",
            "factory 1",
            "complete 1",
            "factory 2",
            "complete 2",
            "factory 3",
            "complete 3"
        ]
    );
    assert_eq!(*value.get().active.configuration, 3);
}

fn panel_event(number: u32, log: &Log) -> PanelsEvent<u32, u32, u32> {
    let log = log.clone();
    PanelsEvent::with_completion(
        move |mut p| {
            p.main = Rc::new(number);
            p
        },
        move |r| {
            let result = r.unwrap();
            assert_eq!(*result.new_panels.main, number);
            log.borrow_mut().push(format!("complete {number}"));
        },
    )
}
#[test]
fn panels_initial_hook_precedes_fifo_pump_and_transition_completions() {
    let parent = root();
    let log: Log = Default::default();
    let source = Rc::new(ReplaySource::new(Some(panel_event(1, &log))));
    let factory_source = source.clone();
    let factory_log = log.clone();
    let hook_source = source.clone();
    let hook_log = log.clone();
    let value = child_panels_with_initial(
        &parent,
        &*source,
        || {
            assert!(source.subscribed.get());
            assert!(
                parent
                    .state_store()
                    .is_registered("DefaultChildPanels")
                    .unwrap()
            );
            log.borrow_mut().push("initial".into());
            source.emit(panel_event(2, &log));
            Panels {
                main: Rc::new(0),
                details: None,
                extra: None,
                mode: ChildPanelsMode::Single,
            }
        },
        PanelsOptions {
            on_state_changed: Some(Rc::new(move |p, old| {
                hook_log
                    .borrow_mut()
                    .push(format!("hook {} {}", p.main, old.is_some()));
                if old.is_none() {
                    hook_source.emit(panel_event(4, &hook_log));
                }
            })),
            ..Default::default()
        },
        move |c, ctx| {
            factory_log.borrow_mut().push(format!("factory {c}"));
            if *c == 0 {
                factory_source.emit(panel_event(3, &factory_log));
            }
            ctx
        },
        |_, ctx| ctx,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(
        &*log.borrow(),
        &[
            "initial",
            "factory 0",
            "hook 0 false",
            "factory 1",
            "hook 1 true",
            "complete 1",
            "factory 2",
            "hook 2 true",
            "complete 2",
            "factory 3",
            "hook 3 true",
            "complete 3",
            "factory 4",
            "hook 4 true",
            "complete 4"
        ]
    );
    assert_eq!(*value.get().main.configuration, 4);
}

macro_rules! family_fifo_test {
    ($name:ident, $event:ty, $builder:ident, $options:ty, $initial:expr, $transform:expr, $result:expr, $view:expr $(, $extra_factory:expr)*) => {
        #[test]
        fn $name() {
            let parent = root();
            let log: Log = Default::default();
            let event = |number, log: &Log| {
                let log = log.clone();
                <$event>::with_completion(move |old| ($transform)(old, number), move |r| {
                    assert_eq!(($result)(r.unwrap()), number);
                    log.borrow_mut().push(format!("complete {number}"));
                })
            };
            let source = Rc::new(ReplaySource::new(Some(event(1, &log))));
            let factory_source = source.clone();
            let factory_log = log.clone();
            let value = $builder(&parent, &*source, || {
                assert!(source.subscribed.get());
                log.borrow_mut().push("initial".into());
                source.emit(event(2, &log));
                $initial
            }, <$options>::default(), move |c, ctx| {
                factory_log.borrow_mut().push(format!("factory {c}"));
                if *c == 0 { factory_source.emit(event(3, &factory_log)); }
                ctx
            } $(, $extra_factory)*).unwrap();
            assert_eq!(&*log.borrow(), &["initial", "factory 0", "factory 1", "complete 1", "factory 2", "complete 2", "factory 3", "complete 3"]);
            assert_eq!(($view)(value.get()), 3);
            parent.lifecycle().destroy().unwrap();
            assert_eq!(source.cancelled.get(), 1);
        }
    };
}
fn active_items(number: u32) -> Items<u32> {
    let mut items = Items::new(vec![number]).unwrap();
    items
        .active_items
        .insert(Rc::new(number), ActiveLifecycleState::Resumed);
    items
}
family_fifo_test!(
    slot_construction_emissions_and_completions_are_fifo,
    SlotEvent<u32>,
    child_slot_with_initial,
    SlotOptions<u32>,
    Some(0),
    |_, number| Some(Rc::new(number)),
    |r: decompose_rs::router::slot::NavigationResult<u32>| *r.new_configuration.unwrap(),
    |v: Rc<ChildSlot<u32, DefaultComponentContext>>| *v.child.as_ref().unwrap().configuration
);
family_fifo_test!(
    pages_construction_emissions_and_completions_are_fifo,
    PagesEvent<u32>,
    child_pages_with_initial,
    PagesOptions<u32>,
    Pages::new(vec![0], 0).unwrap(),
    |_, number| Pages::new(vec![number], 0).unwrap(),
    |r: decompose_rs::router::pages::NavigationResult<u32>| *r.new_pages.items[0],
    |v: Rc<ChildPages<u32, DefaultComponentContext>>| **v.items[0].configuration()
);
family_fifo_test!(
    items_construction_emissions_and_completions_are_fifo,
    ItemsEvent<u32>,
    child_items_with_initial,
    ItemsOptions<u32>,
    active_items(0),
    |_, number| active_items(number),
    |r: decompose_rs::router::items::NavigationResult<u32>| *r.new_items.items[0],
    |v: Rc<ChildItems<u32, DefaultComponentContext>>| *v.items[0]
);
family_fifo_test!(two_role_panels_construction_emissions_and_completions_are_fifo,
    PanelsEvent<u32, u32>, child_panels_without_extra_with_initial, PanelsOptions<u32, u32>,
    Panels { main: Rc::new(0), details: None, extra: None, mode: ChildPanelsMode::Single },
    |mut old: Panels<u32, u32>, number| { old.main = Rc::new(number); old },
    |r: decompose_rs::router::panels::NavigationResult<u32, u32>| *r.new_panels.main,
    |v: Rc<ChildPanels<u32, DefaultComponentContext, u32, DefaultComponentContext>>| *v.main.configuration,
    |_, ctx| ctx);

#[test]
fn invalid_decoded_models_suppress_initialization_and_preserve_retry_input() {
    let parent = root();
    construct(Family::Stack, &parent, Seed::populated, true, false).unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = restored(saved.clone());
    let result = child_stack_with_initial(
        &parent,
        &StackNavigation::new(),
        || panic!("invalid decoded stack initialized"),
        StackOptions {
            codec: Some(StackCodec::new(encode_list, |_| Ok(vec![7, 7, 7]))),
            ..Default::default()
        },
        |_, ctx| ctx,
    );
    assert!(matches!(
        result,
        Err(decompose_rs::router::NavigationError::DuplicateConfiguration)
    ));
    assert_eq!(parent.state_store().save().unwrap(), saved);
    construct(
        Family::Stack,
        &parent,
        || panic!("stack retry reset"),
        true,
        false,
    )
    .unwrap();

    let parent = root();
    construct(Family::Items, &parent, Seed::populated, true, false).unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = restored(saved.clone());
    let result = child_items_with_initial(
        &parent,
        &ItemsNavigation::new(),
        || panic!("invalid decoded items initialized"),
        ItemsOptions {
            codec: Some(ItemsCodec::new(encode_list, |_| Ok(vec![7, 7, 7]))),
            ..Default::default()
        },
        |_, ctx| ctx,
    );
    assert!(result.is_err());
    assert_eq!(parent.state_store().save().unwrap(), saved);
    construct(
        Family::Items,
        &parent,
        || panic!("items retry reset"),
        true,
        false,
    )
    .unwrap();

    let parent = root();
    construct(Family::Pages, &parent, Seed::populated, true, false).unwrap();
    let mut bad = parent.state_store().save().unwrap();
    // DPG1 places the u64 selection after magic and the u64 next-key counter.
    bad.get_mut("DefaultChildPages").unwrap()[12..20].copy_from_slice(&99u64.to_le_bytes());
    let parent = restored(bad.clone());
    for _ in 0..2 {
        assert!(
            construct(
                Family::Pages,
                &parent,
                || panic!("invalid saved selection initialized"),
                true,
                false
            )
            .is_err()
        );
        assert!(
            !parent
                .state_store()
                .is_registered("DefaultChildPages")
                .unwrap()
        );
        assert_eq!(parent.state_store().save().unwrap(), bad);
    }
}
