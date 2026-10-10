#![allow(deprecated)] // Legacy API behavior remains covered during migration.
use decompose_rs::{
    router::{
        self,
        children::{NavigationSource, SimpleNavigation},
        items::*,
        pages::*,
        panels::*,
        slot::*,
        stack::*,
    },
    *,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn root() -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::new(lifecycle)
}
#[derive(PartialEq, Eq)]
struct Config(u8);
struct Component(u8);

// A deliberately non-Clone, borrowed source handle. Only the returned token
// and callback own their registration; factories cannot require a static source.
struct BorrowedSource<'a, E> {
    transport: &'a SimpleNavigation<E>,
    connections: Rc<Cell<usize>>,
    on_subscribe: Option<Rc<E>>,
    panic_on_cancel: bool,
}
impl<'a, E> BorrowedSource<'a, E> {
    fn new(transport: &'a SimpleNavigation<E>) -> Self {
        Self {
            transport,
            connections: Rc::new(Cell::new(0)),
            on_subscribe: None,
            panic_on_cancel: false,
        }
    }
}
impl<E: 'static> NavigationSource<E> for BorrowedSource<'_, E> {
    fn subscribe(&self, observer: Rc<dyn Fn(Rc<E>)>) -> decompose_rs::value::Subscription {
        let token = self.transport.subscribe(observer.clone());
        self.connections.set(self.connections.get() + 1);
        let connections = self.connections.clone();
        let panic_on_cancel = self.panic_on_cancel;
        let owned = decompose_rs::value::Subscription::from_cancel(move || {
            token.cancel();
            connections.set(connections.get() - 1);
            assert!(!panic_on_cancel, "custom source cancellation");
        });
        if let Some(event) = &self.on_subscribe {
            observer(event.clone());
        }
        owned
    }
}

#[test]
fn public_event_callbacks_clone_without_payload_bounds() {
    struct State(u8);
    struct ResultValue(u8);
    let output = Rc::new(Cell::new(0));
    let captured = output.clone();
    let event = router::NavigationEvent::with_completion(
        |state: State| State(state.0 + 1),
        move |result: Result<ResultValue, router::NavigationError>| captured.set(result.unwrap().0),
    );
    let clone = event.clone();
    assert_eq!(clone.transform(State(1)).0, 2);
    clone.complete(Ok(ResultValue(3)));
    assert_eq!(output.get(), 3);
    router::NavigationEvent::<State, ResultValue>::new(|state| state)
        .complete(Err(router::NavigationError::Destroyed));
}

#[test]
fn dyn_custom_sources_connect_every_standard_factory_with_nonclone_payloads() {
    let parent = root();
    let source = SimpleNavigation::<StackEvent<Config>>::new();
    let custom = BorrowedSource::new(&source);
    let erased: &dyn NavigationSource<StackEvent<Config>> = &custom;
    let stack = child_stack::<_, _, _>(
        &parent,
        erased,
        vec![Config(1)],
        StackOptions::default(),
        |c, _| Component(c.0),
    )
    .unwrap();
    source.navigate(StackEvent::new(|mut state| {
        state.push(Rc::new(Config(2)));
        state
    }));
    assert_eq!(stack.get().active.instance.0, 2);

    let source = SimpleNavigation::<SlotEvent<Config>>::new();
    let custom = BorrowedSource::new(&source);
    let erased: &dyn NavigationSource<SlotEvent<Config>> = &custom;
    let slot = child_slot::<_, _, _>(&parent, erased, None, SlotOptions::default(), |c, _| {
        Component(c.0)
    })
    .unwrap();
    source.navigate(SlotEvent::new(|_| Some(Rc::new(Config(3)))));
    assert_eq!(slot.get().child.as_ref().unwrap().instance.0, 3);

    let source = SimpleNavigation::<PagesEvent<Config>>::new();
    let custom = BorrowedSource::new(&source);
    let erased: &dyn NavigationSource<PagesEvent<Config>> = &custom;
    let pages = child_pages::<_, _, _>(
        &parent,
        erased,
        Pages::new(vec![Config(4), Config(5)], 0).unwrap(),
        PagesOptions::default(),
        |c, _| Component(c.0),
    )
    .unwrap();
    source.navigate(PagesEvent::new(|mut state| {
        state.selected_index = Some(1);
        state
    }));
    assert_eq!(pages.get().selected().unwrap().configuration().0, 5);

    let source = SimpleNavigation::<PanelsEvent<Config, Config, Config>>::new();
    let custom = BorrowedSource::new(&source);
    let erased: &dyn NavigationSource<PanelsEvent<Config, Config, Config>> = &custom;
    let panels = child_panels::<_, _, _, _, _, _, _>(
        &parent,
        erased,
        Panels::new(Config(6)),
        PanelsOptions::default(),
        |c, _| Component(c.0),
        |c, _| Component(c.0),
        |c, _| Component(c.0),
    )
    .unwrap();
    source.navigate(PanelsEvent::new(|mut state| {
        state.extra = Some(Rc::new(Config(7)));
        state
    }));
    assert_eq!(panels.get().extra.as_ref().unwrap().instance.0, 7);

    let source = SimpleNavigation::<ItemsEvent<Config>>::new();
    let custom = BorrowedSource::new(&source);
    let erased: &dyn NavigationSource<ItemsEvent<Config>> = &custom;
    let items = child_items::<_, _, _>(
        &parent,
        erased,
        Items::new(vec![Config(8)]).unwrap(),
        ItemsOptions::default(),
        |c, _| Component(c.0),
    )
    .unwrap();
    source.navigate(ItemsEvent::new(|mut state: Items<Config>| {
        state
            .active_items
            .insert(state.items[0].clone(), ActiveLifecycleState::Resumed);
        state
    }));
    assert_eq!(
        items
            .get()
            .active_items
            .get(&Config(8))
            .unwrap()
            .child
            .instance
            .0,
        8
    );
    assert_eq!(items.get_or_create(&Config(8)).unwrap().0, 8);
}

#[test]
fn two_role_panels_accept_dyn_source_with_existing_generic_count() {
    let parent = root();
    let source = SimpleNavigation::<PanelsEvent<Config, Config>>::new();
    let custom = BorrowedSource::new(&source);
    let erased: &dyn NavigationSource<PanelsEvent<Config, Config>> = &custom;
    let panels = child_panels_without_extra::<_, _, _, _, _>(
        &parent,
        erased,
        Panels::new(Config(1)),
        PanelsOptions::default(),
        |c, _| Component(c.0),
        |c, _| Component(c.0),
    )
    .unwrap();
    source.navigate(PanelsEvent::new(|mut state| {
        state.details = Some(Rc::new(Config(2)));
        state
    }));
    assert_eq!(panels.get().details.as_ref().unwrap().instance.0, 2);
}

struct StackWrapper(StackNavigation<Config>);
impl StackNavigator<Config> for StackWrapper {
    fn navigate_with(
        &self,
        transformer: impl Fn(Vec<Rc<Config>>) -> Vec<Rc<Config>> + 'static,
        completion: impl Fn(
            Result<decompose_rs::router::stack::NavigationResult<Config>, router::NavigationError>,
        ) + 'static,
    ) {
        self.0.navigate_with(transformer, completion);
    }
}
struct SlotWrapper(SlotNavigation<Config>);
impl SlotNavigator<Config> for SlotWrapper {
    fn navigate_with(
        &self,
        transformer: impl Fn(Option<Rc<Config>>) -> Option<Rc<Config>> + 'static,
        completion: impl Fn(
            Result<decompose_rs::router::slot::NavigationResult<Config>, router::NavigationError>,
        ) + 'static,
    ) {
        self.0.navigate_with(transformer, completion);
    }
}
struct PagesWrapper(PagesNavigation<Config>);
impl PagesNavigator<Config> for PagesWrapper {
    fn navigate_with(
        &self,
        transformer: impl Fn(Pages<Config>) -> Pages<Config> + 'static,
        completion: impl Fn(
            Result<decompose_rs::router::pages::NavigationResult<Config>, router::NavigationError>,
        ) + 'static,
    ) {
        self.0.navigate_with(transformer, completion);
    }
}
struct PanelsWrapper(PanelsNavigation<Config, Config, Config>);
impl PanelsNavigator<Config, Config, Config> for PanelsWrapper {
    fn navigate_with(
        &self,
        transformer: impl Fn(Panels<Config, Config, Config>) -> Panels<Config, Config, Config> + 'static,
        completion: impl Fn(
            Result<
                decompose_rs::router::panels::NavigationResult<Config, Config, Config>,
                router::NavigationError,
            >,
        ) + 'static,
    ) {
        self.0.navigate_with(transformer, completion);
    }
}
struct ItemsWrapper(ItemsNavigation<Config>);
impl ItemsNavigator<Config> for ItemsWrapper {
    fn navigate_with(
        &self,
        transformer: impl Fn(Items<Config>) -> Items<Config> + 'static,
        completion: impl Fn(
            Result<decompose_rs::router::items::NavigationResult<Config>, router::NavigationError>,
        ) + 'static,
    ) {
        self.0.navigate_with(transformer, completion);
    }
}

#[test]
fn custom_navigators_inherit_standard_helpers_and_local_items_still_work() {
    let parent = root();
    let stack = StackWrapper(StackNavigation::new());
    let value = child_stack(
        &parent,
        &stack.0,
        vec![Config(1)],
        StackOptions::default(),
        |c, _| Component(c.0),
    )
    .unwrap();
    stack.push_new(Config(2));
    stack.push_new(Config(2));
    stack.push_to_front(Config(1));
    assert_eq!(
        value
            .get()
            .items()
            .map(|c| c.configuration.0)
            .collect::<Vec<_>>(),
        [2, 1]
    );
    stack.pop_to_first();
    stack.replace_current(Config(3));
    assert_eq!(value.get().active.instance.0, 3);
    let slot = SlotWrapper(SlotNavigation::new());
    let value = child_slot(&parent, &slot.0, None, SlotOptions::default(), |c, _| {
        Component(c.0)
    })
    .unwrap();
    slot.activate(Config(4));
    slot.dismiss_with(|result| assert!(result.unwrap()));
    assert!(value.get().child.is_none());
    let pages = PagesWrapper(PagesNavigation::new());
    let value = child_pages(
        &parent,
        &pages.0,
        Pages::new(vec![Config(5), Config(6)], 0).unwrap(),
        PagesOptions::default(),
        |c, _| Component(c.0),
    )
    .unwrap();
    pages.select_next(true);
    pages.select_prev(false);
    pages.select_last();
    assert_eq!(value.get().selected_index, Some(1));
    pages.clear();
    assert!(value.get().items.is_empty());
    let panels = PanelsWrapper(PanelsNavigation::new());
    let value = child_panels(
        &parent,
        &panels.0,
        Panels::new(Config(7)),
        PanelsOptions::default(),
        |c, _| Component(c.0),
        |c, _| Component(c.0),
        |c, _| Component(c.0),
    )
    .unwrap();
    panels.activate_details(Config(8));
    panels.activate_extra(Config(9));
    panels.pop();
    panels.set_mode(ChildPanelsMode::Triple);
    assert!(value.get().extra.is_none());
    assert_eq!(value.get().details.as_ref().unwrap().instance.0, 8);
    let items = ItemsWrapper(ItemsNavigation::new());
    let value = child_items(
        &parent,
        &items.0,
        Items::new(vec![Config(10)]).unwrap(),
        ItemsOptions::default(),
        |c, _| Component(c.0),
    )
    .unwrap();
    items.set_active_items(|_| {
        [(Rc::new(Config(10)), ActiveLifecycleState::Resumed)]
            .into_iter()
            .collect()
    });
    assert_eq!(
        value
            .get()
            .active_items
            .get(&Config(10))
            .unwrap()
            .child
            .instance
            .0,
        10
    );
    value.set_items(|mut c| {
        c.push(Rc::new(Config(11)));
        c
    });
    assert_eq!(value.get_or_create(&Config(11)).unwrap().0, 11);
}

fn assert_shared_event<E: 'static>(source: &impl NavigationSource<E>, emit: impl FnOnce()) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    let _first = source.subscribe(Rc::new(move |event| {
        output.borrow_mut().push(Rc::as_ptr(&event))
    }));
    let output = seen.clone();
    let _second = source.subscribe(Rc::new(move |event| {
        output.borrow_mut().push(Rc::as_ptr(&event))
    }));
    emit();
    assert_eq!(seen.borrow().len(), 2);
    assert_eq!(seen.borrow()[0], seen.borrow()[1]);
}

#[test]
fn every_builtin_implements_public_source_with_shared_rc_identity() {
    struct NoEq;
    let stack = StackNavigation::<NoEq>::new();
    assert_shared_event(&stack, || stack.navigate(|s| s));
    let slot = SlotNavigation::<NoEq>::new();
    assert_shared_event(&slot, || slot.navigate(|s| s));
    let pages = PagesNavigation::<NoEq>::new();
    assert_shared_event(&pages, || pages.navigate(|s| s));
    let panels = PanelsNavigation::<NoEq, NoEq, NoEq>::new();
    assert_shared_event(&panels, || panels.navigate(|s| s));
    let items = ItemsNavigation::<NoEq>::new();
    assert_shared_event(&items, || items.navigate(|s| s));
}

#[test]
fn builtin_public_receivers_apply_own_state_complete_in_order_and_nested_events_are_fifo() {
    let navigation = StackNavigation::<Config>::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let identities = Rc::new(RefCell::new(Vec::new()));
    let first_state = Rc::new(RefCell::new(vec![Rc::new(Config(10))]));
    let second_state = Rc::new(RefCell::new(vec![Rc::new(Config(20))]));
    let mut tokens = Vec::new();
    for (id, state) in [(1, first_state.clone()), (2, second_state.clone())] {
        let output = seen.clone();
        let identities = identities.clone();
        let nested = navigation.clone();
        tokens.push(NavigationSource::subscribe(
            &navigation,
            Rc::new(move |event: Rc<StackEvent<Config>>| {
                let old_stack = state.borrow().clone();
                let new_stack = event.transform(old_stack.clone());
                let last = new_stack.last().unwrap().0;
                *state.borrow_mut() = new_stack.clone();
                output.borrow_mut().push((id, last));
                identities.borrow_mut().push((last, Rc::as_ptr(&event)));
                if id == 1 && last == 1 {
                    nested.push(Config(2));
                }
                event.complete(Ok(decompose_rs::router::stack::NavigationResult {
                    old_stack,
                    new_stack,
                }));
            }),
        ));
    }
    let completed = Rc::new(RefCell::new(Vec::new()));
    let output = completed.clone();
    navigation.navigate_with(
        |mut state| {
            state.push(Rc::new(Config(1)));
            state
        },
        move |result| {
            let result = result.unwrap();
            output
                .borrow_mut()
                .push((result.old_stack[0].0, result.new_stack.last().unwrap().0));
        },
    );
    assert_eq!(*seen.borrow(), [(1, 1), (2, 1), (1, 2), (2, 2)]);
    assert_eq!(*completed.borrow(), [(10, 1), (20, 1)]);
    let identities = identities.borrow();
    assert_eq!(identities[0].1, identities[1].1);
    assert_eq!(identities[2].1, identities[3].1);
    assert_eq!(
        first_state.borrow().iter().map(|c| c.0).collect::<Vec<_>>(),
        [10, 1, 2]
    );
    assert_eq!(
        second_state
            .borrow()
            .iter()
            .map(|c| c.0)
            .collect::<Vec<_>>(),
        [20, 1, 2]
    );
}

#[test]
fn public_sources_discard_disconnected_events_without_replay_and_release_captures() {
    let navigation = SlotNavigation::<Config>::new();
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    navigation.navigate_with(
        move |s| {
            let _ = &capture;
            s
        },
        |_| panic!("disconnected event must not complete"),
    );
    assert!(weak.upgrade().is_none());
    let count = Rc::new(Cell::new(0));
    let output = count.clone();
    let token =
        NavigationSource::subscribe(&navigation, Rc::new(move |_| output.set(output.get() + 1)));
    assert_eq!(count.get(), 0);
    navigation.dismiss();
    assert_eq!(count.get(), 1);
    token.cancel();
    navigation.dismiss();
    assert_eq!(count.get(), 1);
}

#[test]
fn public_cancellation_skips_captured_receiver_and_late_receiver_starts_next_round() {
    let navigation = StackNavigation::<u8>::new();
    let second = Rc::new(RefCell::new(None::<decompose_rs::value::Subscription>));
    let late = Rc::new(RefCell::new(None));
    let log = Rc::new(RefCell::new(Vec::new()));
    let output = log.clone();
    let cancel = second.clone();
    let slot = late.clone();
    let nested = navigation.clone();
    let _first = NavigationSource::subscribe(
        &navigation,
        Rc::new(move |event: Rc<StackEvent<u8>>| {
            let value = *event.transform(vec![])[0];
            output.borrow_mut().push((1, value));
            if value == 1 {
                cancel.borrow().as_ref().unwrap().cancel();
                let output = output.clone();
                *slot.borrow_mut() = Some(NavigationSource::subscribe(
                    &nested,
                    Rc::new(move |event: Rc<StackEvent<u8>>| {
                        output.borrow_mut().push((3, *event.transform(vec![])[0]))
                    }),
                ));
                nested.push(2);
            }
        }),
    );
    let output = log.clone();
    *second.borrow_mut() = Some(NavigationSource::subscribe(
        &navigation,
        Rc::new(move |_| output.borrow_mut().push((2, 0))),
    ));
    navigation.push(1);
    assert_eq!(*log.borrow(), [(1, 1), (1, 2), (3, 2)]);
}

#[test]
fn public_observer_panic_clears_queue_recovers_and_drops_captures_outside_borrows() {
    struct EmitOnDrop(StackNavigation<u8>, Rc<Cell<usize>>);
    impl Drop for EmitOnDrop {
        fn drop(&mut self) {
            self.1.set(self.1.get() + 1);
            self.0.push(9);
        }
    }
    let navigation = StackNavigation::<u8>::new();
    let nested = navigation.clone();
    let dropped = Rc::new(Cell::new(0));
    let count = dropped.clone();
    let _first = NavigationSource::subscribe(
        &navigation,
        Rc::new(move |event: Rc<StackEvent<u8>>| {
            if *event.transform(vec![])[0] == 1 {
                let capture = EmitOnDrop(nested.clone(), count.clone());
                nested.navigate(move |_| {
                    let _ = &capture;
                    vec![Rc::new(2)]
                });
                panic!("public observer panic");
            }
        }),
    );
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    let _second = NavigationSource::subscribe(
        &navigation,
        Rc::new(move |event: Rc<StackEvent<u8>>| {
            output.borrow_mut().push(*event.transform(vec![])[0])
        }),
    );
    let panic =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| navigation.push(1))).unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"public observer panic"));
    assert_eq!(dropped.get(), 1);
    navigation.push(3);
    assert_eq!(*seen.borrow(), [9, 3]);
}

#[test]
fn custom_source_errors_preserve_receiver_state_and_completions_follow_publication() {
    let parent = root();
    let source = SimpleNavigation::<StackEvent<Config>>::new();
    let custom = BorrowedSource::new(&source);
    let value = child_stack(
        &parent,
        &custom,
        vec![Config(1)],
        StackOptions::default(),
        |c, _| Component(c.0),
    )
    .unwrap();
    let before = value.get();
    let checked = value.clone();
    source.navigate(StackEvent::with_completion(
        |_| vec![],
        move |result| {
            assert!(matches!(result, Err(router::NavigationError::EmptyStack)));
            assert!(Rc::ptr_eq(&before, &checked.get()));
        },
    ));
    let checked = value.clone();
    source.navigate(StackEvent::with_completion(
        |mut s| {
            s.push(Rc::new(Config(2)));
            s
        },
        move |r| {
            assert_eq!(r.unwrap().new_stack.last().unwrap().0, 2);
            assert_eq!(checked.get().active.instance.0, 2);
        },
    ));
    let source = SimpleNavigation::<PagesEvent<Config>>::new();
    let custom = BorrowedSource::new(&source);
    let value = child_pages(
        &parent,
        &custom,
        Pages::new(vec![Config(3)], 0).unwrap(),
        PagesOptions::default(),
        |c, _| Component(c.0),
    )
    .unwrap();
    let before = value.get();
    let checked = value.clone();
    source.navigate(PagesEvent::with_completion(
        |mut s| {
            s.selected_index = Some(99);
            s
        },
        move |r| {
            assert!(matches!(
                r,
                Err(router::NavigationError::InvalidSelection {
                    selected_index: Some(99),
                    item_count: 1
                })
            ));
            assert!(Rc::ptr_eq(&before, &checked.get()));
        },
    ));
}

fn completion<R>(
    tag: u8,
    log: &Rc<RefCell<Vec<u8>>>,
) -> impl Fn(Result<R, router::NavigationError>) + 'static {
    let log = log.clone();
    move |result| {
        assert!(result.is_ok());
        log.borrow_mut().push(tag);
    }
}
fn active_config(id: u8) -> Items<Config> {
    let configuration = Rc::new(Config(id));
    Items {
        items: vec![configuration.clone()],
        active_items: [(configuration, ActiveLifecycleState::Resumed)]
            .into_iter()
            .collect(),
    }
}

#[test]
fn all_families_buffer_subscribe_and_initial_factory_events_before_completion() {
    let parent = root();
    let log = Rc::new(RefCell::new(Vec::new()));
    let source = SimpleNavigation::<StackEvent<Config>>::new();
    let mut custom = BorrowedSource::new(&source);
    custom.on_subscribe = Some(Rc::new(StackEvent::with_completion(
        |_| vec![Rc::new(Config(1))],
        completion(1, &log),
    )));
    let nested = source.clone();
    let output = log.clone();
    let value = child_stack(
        &parent,
        &custom,
        vec![Config(0)],
        StackOptions::default(),
        move |c, _| {
            if c.0 == 0 {
                assert!(output.borrow().is_empty());
                nested.navigate(StackEvent::with_completion(
                    |_| vec![Rc::new(Config(2))],
                    completion(2, &output),
                ));
            }
            Component(c.0)
        },
    )
    .unwrap();
    assert_eq!(*log.borrow(), [1, 2]);
    assert_eq!(value.get().active.instance.0, 2);

    let log = Rc::new(RefCell::new(Vec::new()));
    let source = SimpleNavigation::<SlotEvent<Config>>::new();
    let mut custom = BorrowedSource::new(&source);
    custom.on_subscribe = Some(Rc::new(SlotEvent::with_completion(
        |_| Some(Rc::new(Config(1))),
        completion(1, &log),
    )));
    let nested = source.clone();
    let output = log.clone();
    let value = child_slot(
        &parent,
        &custom,
        Some(Config(0)),
        SlotOptions::default(),
        move |c, _| {
            if c.0 == 0 {
                assert!(output.borrow().is_empty());
                nested.navigate(SlotEvent::with_completion(
                    |_| Some(Rc::new(Config(2))),
                    completion(2, &output),
                ));
            }
            Component(c.0)
        },
    )
    .unwrap();
    assert_eq!(*log.borrow(), [1, 2]);
    assert_eq!(value.get().child.as_ref().unwrap().instance.0, 2);

    let log = Rc::new(RefCell::new(Vec::new()));
    let source = SimpleNavigation::<PagesEvent<Config>>::new();
    let mut custom = BorrowedSource::new(&source);
    custom.on_subscribe = Some(Rc::new(PagesEvent::with_completion(
        |_| Pages::new(vec![Config(1)], 0).unwrap(),
        completion(1, &log),
    )));
    let nested = source.clone();
    let output = log.clone();
    let value = child_pages(
        &parent,
        &custom,
        Pages::new(vec![Config(0)], 0).unwrap(),
        PagesOptions::default(),
        move |c, _| {
            if c.0 == 0 {
                assert!(output.borrow().is_empty());
                nested.navigate(PagesEvent::with_completion(
                    |_| Pages::new(vec![Config(2)], 0).unwrap(),
                    completion(2, &output),
                ));
            }
            Component(c.0)
        },
    )
    .unwrap();
    assert_eq!(*log.borrow(), [1, 2]);
    assert_eq!(value.get().selected().unwrap().configuration().0, 2);

    let log = Rc::new(RefCell::new(Vec::new()));
    let source = SimpleNavigation::<PanelsEvent<Config, Config, Config>>::new();
    let mut custom = BorrowedSource::new(&source);
    custom.on_subscribe = Some(Rc::new(PanelsEvent::with_completion(
        |_| Panels::new(Config(1)),
        completion(1, &log),
    )));
    let nested = source.clone();
    let output = log.clone();
    let hook_source = source.clone();
    let hook_log = log.clone();
    let changes = Rc::new(RefCell::new(Vec::new()));
    let changed = changes.clone();
    let options = PanelsOptions {
        on_state_changed: Some(Rc::new(
            move |state: &Panels<Config, Config, Config>, old| {
                changed
                    .borrow_mut()
                    .push((state.main.0, old.map(|s| s.main.0)));
                if old.is_none() {
                    assert!(hook_log.borrow().is_empty());
                    hook_source.navigate(PanelsEvent::with_completion(
                        |_| Panels::new(Config(3)),
                        completion(3, &hook_log),
                    ));
                }
            },
        )),
        ..Default::default()
    };
    let value = child_panels(
        &parent,
        &custom,
        Panels::new(Config(0)),
        options,
        move |c, _| {
            if c.0 == 0 {
                assert!(output.borrow().is_empty());
                nested.navigate(PanelsEvent::with_completion(
                    |_| Panels::new(Config(2)),
                    completion(2, &output),
                ));
            }
            Component(c.0)
        },
        |c, _| Component(c.0),
        |c, _| Component(c.0),
    )
    .unwrap();
    assert_eq!(*log.borrow(), [1, 2, 3]);
    assert_eq!(value.get().main.instance.0, 3);
    assert_eq!(
        *changes.borrow(),
        [(0, None), (1, Some(0)), (2, Some(1)), (3, Some(2))]
    );

    let log = Rc::new(RefCell::new(Vec::new()));
    let source = SimpleNavigation::<ItemsEvent<Config>>::new();
    let mut custom = BorrowedSource::new(&source);
    custom.on_subscribe = Some(Rc::new(ItemsEvent::with_completion(
        |_| active_config(1),
        completion(1, &log),
    )));
    let nested = source.clone();
    let output = log.clone();
    let value = child_items(
        &parent,
        &custom,
        active_config(0),
        ItemsOptions::default(),
        move |c, _| {
            if c.0 == 0 {
                assert!(output.borrow().is_empty());
                nested.navigate(ItemsEvent::with_completion(
                    |_| active_config(2),
                    completion(2, &output),
                ));
            }
            Component(c.0)
        },
    )
    .unwrap();
    assert_eq!(*log.borrow(), [1, 2]);
    assert_eq!(
        value
            .get()
            .active_items
            .get(&Config(2))
            .unwrap()
            .child
            .instance
            .0,
        2
    );
}

#[test]
fn back_uses_receiver_policy_for_all_families_without_a_navigator_or_source_event() {
    let parent = root();
    let transport = SimpleNavigation::<StackEvent<Config>>::new();
    let source = BorrowedSource::new(&transport);
    let value = child_stack(
        &parent,
        &source,
        vec![Config(1), Config(2)],
        StackOptions {
            handle_back_button: true,
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(source.connections.get(), 1);
    assert!(parent.back_handler().back());
    assert_eq!(value.get().active.configuration.0, 1);

    let parent = root();
    let transport = SimpleNavigation::<SlotEvent<Config>>::new();
    let source = BorrowedSource::new(&transport);
    let value = child_slot(
        &parent,
        &source,
        Some(Config(1)),
        SlotOptions {
            handle_back_button: true,
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(source.connections.get(), 1);
    assert!(parent.back_handler().back());
    assert!(value.get().child.is_none());

    let parent = root();
    let transport = SimpleNavigation::<PagesEvent<Config>>::new();
    let source = BorrowedSource::new(&transport);
    let value = child_pages(
        &parent,
        &source,
        Pages::new(vec![Config(1), Config(2)], 1).unwrap(),
        PagesOptions {
            handle_back_button: true,
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(source.connections.get(), 1);
    assert!(parent.back_handler().back());
    assert_eq!(value.get().selected_index, Some(0));

    let parent = root();
    let transport = SimpleNavigation::<PanelsEvent<Config, Config, Config>>::new();
    let source = BorrowedSource::new(&transport);
    let initial = Panels {
        main: Rc::new(Config(1)),
        details: Some(Rc::new(Config(2))),
        extra: Some(Rc::new(Config(3))),
        mode: ChildPanelsMode::Single,
    };
    let value = child_panels(
        &parent,
        &source,
        initial,
        PanelsOptions {
            handle_back_button: true,
            ..Default::default()
        },
        |_, ctx| ctx,
        |_, ctx| ctx,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(source.connections.get(), 1);
    assert!(parent.back_handler().back());
    assert!(value.get().extra.is_none());
    assert!(parent.back_handler().back());
    assert!(value.get().details.is_none());

    let parent = root();
    let transport = SimpleNavigation::<ItemsEvent<Config>>::new();
    let source = BorrowedSource::new(&transport);
    let handled = Rc::new(Cell::new(0));
    let count = handled.clone();
    let value = child_items(
        &parent,
        &source,
        active_config(1),
        ItemsOptions::default(),
        move |_, ctx| {
            let count = count.clone();
            ctx.back_handler()
                .register(0, true, move |_, _| count.set(count.get() + 1))
                .unwrap();
            ctx
        },
    )
    .unwrap();
    assert_eq!(source.connections.get(), 1);
    assert!(parent.back_handler().back());
    assert_eq!(handled.get(), 1);
    assert_eq!(value.get().active_items.len(), 1);
}

#[test]
fn dropping_custom_handles_does_not_disconnect_and_parent_destruction_cancels_every_registration() {
    let parent = root();
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let stack_transport = SimpleNavigation::<StackEvent<Config>>::new();
    let source = BorrowedSource::new(&stack_transport);
    let stack_connections = source.connections.clone();
    let keep = capture.clone();
    let stack = child_stack(
        &parent,
        &source,
        vec![Config(1)],
        StackOptions::default(),
        move |_, ctx| {
            let _ = &keep;
            ctx
        },
    )
    .unwrap();
    drop(source);
    stack_transport.navigate(StackEvent::new(|mut s| {
        s.push(Rc::new(Config(2)));
        s
    }));
    assert_eq!(stack.get().active.configuration.0, 2);

    let slot_transport = SimpleNavigation::<SlotEvent<Config>>::new();
    let source = BorrowedSource::new(&slot_transport);
    let slot_connections = source.connections.clone();
    let keep = capture.clone();
    let slot = child_slot(
        &parent,
        &source,
        Some(Config(1)),
        SlotOptions::default(),
        move |_, ctx| {
            let _ = &keep;
            ctx
        },
    )
    .unwrap();
    let pages_transport = SimpleNavigation::<PagesEvent<Config>>::new();
    let source = BorrowedSource::new(&pages_transport);
    let pages_connections = source.connections.clone();
    let keep = capture.clone();
    let pages = child_pages(
        &parent,
        &source,
        Pages::new(vec![Config(1)], 0).unwrap(),
        PagesOptions::default(),
        move |_, ctx| {
            let _ = &keep;
            ctx
        },
    )
    .unwrap();
    let panels_transport = SimpleNavigation::<PanelsEvent<Config, Config>>::new();
    let source = BorrowedSource::new(&panels_transport);
    let panels_connections = source.connections.clone();
    let keep = capture.clone();
    let panels = child_panels_without_extra(
        &parent,
        &source,
        Panels::new(Config(1)),
        PanelsOptions::default(),
        move |_, ctx| {
            let _ = &keep;
            ctx
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let items_transport = SimpleNavigation::<ItemsEvent<Config>>::new();
    let source = BorrowedSource::new(&items_transport);
    let items_connections = source.connections.clone();
    let keep = capture.clone();
    let items = child_items(
        &parent,
        &source,
        active_config(1),
        ItemsOptions::default(),
        move |_, ctx| {
            let _ = &keep;
            ctx
        },
    )
    .unwrap();
    drop(capture);
    assert!(weak.upgrade().is_some());
    parent.lifecycle().destroy().unwrap();
    for count in [
        stack_connections,
        slot_connections,
        pages_connections,
        panels_connections,
        items_connections,
    ] {
        assert_eq!(count.get(), 0);
    }
    assert!(weak.upgrade().is_none());
    assert!(
        stack
            .get()
            .active
            .instance
            .lifecycle()
            .state()
            .is_destroyed()
    );
    assert!(
        slot.get()
            .child
            .as_ref()
            .unwrap()
            .instance
            .lifecycle()
            .state()
            .is_destroyed()
    );
    assert!(
        pages
            .get()
            .selected()
            .unwrap()
            .instance()
            .unwrap()
            .lifecycle()
            .state()
            .is_destroyed()
    );
    assert!(
        panels
            .get()
            .main
            .instance
            .lifecycle()
            .state()
            .is_destroyed()
    );
    assert!(
        items
            .get()
            .active_items
            .get(&Config(1))
            .unwrap()
            .child
            .instance
            .lifecycle()
            .state()
            .is_destroyed()
    );
    assert_eq!(parent.back_handler().handler_count(), 0);
    assert!(parent.state_store().save().unwrap().is_empty());
}

fn stack_codec() -> StackCodec<Config> {
    StackCodec::new(
        |state: &[Rc<Config>]| Ok(state.iter().map(|c| c.0).collect()),
        |bytes| Ok(bytes.iter().copied().map(Config).collect()),
    )
}
fn saved_stack() -> std::collections::BTreeMap<String, Vec<u8>> {
    let parent = root();
    let value = child_stack(
        &parent,
        &StackNavigation::new(),
        vec![Config(1)],
        StackOptions {
            codec: Some(stack_codec()),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    value
        .get()
        .active
        .instance
        .state_store()
        .register("child-state", || vec![42])
        .unwrap();
    parent.state_store().save().unwrap()
}
fn restored(saved: std::collections::BTreeMap<String, Vec<u8>>) -> DefaultComponentContext {
    DefaultComponentContext::with_state_store(
        root().lifecycle(),
        StateStore::from(StateKeeper::with_restored(saved)),
    )
}
fn assert_clean(parent: &DefaultComponentContext, key: &str) {
    assert!(!parent.state_store().is_registered(key).unwrap());
    assert!(parent.instance_store().is_empty().unwrap());
    assert_eq!(parent.back_handler().handler_count(), 0);
}

#[test]
fn invalid_initial_states_cancel_custom_observers_release_reservations_and_allow_retry() {
    let parent = root();
    let transport = SimpleNavigation::<StackEvent<Config>>::new();
    let source = BorrowedSource::new(&transport);
    assert!(matches!(
        child_stack(&parent, &source, vec![], StackOptions::default(), |_, _| {
            Component(0)
        }),
        Err(router::NavigationError::EmptyStack)
    ));
    assert_eq!(source.connections.get(), 0);
    assert_clean(&parent, "DefaultChildStack");
    child_stack(
        &parent,
        &source,
        vec![Config(1)],
        StackOptions::default(),
        |_, _| Component(1),
    )
    .unwrap();

    let parent = root();
    let transport = SimpleNavigation::<PagesEvent<Config>>::new();
    let source = BorrowedSource::new(&transport);
    let invalid = Pages {
        items: vec![Rc::new(Config(1))],
        selected_index: None,
    };
    assert!(matches!(
        child_pages(
            &parent,
            &source,
            invalid,
            PagesOptions::default(),
            |_, _| Component(0)
        ),
        Err(router::NavigationError::InvalidSelection { .. })
    ));
    assert_eq!(source.connections.get(), 0);
    assert_clean(&parent, "DefaultChildPages");
    child_pages(
        &parent,
        &source,
        Pages::new(vec![Config(1)], 0).unwrap(),
        PagesOptions::default(),
        |_, _| Component(1),
    )
    .unwrap();

    let parent = root();
    let transport = SimpleNavigation::<ItemsEvent<Config>>::new();
    let source = BorrowedSource::new(&transport);
    let invalid = Items {
        items: vec![Rc::new(Config(1)), Rc::new(Config(1))],
        active_items: ItemMap::new(),
    };
    assert!(matches!(
        child_items(
            &parent,
            &source,
            invalid,
            ItemsOptions::default(),
            |_, _| Component(0)
        ),
        Err(router::NavigationError::DuplicateConfiguration)
    ));
    assert_eq!(source.connections.get(), 0);
    assert_clean(&parent, "DefaultChildItems");
    child_items(
        &parent,
        &source,
        active_config(1),
        ItemsOptions::default(),
        |_, _| Component(1),
    )
    .unwrap();
}

#[test]
fn construction_panics_preserve_original_unwind_even_when_custom_cancellation_panics() {
    const MODE: &str = "DECOMPOSE_STANDARD_EXTENSION_PANIC_CASE";
    let Ok(mode) = std::env::var(MODE) else {
        for mode in [
            "decoder",
            "slot_factory",
            "panels_hook",
            "restored_stack_factory",
        ] {
            let output = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", "construction_panics_preserve_original_unwind_even_when_custom_cancellation_panics", "--nocapture"]).env(MODE, mode).output().unwrap();
            assert!(
                output.status.success(),
                "{mode} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };
    let saved = if mode == "decoder" || mode == "restored_stack_factory" {
        saved_stack()
    } else {
        Default::default()
    };
    let parent = restored(saved.clone());
    let scopes = Rc::new(RefCell::new(Vec::new()));
    let captured = scopes.clone();
    let connections;
    let panic;
    let key;
    if mode == "slot_factory" {
        let transport = SimpleNavigation::<SlotEvent<Config>>::new();
        let mut source = BorrowedSource::new(&transport);
        source.panic_on_cancel = true;
        connections = source.connections.clone();
        key = "DefaultChildSlot";
        panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            child_slot(
                &parent,
                &source,
                Some(Config(1)),
                SlotOptions::default(),
                move |_, ctx| -> Component {
                    captured
                        .borrow_mut()
                        .push((ctx.lifecycle(), ctx.instance_store()));
                    panic!("original slot factory");
                },
            )
        }))
        .err()
        .unwrap();
    } else if mode == "panels_hook" {
        let transport = SimpleNavigation::<PanelsEvent<Config, Config, Config>>::new();
        let mut source = BorrowedSource::new(&transport);
        source.panic_on_cancel = true;
        connections = source.connections.clone();
        key = "DefaultChildPanels";
        let options = PanelsOptions {
            on_state_changed: Some(Rc::new(|_, _| panic!("original panels hook"))),
            ..Default::default()
        };
        let initial = Panels {
            main: Rc::new(Config(1)),
            details: Some(Rc::new(Config(2))),
            extra: Some(Rc::new(Config(3))),
            mode: ChildPanelsMode::Triple,
        };
        let details = captured.clone();
        let extra = captured.clone();
        panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            child_panels(
                &parent,
                &source,
                initial,
                options,
                move |c, ctx| {
                    captured
                        .borrow_mut()
                        .push((ctx.lifecycle(), ctx.instance_store()));
                    Component(c.0)
                },
                move |c, ctx| {
                    details
                        .borrow_mut()
                        .push((ctx.lifecycle(), ctx.instance_store()));
                    Component(c.0)
                },
                move |c, ctx| {
                    extra
                        .borrow_mut()
                        .push((ctx.lifecycle(), ctx.instance_store()));
                    Component(c.0)
                },
            )
        }))
        .err()
        .unwrap();
    } else {
        let transport = SimpleNavigation::<StackEvent<Config>>::new();
        let mut source = BorrowedSource::new(&transport);
        source.panic_on_cancel = true;
        connections = source.connections.clone();
        key = "DefaultChildStack";
        let codec = if mode == "decoder" {
            StackCodec::new(
                |state: &[Rc<Config>]| Ok(state.iter().map(|c| c.0).collect()),
                |_| -> Result<Vec<Config>, String> { panic!("original decoder") },
            )
        } else {
            stack_codec()
        };
        panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            child_stack(
                &parent,
                &source,
                vec![Config(99)],
                StackOptions {
                    codec: Some(codec),
                    ..Default::default()
                },
                move |_, ctx| -> Component {
                    captured
                        .borrow_mut()
                        .push((ctx.lifecycle(), ctx.instance_store()));
                    panic!("original restored factory");
                },
            )
        }))
        .err()
        .unwrap();
    }
    let expected = match mode.as_str() {
        "slot_factory" => "original slot factory",
        "panels_hook" => "original panels hook",
        "decoder" => "original decoder",
        _ => "original restored factory",
    };
    assert_eq!(panic.downcast_ref::<&str>().copied(), Some(expected));
    assert_eq!(connections.get(), 0);
    assert_clean(&parent, key);
    assert_eq!(parent.state_store().save().unwrap(), saved);
    for (lifecycle, instances) in scopes.borrow().iter() {
        assert!(lifecycle.state().is_destroyed());
        assert!(instances.is_destroyed().unwrap());
    }
    match mode.as_str() {
        "slot_factory" => {
            child_slot(
                &parent,
                &SimpleNavigation::<SlotEvent<Config>>::new(),
                Some(Config(1)),
                SlotOptions::default(),
                |_, ctx| ctx,
            )
            .unwrap();
        }
        "panels_hook" => {
            child_panels_without_extra(
                &parent,
                &SimpleNavigation::<PanelsEvent<Config, Config>>::new(),
                Panels::new(Config(1)),
                PanelsOptions::default(),
                |_, ctx| ctx,
                |_, ctx| ctx,
            )
            .unwrap();
        }
        _ => {
            let value = child_stack(
                &parent,
                &SimpleNavigation::<StackEvent<Config>>::new(),
                vec![Config(99)],
                StackOptions {
                    codec: Some(stack_codec()),
                    ..Default::default()
                },
                |_, ctx| ctx,
            )
            .unwrap();
            assert_eq!(value.get().active.configuration.0, 1);
            assert_eq!(
                value
                    .get()
                    .active
                    .instance
                    .state_store()
                    .consume_bytes("child-state")
                    .unwrap(),
                Some(vec![42])
            );
        }
    }
}

#[test]
fn back_dispatch_construction_error_restores_saved_input_before_custom_cancel_panic() {
    let saved = saved_stack();
    let parent = restored(saved.clone());
    let transport = SimpleNavigation::<StackEvent<Config>>::new();
    let connections = Rc::new(Cell::new(0));
    let observed_connections = connections.clone();
    let callback_transport = transport.clone();
    let nested = parent.clone();
    let scoped = Rc::new(RefCell::new(None));
    let captured = scoped.clone();
    let handle = parent
        .back_handler()
        .register(0, true, move |_, _| {
            let mut source = BorrowedSource::new(&callback_transport);
            source.panic_on_cancel = true;
            source.connections = observed_connections.clone();
            let captured = captured.clone();
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                child_stack(
                    &nested,
                    &source,
                    vec![Config(99)],
                    StackOptions {
                        codec: Some(stack_codec()),
                        ..Default::default()
                    },
                    move |_, ctx| {
                        *captured.borrow_mut() = Some((ctx.lifecycle(), ctx.instance_store()));
                        ctx
                    },
                )
            }))
            .err()
            .unwrap();
            assert_eq!(
                panic.downcast_ref::<&str>(),
                Some(&"custom source cancellation")
            );
        })
        .unwrap();
    assert!(parent.back_handler().back());
    assert_eq!(connections.get(), 0);
    assert!(
        !parent
            .state_store()
            .is_registered("DefaultChildStack")
            .unwrap()
    );
    assert!(parent.instance_store().is_empty().unwrap());
    assert_eq!(parent.state_store().save().unwrap(), saved);
    assert_eq!(parent.back_handler().handler_count(), 1);
    // Back dispatch rejects the new child scope before its factory is called.
    assert!(scoped.borrow().is_none());
    parent.back_handler().unregister(handle.id());
    let value = child_stack(
        &parent,
        &transport,
        vec![Config(99)],
        StackOptions {
            codec: Some(stack_codec()),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(value.get().active.configuration.0, 1);
}

#[test]
fn decoder_and_restored_factory_events_join_subscribe_events_in_fifo_order() {
    let saved = saved_stack();
    let parent = restored(saved);
    let transport = SimpleNavigation::<StackEvent<Config>>::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut source = BorrowedSource::new(&transport);
    source.on_subscribe = Some(Rc::new(StackEvent::with_completion(
        |mut s| {
            s.push(Rc::new(Config(2)));
            s
        },
        completion(2, &log),
    )));
    let decoder_source = transport.clone();
    let decoder_log = log.clone();
    let codec = StackCodec::new(
        |state: &[Rc<Config>]| Ok(state.iter().map(|c| c.0).collect()),
        move |bytes| {
            assert!(decoder_log.borrow().is_empty());
            decoder_source.navigate(StackEvent::with_completion(
                |mut s| {
                    s.push(Rc::new(Config(3)));
                    s
                },
                completion(3, &decoder_log),
            ));
            Ok(bytes.iter().copied().map(Config).collect())
        },
    );
    let nested = transport.clone();
    let factory_log = log.clone();
    let value = child_stack(
        &parent,
        &source,
        vec![Config(99)],
        StackOptions {
            codec: Some(codec),
            ..Default::default()
        },
        move |c, ctx| {
            if c.0 == 1 {
                assert!(factory_log.borrow().is_empty());
                assert_eq!(
                    ctx.state_store().consume_bytes("child-state").unwrap(),
                    Some(vec![42])
                );
                nested.navigate(StackEvent::with_completion(
                    |mut s| {
                        s.push(Rc::new(Config(4)));
                        s
                    },
                    completion(4, &factory_log),
                ));
            }
            ctx
        },
    )
    .unwrap();
    assert_eq!(*log.borrow(), [2, 3, 4]);
    assert_eq!(
        value
            .get()
            .items()
            .map(|c| c.configuration.0)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
}

#[test]
fn one_custom_source_completes_after_publication_separately_for_each_connected_receiver() {
    let first_parent = root();
    let second_parent = root();
    let transport = SimpleNavigation::<StackEvent<Config>>::new();
    let source = BorrowedSource::new(&transport);
    let first = child_stack(
        &first_parent,
        &source,
        vec![Config(10)],
        StackOptions::default(),
        |c, _| Component(c.0),
    )
    .unwrap();
    let second = child_stack(
        &second_parent,
        &source,
        vec![Config(20)],
        StackOptions::default(),
        |c, _| Component(c.0),
    )
    .unwrap();
    assert_eq!(source.connections.get(), 2);
    let completed = Rc::new(RefCell::new(Vec::new()));
    let output = completed.clone();
    let first_check = first.clone();
    let second_check = second.clone();
    transport.navigate(StackEvent::with_completion(
        |mut state: Vec<Rc<Config>>| {
            state.push(Rc::new(Config(state[0].0 + 1)));
            state
        },
        move |result| {
            let result = result.unwrap();
            let old = result.old_stack[0].0;
            let published = if old == 10 {
                first_check.get()
            } else {
                second_check.get()
            };
            assert_eq!(published.active.instance.0, old + 1);
            output
                .borrow_mut()
                .push((old, result.new_stack.last().unwrap().0));
        },
    ));
    assert_eq!(*completed.borrow(), [(10, 11), (20, 21)]);
    first_parent.lifecycle().destroy().unwrap();
    assert_eq!(source.connections.get(), 1);
    second_parent.lifecycle().destroy().unwrap();
    assert_eq!(source.connections.get(), 0);
}

#[test]
fn slot_public_event_result_adapts_none_and_some_without_replacing_equal_child() {
    let parent = root();
    let transport = SimpleNavigation::<SlotEvent<Config>>::new();
    let source = BorrowedSource::new(&transport);
    let value = child_slot(&parent, &source, None, SlotOptions::default(), |c, _| {
        Component(c.0)
    })
    .unwrap();
    transport.navigate(SlotEvent::with_completion(
        |_| Some(Rc::new(Config(1))),
        |result| {
            let result = result.unwrap();
            assert!(result.old_configuration.is_none());
            assert_eq!(result.new_configuration.unwrap().0, 1);
        },
    ));
    let child = value.get().child.as_ref().unwrap().clone();
    transport.navigate(SlotEvent::with_completion(
        |_| Some(Rc::new(Config(1))),
        |result| {
            let result = result.unwrap();
            assert_eq!(result.old_configuration.unwrap().0, 1);
            assert_eq!(result.new_configuration.unwrap().0, 1);
        },
    ));
    assert_eq!(value.get().child.as_ref().unwrap().key, child.key);
    assert!(Rc::ptr_eq(
        &value.get().child.as_ref().unwrap().instance,
        &child.instance
    ));
    transport.navigate(SlotEvent::with_completion(
        |_: Option<Rc<Config>>| None,
        |result| {
            let result = result.unwrap();
            assert_eq!(result.old_configuration.unwrap().0, 1);
            assert!(result.new_configuration.is_none());
        },
    ));
    assert!(value.get().child.is_none());
}

#[test]
fn typed_decoder_error_returns_saved_input_and_cancels_custom_source_before_retry() {
    let saved = saved_stack();
    let parent = restored(saved.clone());
    let transport = SimpleNavigation::<StackEvent<Config>>::new();
    let source = BorrowedSource::new(&transport);
    let options = StackOptions {
        codec: Some(StackCodec::new(
            |_: &[Rc<Config>]| Ok(vec![]),
            |_| Err("typed decoder error".into()),
        )),
        ..Default::default()
    };
    let result = child_stack(
        &parent,
        &source,
        vec![Config(99)],
        options,
        |_, _| -> Component { panic!("factory must not run") },
    );
    assert!(
        matches!(result, Err(router::NavigationError::State(message)) if message == "typed decoder error")
    );
    assert_eq!(source.connections.get(), 0);
    assert_clean(&parent, "DefaultChildStack");
    assert_eq!(parent.state_store().save().unwrap(), saved);
    let value = child_stack(
        &parent,
        &source,
        vec![Config(99)],
        StackOptions {
            codec: Some(stack_codec()),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(value.get().active.configuration.0, 1);
    assert_eq!(source.connections.get(), 1);
}

#[test]
fn parent_cleanup_finishes_scopes_and_provider_removal_before_custom_cancel_panic_propagates() {
    let parent = root();
    let transport = SimpleNavigation::<SlotEvent<Config>>::new();
    let mut source = BorrowedSource::new(&transport);
    source.panic_on_cancel = true;
    let value = child_slot(
        &parent,
        &source,
        Some(Config(1)),
        SlotOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let child = value.get().child.as_ref().unwrap().clone();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        parent.lifecycle().destroy()
    }))
    .unwrap_err();
    assert_eq!(
        panic.downcast_ref::<&str>(),
        Some(&"custom source cancellation")
    );
    assert_eq!(source.connections.get(), 0);
    assert!(
        !parent
            .state_store()
            .is_registered("DefaultChildSlot")
            .unwrap()
    );
    assert_eq!(parent.back_handler().handler_count(), 0);
    assert!(child.instance.lifecycle().state().is_destroyed());
}
