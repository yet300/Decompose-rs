#![allow(deprecated)] // Legacy API behavior remains covered during migration.
use decompose_rs::router::children::*;
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
fn entry(c: u8, status: ChildStatus) -> ChildNavState<u8> {
    ChildNavState::new(c, status)
}

#[test]
fn source_non_clone_events_share_identity_and_nested_broadcasts_are_fifo() {
    struct Event(u8);
    let source = SimpleNavigation::<Event>::default();
    let nested = source.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    source.navigate(Event(0));
    let output = seen.clone();
    let _first = source.subscribe(Rc::new(move |event| {
        output.borrow_mut().push((1, event.0, Rc::as_ptr(&event)));
        if event.0 == 1 {
            nested.navigate(Event(2));
        }
    }));
    let output = seen.clone();
    let _second = source.subscribe(Rc::new(move |event| {
        output.borrow_mut().push((2, event.0, Rc::as_ptr(&event)))
    }));
    let event = Rc::new(Event(1));
    source.navigate_rc(event.clone());
    let seen = seen.borrow();
    assert_eq!(
        seen.iter().map(|(i, e, _)| (*i, *e)).collect::<Vec<_>>(),
        [(1, 1), (2, 1), (1, 2), (2, 2)]
    );
    assert_eq!(seen[0].2, Rc::as_ptr(&event));
    assert_eq!(seen[0].2, seen[1].2);
}

#[test]
fn generic_four_statuses_and_tombstone_resurrection_preserve_key() {
    let parent = root();
    let source = SimpleNavigation::new();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let value = children(
        &parent,
        &source,
        || {
            vec![
                entry(0, ChildStatus::Created),
                entry(1, ChildStatus::Started),
                entry(2, ChildStatus::Resumed),
                entry(3, ChildStatus::Destroyed),
            ]
        },
        ChildrenOptions::default(),
        |_, e: &Vec<ChildNavState<u8>>| e.clone(),
        |_, children| children,
        move |_, ctx| {
            count.set(count.get() + 1);
            ctx
        },
    )
    .unwrap();
    let old = value.get();
    assert_eq!(calls.get(), 3);
    for (i, s) in [
        LifecycleState::Created,
        LifecycleState::Started,
        LifecycleState::Resumed,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(old[i].instance().unwrap().lifecycle().state(), s);
    }
    assert!(old[3].instance().is_none());
    source.navigate(vec![entry(2, ChildStatus::Destroyed)]);
    assert!(
        old[2]
            .instance()
            .unwrap()
            .lifecycle()
            .state()
            .is_destroyed()
    );
    source.navigate(vec![entry(2, ChildStatus::Resumed)]);
    assert_eq!(value.get()[0].key(), old[2].key());
    assert!(!Rc::ptr_eq(
        value.get()[0].instance().unwrap(),
        old[2].instance().unwrap()
    ));
    source.navigate(vec![]);
    assert!(value.get().is_empty());
}

#[test]
fn resurrection_uses_current_equal_descriptor_without_replacing_live_component() {
    struct Config {
        id: u8,
        payload: u8,
    }
    impl PartialEq for Config {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id
        }
    }
    impl Eq for Config {}
    fn state(payload: u8, status: ChildStatus) -> Vec<ChildNavState<Config>> {
        vec![ChildNavState::new(Config { id: 1, payload }, status)]
    }
    let parent = root();
    let source = SimpleNavigation::new();
    let value = children(
        &parent,
        &source,
        || state(1, ChildStatus::Destroyed),
        ChildrenOptions::default(),
        |_, e: &Vec<ChildNavState<Config>>| e.clone(),
        |_, c| c,
        |c, _| c.payload,
    )
    .unwrap();
    let key = value.get()[0].key().to_owned();
    source.navigate(state(2, ChildStatus::Resumed));
    assert_eq!(**value.get()[0].instance().unwrap(), 2);
    let live = value.get()[0].instance().unwrap().clone();
    source.navigate(state(3, ChildStatus::Started));
    assert!(Rc::ptr_eq(&live, value.get()[0].instance().unwrap()));
    assert_eq!(value.get()[0].configuration().payload, 2);
    source.navigate(state(4, ChildStatus::Destroyed));
    assert_eq!(value.get()[0].configuration().payload, 4);
    source.navigate(state(5, ChildStatus::Resumed));
    assert_eq!(**value.get()[0].instance().unwrap(), 5);
    assert_eq!(value.get()[0].key(), key);
}

type State = Vec<ChildNavState<u8>>;
fn codec() -> NavStateCodec<State> {
    NavStateCodec::new(
        |state: &State| {
            Ok(Some(
                state
                    .iter()
                    .flat_map(|c| {
                        [
                            *c.configuration,
                            match c.status {
                                ChildStatus::Created => 0,
                                ChildStatus::Started => 1,
                                ChildStatus::Resumed => 2,
                                ChildStatus::Destroyed => 3,
                            },
                        ]
                    })
                    .collect(),
            ))
        },
        |bytes| {
            if bytes.len() % 2 != 0 {
                return Err("odd payload".into());
            }
            bytes
                .chunks_exact(2)
                .map(|b| {
                    Ok(entry(
                        b[0],
                        match b[1] {
                            0 => ChildStatus::Created,
                            1 => ChildStatus::Started,
                            2 => ChildStatus::Resumed,
                            3 => ChildStatus::Destroyed,
                            _ => return Err("bad status".into()),
                        },
                    ))
                })
                .collect::<Result<Vec<_>, String>>()
                .map(Some)
        },
    )
}
fn restored(
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
fn persistent() -> ChildrenOptions<State, State> {
    ChildrenOptions {
        codec: Some(codec()),
        ..Default::default()
    }
}

#[test]
fn dch1_literal_roundtrip_skips_initial_and_restores_child_state_and_tombstone() {
    let parent = root();
    let source = SimpleNavigation::new();
    let value = children(
        &parent,
        &source,
        || {
            vec![
                entry(7, ChildStatus::Resumed),
                entry(8, ChildStatus::Destroyed),
            ]
        },
        persistent(),
        |_, e: &State| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    value.get()[0]
        .instance()
        .unwrap()
        .state_store()
        .register("counter", || vec![42])
        .unwrap();
    let saved = parent.state_store().save().unwrap();
    let bytes = &saved["DefaultChildren"];
    assert_eq!(&bytes[..4], b"DCH1");
    assert_eq!(&bytes[4..12], &2u64.to_le_bytes());
    assert_eq!(&bytes[12..24], &[4, 0, 0, 0, 7, 2, 8, 3, 2, 0, 0, 0]);
    let parent = restored(saved.clone(), None);
    let source = SimpleNavigation::new();
    let value = children(
        &parent,
        &source,
        || panic!("restored initial must not run"),
        persistent(),
        |_, e: &State| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(value.get()[0].key(), "child/0");
    assert_eq!(value.get()[1].key(), "child/1");
    assert!(value.get()[1].instance().is_none());
    assert_eq!(
        value.get()[0]
            .instance()
            .unwrap()
            .state_store()
            .consume_bytes("counter")
            .unwrap(),
        Some(vec![42])
    );
    source.navigate(vec![
        entry(7, ChildStatus::Resumed),
        entry(8, ChildStatus::Resumed),
        entry(9, ChildStatus::Created),
    ]);
    assert_eq!(value.get()[2].key(), "child/2");
}

#[test]
fn codec_decline_calls_initial_and_discards_saved_child_state() {
    let parent = root();
    let value = children(
        &parent,
        &SimpleNavigation::new(),
        || vec![entry(1, ChildStatus::Resumed)],
        persistent(),
        |_, e: &State| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    value.get()[0]
        .instance()
        .unwrap()
        .state_store()
        .register("old", || vec![1])
        .unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = restored(saved, None);
    let options = ChildrenOptions {
        codec: Some(NavStateCodec::new(|_| Ok(None), |_| Ok(None))),
        ..Default::default()
    };
    let value = children(
        &parent,
        &SimpleNavigation::<()>::new(),
        || vec![entry(1, ChildStatus::Resumed)],
        options,
        |s, _| s,
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(
        value.get()[0]
            .instance()
            .unwrap()
            .state_store()
            .pending_count()
            .unwrap(),
        0
    );
    assert!(
        !parent
            .state_store()
            .save()
            .unwrap()
            .contains_key("DefaultChildren")
    );
}

#[test]
fn corrupt_envelope_is_rejected_before_user_decoder_and_reservation_rolls_back() {
    let parent = root();
    children(
        &parent,
        &SimpleNavigation::new(),
        || vec![entry(1, ChildStatus::Resumed)],
        persistent(),
        |_, e: &State| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    let good = parent.state_store().save().unwrap();
    for kind in 0..6 {
        let mut saved = good.clone();
        let bytes = saved.get_mut("DefaultChildren").unwrap();
        match kind {
            0 => bytes[0] = 0,
            1 => bytes.push(0),
            2 => bytes[4..12].copy_from_slice(&0u64.to_le_bytes()),
            3 => {
                bytes.pop();
            }
            4 => bytes[29] = 0xff,
            _ => bytes[40] = 0,
        }
        let parent = restored(saved.clone(), None);
        let calls = Rc::new(Cell::new(0));
        let count = calls.clone();
        let options = ChildrenOptions {
            codec: Some(NavStateCodec::new(
                |_| Ok(None),
                move |_| {
                    count.set(count.get() + 1);
                    Ok(Some(vec![entry(1, ChildStatus::Resumed)]))
                },
            )),
            ..Default::default()
        };
        assert!(
            children(
                &parent,
                &SimpleNavigation::<()>::new(),
                || panic!("invalid envelope initial"),
                options,
                |s, _| s,
                |_, c| c,
                |_, ctx| ctx
            )
            .is_err(),
            "mutation {kind}"
        );
        assert_eq!(calls.get(), 0, "mutation {kind}");
        assert_eq!(parent.state_store().save().unwrap(), saved);
        assert_eq!(parent.back_handler().handler_count(), 0);
    }
}

#[test]
fn absent_or_encode_none_codec_never_queries_child_providers() {
    for encode_none in [false, true] {
        let parent = root();
        let options = ChildrenOptions {
            codec: encode_none.then(|| NavStateCodec::new(|_: &State| Ok(None), |_| Ok(None))),
            ..Default::default()
        };
        let value = children(
            &parent,
            &SimpleNavigation::<()>::new(),
            || vec![entry(1, ChildStatus::Resumed)],
            options,
            |s, _| s,
            |_, c| c,
            |_, ctx| ctx,
        )
        .unwrap();
        value.get()[0]
            .instance()
            .unwrap()
            .state_store()
            .register("panic", || panic!("child must not be queried"))
            .unwrap();
        assert!(
            !parent
                .state_store()
                .save()
                .unwrap()
                .contains_key("DefaultChildren")
        );
    }
}

type ReplayObserver = Rc<dyn Fn(Rc<u8>)>;
struct ReplaySource {
    observer: Rc<RefCell<Option<ReplayObserver>>>,
    subscribed: Rc<Cell<bool>>,
    cancelled: Rc<Cell<usize>>,
}
impl ReplaySource {
    fn new() -> Self {
        Self {
            observer: Rc::new(RefCell::new(None)),
            subscribed: Rc::new(Cell::new(false)),
            cancelled: Rc::new(Cell::new(0)),
        }
    }
    fn emit(&self, value: u8) {
        let callback = self.observer.borrow().clone();
        if let Some(callback) = callback {
            callback(Rc::new(value));
        }
    }
}
impl NavigationSource<u8> for ReplaySource {
    fn subscribe(&self, observer: Rc<dyn Fn(Rc<u8>)>) -> decompose_rs::value::Subscription {
        self.subscribed.set(true);
        *self.observer.borrow_mut() = Some(observer.clone());
        observer(Rc::new(1));
        let slot = self.observer.clone();
        let cancelled = self.cancelled.clone();
        decompose_rs::value::Subscription::from_cancel(move || {
            let old = slot.borrow_mut().take();
            drop(old);
            cancelled.set(cancelled.get() + 1);
        })
    }
}
#[test]
fn custom_source_subscribe_initial_factory_and_hook_emissions_share_fifo() {
    let parent = root();
    let source = Rc::new(ReplaySource::new());
    let initial_source = source.clone();
    let factory_source = source.clone();
    let hook_source = source.clone();
    let changed = Rc::new(RefCell::new(Vec::new()));
    let log = changed.clone();
    let completed = Rc::new(RefCell::new(Vec::new()));
    let completions = completed.clone();
    let options = ChildrenOptions {
        on_state_changed: Some(Rc::new(move |n: &State, old| {
            log.borrow_mut()
                .push((*n[0].configuration, old.map(|s| *s[0].configuration)));
            if old.is_none() {
                hook_source.emit(4);
            }
        })),
        on_event_complete: Some(Rc::new(move |e: &u8, result| {
            let result = result.unwrap();
            completions
                .borrow_mut()
                .push((*e, *result.new_state[0].configuration));
        })),
        ..Default::default()
    };
    let dynamic: &dyn NavigationSource<u8> = &*source;
    let value = children(
        &parent,
        dynamic,
        move || {
            assert!(initial_source.subscribed.get());
            initial_source.emit(2);
            vec![entry(0, ChildStatus::Resumed)]
        },
        options,
        |_, e| vec![entry(*e, ChildStatus::Resumed)],
        |_, c| c,
        move |c, ctx| {
            if *c == 0 {
                factory_source.emit(3);
            }
            ctx
        },
    )
    .unwrap();
    assert_eq!(
        *changed.borrow(),
        [
            (0, None),
            (1, Some(0)),
            (2, Some(1)),
            (3, Some(2)),
            (4, Some(3))
        ]
    );
    assert_eq!(*completed.borrow(), [(1, 1), (2, 2), (3, 3), (4, 4)]);
    assert_eq!(**value.get()[0].configuration(), 4);
    parent.lifecycle().destroy().unwrap();
    assert_eq!(source.cancelled.get(), 1);
    source.emit(5);
    assert_eq!(**value.get()[0].configuration(), 4);
}
#[test]
fn failed_initial_validation_cancels_custom_source_and_releases_key() {
    let parent = root();
    let source = ReplaySource::new();
    let factory = Rc::new(Cell::new(0));
    let calls = factory.clone();
    assert!(matches!(
        children(
            &parent,
            &source,
            || vec![
                entry(0, ChildStatus::Created),
                entry(0, ChildStatus::Destroyed)
            ],
            ChildrenOptions::default(),
            |_, e| vec![entry(*e, ChildStatus::Resumed)],
            |_, c| c,
            move |_, ctx| {
                calls.set(calls.get() + 1);
                ctx
            }
        ),
        Err(NavigationError::DuplicateConfiguration)
    ));
    assert_eq!(factory.get(), 0);
    assert_eq!(source.cancelled.get(), 1);
    assert!(source.observer.borrow().is_none());
    let value = children(
        &parent,
        &source,
        Vec::<ChildNavState<u8>>::new,
        ChildrenOptions::default(),
        |_, e| vec![entry(*e, ChildStatus::Resumed)],
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(**value.get()[0].configuration(), 1);
}
#[test]
fn source_cancellation_detachment_and_panic_clear_queue_and_preserve_reusability() {
    use decompose_rs::value::Subscription;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let source = SimpleNavigation::<u8>::new();
    let pending = Rc::new(RefCell::new(None::<Subscription>));
    let cancel = pending.clone();
    let nested = source.clone();
    let _first = source.subscribe(Rc::new(move |e| {
        if *e == 1 {
            cancel.borrow().as_ref().unwrap().cancel();
            nested.navigate(2);
            panic!("source callback");
        }
    }));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    *pending.borrow_mut() = Some(source.subscribe(Rc::new(move |e| log.borrow_mut().push(*e))));
    let log = seen.clone();
    source
        .subscribe(Rc::new(move |e| log.borrow_mut().push(*e + 10)))
        .detach();
    assert!(catch_unwind(AssertUnwindSafe(|| source.navigate(1))).is_err());
    source.navigate(3);
    assert_eq!(*seen.borrow(), [13]);
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let token = source.subscribe(Rc::new(move |_| {
        let _ = &capture;
    }));
    drop(token);
    assert!(weak.upgrade().is_none());
}

struct RichState {
    records: State,
    revision: u8,
}
impl Clone for RichState {
    fn clone(&self) -> Self {
        Self {
            records: self.records.clone(),
            revision: self.revision,
        }
    }
}
impl NavState<u8> for RichState {
    fn children(&self) -> State {
        self.records.clone()
    }
}
struct NonCloneContext {
    base: DefaultComponentContext,
    service: Rc<Cell<u8>>,
}
impl ComponentContext for NonCloneContext {
    fn lifecycle(&self) -> LifecycleRegistry {
        self.base.lifecycle()
    }
    fn state_store(&self) -> StateStore {
        self.base.state_store()
    }
    fn instance_store(&self) -> InstanceStore {
        self.base.instance_store()
    }
    fn back_handler(&self) -> SharedBack {
        self.base.back_handler()
    }
    fn context_factory(&self) -> ContextFactory<Self> {
        let base = self.base.context_factory();
        let service = self.service.clone();
        Rc::new(move |parts| Self {
            base: base(parts),
            service: service.clone(),
        })
    }
}
#[test]
fn rich_state_non_clone_event_output_and_context_preserve_identity_and_services() {
    struct Event {
        revision: u8,
    }
    struct Output {
        revision: u8,
        items: Vec<ChildItem<u8, NonCloneContext>>,
    }
    let parent = NonCloneContext {
        base: root(),
        service: Rc::new(Cell::new(7)),
    };
    let source = SimpleNavigation::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let complete = seen.clone();
    let transform = seen.clone();
    let options: ChildrenOptions<Event, RichState> = ChildrenOptions {
        on_event_complete: Some(Rc::new(move |e: &Event, r| {
            let r = r.unwrap().clone();
            assert_eq!(r.old_state.revision, 0);
            assert_eq!(r.new_state.revision, e.revision);
            complete.borrow_mut().push(e as *const Event);
        })),
        ..Default::default()
    };
    let value = children(
        &parent,
        &source,
        || RichState {
            records: vec![entry(1, ChildStatus::Resumed)],
            revision: 0,
        },
        options,
        move |mut n, e| {
            transform.borrow_mut().push(e as *const Event);
            n.revision = e.revision;
            n
        },
        |n, c| Output {
            revision: n.revision,
            items: c,
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let event = Rc::new(Event { revision: 2 });
    source.navigate_rc(event.clone());
    assert_eq!(value.get().revision, 2);
    assert_eq!(*seen.borrow(), [Rc::as_ptr(&event), Rc::as_ptr(&event)]);
    assert!(Rc::ptr_eq(
        &parent.service,
        &value.get().items[0].instance().unwrap().service
    ));
}

#[test]
fn duplicate_occurrences_keep_distinct_stable_keys_and_failure_skips_factories() {
    let parent = root();
    let source = SimpleNavigation::new();
    let errors = Rc::new(RefCell::new(Vec::new()));
    let output = errors.clone();
    let options = ChildrenOptions {
        allow_duplicates: true,
        on_event_complete: Some(Rc::new(move |_: &State, r| {
            output.borrow_mut().push(r.is_ok())
        })),
        ..Default::default()
    };
    let value = children(
        &parent,
        &source,
        || {
            vec![
                entry(1, ChildStatus::Created),
                entry(1, ChildStatus::Destroyed),
            ]
        },
        options,
        |_, e: &State| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_ne!(value.get()[0].key(), value.get()[1].key());
    let keys = value
        .get()
        .iter()
        .map(|c| c.key().to_owned())
        .collect::<Vec<_>>();
    source.navigate(vec![
        entry(1, ChildStatus::Destroyed),
        entry(1, ChildStatus::Resumed),
    ]);
    assert_eq!(
        value
            .get()
            .iter()
            .map(|c| c.key().to_owned())
            .collect::<Vec<_>>(),
        keys
    );
    assert_eq!(*errors.borrow(), [true]);
    let parent = root();
    let source = SimpleNavigation::new();
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let errors = Rc::new(RefCell::new(None));
    let error = errors.clone();
    let value = children(
        &parent,
        &source,
        || vec![entry(1, ChildStatus::Created)],
        ChildrenOptions {
            on_event_complete: Some(Rc::new(move |_: &State, r| *error.borrow_mut() = r.err())),
            ..Default::default()
        },
        |_, e: &State| e.clone(),
        |_, c| c,
        move |_, ctx| {
            count.set(count.get() + 1);
            ctx
        },
    )
    .unwrap();
    source.navigate(vec![
        entry(2, ChildStatus::Resumed),
        entry(2, ChildStatus::Created),
    ]);
    assert_eq!(
        *errors.borrow(),
        Some(NavigationError::DuplicateConfiguration)
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(**value.get()[0].configuration(), 1);
}

#[test]
fn source_discards_nested_event_emitted_after_last_receiver_cancels() {
    use decompose_rs::value::Subscription;
    let source = SimpleNavigation::<u8>::new();
    let token = Rc::new(RefCell::new(None::<Subscription>));
    let cancellation = token.clone();
    let late = Rc::new(RefCell::new(None::<Subscription>));
    let late_out = late.clone();
    let nested = source.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    *token.borrow_mut() = Some(source.subscribe(Rc::new(move |_| {
        cancellation.borrow().as_ref().unwrap().cancel();
        nested.navigate(2);
        let log = log.clone();
        *late_out.borrow_mut() =
            Some(nested.subscribe(Rc::new(move |e| log.borrow_mut().push(*e))));
    })));
    source.navigate(1);
    assert!(seen.borrow().is_empty());
    source.navigate(3);
    assert_eq!(*seen.borrow(), [3]);
}

#[test]
fn state_hook_initial_registration_publication_completion_and_unchanged_transitions() {
    use decompose_rs::value::Value;
    let parent = root();
    let source = SimpleNavigation::new();
    let parent_hook = parent.clone();
    let slot = Rc::new(RefCell::new(
        None::<Value<Vec<ChildItem<u8, DefaultComponentContext>>>>,
    ));
    let snapshot = slot.clone();
    let log = Rc::new(RefCell::new(Vec::new()));
    let changed = log.clone();
    let completed = log.clone();
    let options = ChildrenOptions {
        on_state_changed: Some(Rc::new(move |n: &State, old| {
            assert!(
                parent_hook
                    .state_store()
                    .is_registered("DefaultChildren")
                    .unwrap()
            );
            assert_eq!(parent_hook.back_handler().handler_count(), 2);
            if old.is_some() {
                assert_eq!(
                    **snapshot.borrow().as_ref().unwrap().get()[0].configuration(),
                    *n[0].configuration
                );
                changed.borrow_mut().push("hook");
            } else {
                changed.borrow_mut().push("initial");
            }
        })),
        on_event_complete: Some(Rc::new(move |_: &State, r| {
            assert!(r.is_ok());
            completed.borrow_mut().push("complete");
        })),
        ..Default::default()
    };
    let value = children(
        &parent,
        &source,
        || vec![entry(1, ChildStatus::Resumed)],
        options,
        |_, e: &State| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    *slot.borrow_mut() = Some(value.clone());
    let observed = log.clone();
    let _token = value.subscribe(move |_| observed.borrow_mut().push("published"));
    log.borrow_mut().clear();
    source.navigate(vec![entry(1, ChildStatus::Resumed)]);
    assert_eq!(*log.borrow(), ["published", "hook", "complete"]);
    slot.borrow_mut().take();
}
#[test]
fn back_action_is_cached_once_per_success_and_predictive_cancel_does_not_change_state() {
    let parent = root();
    let source = SimpleNavigation::<State>::new();
    let count = Rc::new(Cell::new(0));
    let evaluations = count.clone();
    let hooks = Rc::new(RefCell::new(Vec::new()));
    let changed = hooks.clone();
    let completions = Rc::new(Cell::new(0));
    let complete = completions.clone();
    let options = ChildrenOptions {
        back_transformer: Some(Rc::new(move |n: &State| {
            evaluations.set(evaluations.get() + 1);
            let n = n.clone();
            (n.len() > 1).then(|| {
                let action: Rc<BackAction<State>> = Rc::new(move || n[..n.len() - 1].to_vec());
                action
            })
        })),
        on_state_changed: Some(Rc::new(move |n: &State, old| {
            changed.borrow_mut().push((n.len(), old.map(Vec::len)))
        })),
        on_event_complete: Some(Rc::new(move |_: &State, _| {
            complete.set(complete.get() + 1);
        })),
        ..Default::default()
    };
    let value = children(
        &parent,
        &source,
        || {
            vec![
                entry(1, ChildStatus::Created),
                entry(2, ChildStatus::Resumed),
            ]
        },
        options,
        |_, e| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(count.get(), 1);
    assert!(parent.back_handler().can_handle());
    assert!(parent.back_handler().predictive_start());
    assert!(parent.back_handler().predictive_progress(0.5).unwrap());
    assert!(parent.back_handler().predictive_cancel().unwrap());
    assert_eq!(value.get().len(), 2);
    assert_eq!(count.get(), 1);
    assert_eq!(completions.get(), 0);
    source.navigate(vec![
        entry(1, ChildStatus::Created),
        entry(2, ChildStatus::Resumed),
    ]);
    assert_eq!(count.get(), 2);
    source.navigate(vec![
        entry(1, ChildStatus::Created),
        entry(1, ChildStatus::Resumed),
    ]);
    assert_eq!(count.get(), 2); // rejected duplicate does not recompute
    assert!(parent.back_handler().predictive_start());
    assert!(parent.back_handler().predictive_invoke().unwrap());
    assert_eq!(value.get().len(), 1);
    assert_eq!(count.get(), 3);
    assert!(!parent.back_handler().can_handle());
    assert_eq!(completions.get(), 2);
    assert_eq!(*hooks.borrow(), [(2, None), (2, Some(2)), (1, Some(2))]);
}
#[test]
fn child_back_priority_lifecycle_gate_and_explicit_events_are_deferred_until_dispatch_releases() {
    let parent = root();
    let source = SimpleNavigation::<State>::new();
    let nested = source.clone();
    let parent_back = parent.back_handler();
    let observed_back = parent_back.clone();
    let log = Rc::new(RefCell::new(Vec::new()));
    let changed = log.clone();
    let child_log = log.clone();
    let child_source = source.clone();
    let options = ChildrenOptions {
        back_transformer: Some(Rc::new(move |n: &State| {
            let n = n.clone();
            let nested = nested.clone();
            (n.len() > 1).then(|| {
                let a: Rc<BackAction<State>> = Rc::new(move || {
                    nested.navigate(vec![entry(9, ChildStatus::Resumed)]);
                    n[..1].to_vec()
                });
                a
            })
        })),
        on_state_changed: Some(Rc::new(move |n: &State, old| {
            if old.is_some() {
                assert!(!observed_back.is_dispatching());
                changed.borrow_mut().push(*n[0].configuration);
            }
        })),
        ..Default::default()
    };
    let value = children(
        &parent,
        &source,
        || {
            vec![
                entry(1, ChildStatus::Created),
                entry(2, ChildStatus::Started),
            ]
        },
        options,
        |_, e| e.clone(),
        |_, c| c,
        move |c, ctx| {
            if *c == 2 {
                let source = child_source.clone();
                let log = child_log.clone();
                ctx.back_handler()
                    .register(0, true, move |event, _| {
                        if event.phase == BackPhase::Invoked {
                            log.borrow_mut().push(2);
                            source.navigate(vec![entry(3, ChildStatus::Resumed)]);
                        }
                    })
                    .unwrap();
            }
            ctx
        },
    )
    .unwrap();
    assert!(parent_back.back());
    assert_eq!(*log.borrow(), [2, 3]);
    assert_eq!(**value.get()[0].configuration(), 3);
    source.navigate(vec![
        entry(1, ChildStatus::Created),
        entry(2, ChildStatus::Created),
    ]);
    log.borrow_mut().clear();
    assert!(parent_back.back());
    assert_eq!(*log.borrow(), [1, 9]);
    assert_eq!(**value.get()[0].configuration(), 9);
}

#[test]
fn hook_mapper_factory_observer_and_completion_panics_abandon_nested_events_and_can_retry() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    for stage in 0..5 {
        let parent = root();
        let source = SimpleNavigation::<State>::new();
        let flag = Rc::new(Cell::new(true));
        let hook_flag = flag.clone();
        let hook_source = source.clone();
        let done_flag = flag.clone();
        let done_source = source.clone();
        let options = ChildrenOptions {
            on_state_changed: Some(Rc::new(move |n: &State, old| {
                if stage == 0
                    && old.is_some()
                    && *n[0].configuration == 2
                    && hook_flag.replace(false)
                {
                    hook_source.navigate(vec![entry(3, ChildStatus::Resumed)]);
                    panic!("hook");
                }
            })),
            on_event_complete: Some(Rc::new(move |e: &State, _| {
                if stage == 1 && *e[0].configuration == 2 && done_flag.replace(false) {
                    done_source.navigate(vec![entry(3, ChildStatus::Resumed)]);
                    panic!("completion");
                }
            })),
            ..Default::default()
        };
        let mapper_flag = flag.clone();
        let mapper_source = source.clone();
        let factory_flag = flag.clone();
        let factory_source = source.clone();
        let value = children(
            &parent,
            &source,
            || vec![entry(1, ChildStatus::Resumed)],
            options,
            |_, e| e.clone(),
            move |n, c| {
                if stage == 2 && *n[0].configuration == 2 && mapper_flag.replace(false) {
                    mapper_source.navigate(vec![entry(3, ChildStatus::Resumed)]);
                    panic!("mapper");
                }
                c
            },
            move |c, ctx| {
                if stage == 3 && *c == 2 && factory_flag.replace(false) {
                    factory_source.navigate(vec![entry(3, ChildStatus::Resumed)]);
                    panic!("factory");
                }
                ctx
            },
        )
        .unwrap();
        let observer_flag = flag.clone();
        let observer_source = source.clone();
        let _observer = value.subscribe(move |c| {
            if stage == 4 && **c[0].configuration() == 2 && observer_flag.replace(false) {
                observer_source.navigate(vec![entry(3, ChildStatus::Resumed)]);
                panic!("observer");
            }
        });
        assert!(
            catch_unwind(AssertUnwindSafe(
                || source.navigate(vec![entry(2, ChildStatus::Resumed)])
            ))
            .is_err(),
            "stage {stage}"
        );
        assert_ne!(**value.get()[0].configuration(), 3);
        source.navigate(vec![entry(4, ChildStatus::Resumed)]);
        assert_eq!(**value.get()[0].configuration(), 4);
    }
}
#[test]
fn parent_destroy_during_runtime_factory_completes_error_once_and_ends_held_snapshot() {
    let parent = root();
    let lifecycle = parent.lifecycle();
    let source = SimpleNavigation::<State>::new();
    let error = Rc::new(RefCell::new(Vec::new()));
    let errors = error.clone();
    let value = children(
        &parent,
        &source,
        || vec![entry(1, ChildStatus::Resumed)],
        ChildrenOptions {
            on_event_complete: Some(Rc::new(move |_: &State, r| {
                errors.borrow_mut().push(r.err())
            })),
            ..Default::default()
        },
        |_, e| e.clone(),
        |_, c| c,
        move |c, ctx| {
            if *c == 2 {
                lifecycle.destroy().unwrap();
            }
            ctx
        },
    )
    .unwrap();
    let old = value.get();
    source.navigate(vec![entry(2, ChildStatus::Resumed)]);
    assert_eq!(*error.borrow(), [Some(NavigationError::Destroyed)]);
    assert!(
        old[0]
            .instance()
            .unwrap()
            .lifecycle()
            .state()
            .is_destroyed()
    );
    assert_eq!(parent.back_handler().handler_count(), 0);
    assert!(
        !parent
            .state_store()
            .is_registered("DefaultChildren")
            .unwrap()
    );
    source.navigate(vec![entry(3, ChildStatus::Resumed)]);
    assert_eq!(error.borrow().len(), 1);
}

#[test]
fn tombstone_saved_state_survives_resurrection_and_removal_forgets_identity() {
    let parent = root();
    let source = SimpleNavigation::<State>::new();
    let value = children(
        &parent,
        &source,
        || vec![entry(1, ChildStatus::Resumed)],
        ChildrenOptions::default(),
        |_, e| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    let before = value.get()[0].instance().unwrap().clone();
    let key = value.get()[0].key().to_owned();
    before.state_store().register("saved", || vec![42]).unwrap();
    source.navigate(vec![entry(1, ChildStatus::Destroyed)]);
    assert!(before.instance_store().is_destroyed().unwrap());
    source.navigate(vec![entry(1, ChildStatus::Started)]);
    let after = value.get()[0].instance().unwrap().clone();
    assert!(!before.instance_store().ptr_eq(&after.instance_store()));
    assert_eq!(
        after.state_store().consume_bytes("saved").unwrap(),
        Some(vec![42])
    );
    assert_eq!(value.get()[0].key(), key);
    source.navigate(vec![]);
    source.navigate(vec![entry(1, ChildStatus::Resumed)]);
    assert_ne!(value.get()[0].key(), key);
    assert_eq!(
        value.get()[0]
            .instance()
            .unwrap()
            .state_store()
            .pending_count()
            .unwrap(),
        0
    );
}
#[test]
fn parent_recreation_reuses_live_instance_keeper_but_restored_tombstone_releases_it() {
    for tombstone in [false, true] {
        let instances = InstanceStore::from(InstanceKeeper::new());
        let parent = restored(Default::default(), Some(instances.clone()));
        let value = children(
            &parent,
            &SimpleNavigation::new(),
            || vec![entry(1, ChildStatus::Resumed)],
            persistent(),
            |_, e: &State| e.clone(),
            |_, c| c,
            |_, ctx| ctx,
        )
        .unwrap();
        let before = value.get()[0].instance().unwrap().clone();
        let kept = before
            .instance_store()
            .get_or_create("counter", || 42u8)
            .unwrap();
        let mut saved = parent.state_store().save().unwrap();
        parent.lifecycle().destroy().unwrap();
        assert!(!before.instance_store().is_destroyed().unwrap());
        if tombstone {
            saved.get_mut("DefaultChildren").unwrap()[17] = 3;
        }
        let parent = restored(saved, Some(instances.clone()));
        let value = children(
            &parent,
            &SimpleNavigation::new(),
            || panic!("restoration"),
            persistent(),
            |_, e: &State| e.clone(),
            |_, c| c,
            |_, ctx| ctx,
        )
        .unwrap();
        if tombstone {
            assert!(value.get()[0].instance().is_none());
            assert!(before.instance_store().is_destroyed().unwrap());
        } else {
            let after = value.get()[0].instance().unwrap().clone();
            assert!(!Rc::ptr_eq(&before, &after));
            assert!(before.instance_store().ptr_eq(&after.instance_store()));
            assert!(Rc::ptr_eq(
                &kept,
                &after
                    .instance_store()
                    .get_or_create("counter", || 99u8)
                    .unwrap()
            ));
        }
    }
}
fn envelope(payload: &[u8], next: u64, children: &[(&str, &[u8])]) -> Vec<u8> {
    fn field(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    let mut out = b"DCH1".to_vec();
    out.extend_from_slice(&next.to_le_bytes());
    field(&mut out, payload);
    out.extend_from_slice(&(children.len() as u32).to_le_bytes());
    for (key, state) in children {
        field(&mut out, key.as_bytes());
        field(&mut out, state);
    }
    out
}
#[test]
fn full_framing_key_uniqueness_and_nested_state_are_checked_before_declining_decoder() {
    let empty = b"DCS1\0\0\0\0";
    let variants = [
        envelope(&[1, 2, 2, 2], 2, &[("child/0", empty), ("child/0", empty)]),
        envelope(&[1, 2], 2, &[("child/01", empty)]),
        envelope(&[1, 2], 1, &[("child/1", empty)]),
        envelope(&[1, 2], 1, &[("child/0", b"DCS1\0\0\0\0x")]),
    ];
    for bytes in variants {
        let saved = std::collections::BTreeMap::from([("DefaultChildren".into(), bytes)]);
        let parent = restored(saved.clone(), None);
        let options = ChildrenOptions {
            codec: Some(NavStateCodec::new(
                |_: &State| Ok(None),
                |_| panic!("malformed frame must not decode"),
            )),
            ..Default::default()
        };
        assert!(
            children(
                &parent,
                &SimpleNavigation::<()>::new(),
                || panic!("must not initialize"),
                options,
                |s, _| s,
                |_, c| c,
                |_, ctx| ctx
            )
            .is_err()
        );
        assert_eq!(parent.state_store().save().unwrap(), saved);
    }
}
#[test]
fn restored_state_child_count_duplicates_and_codec_errors_precede_factories_and_return_bytes() {
    let empty = b"DCS1\0\0\0\0";
    for variant in 0..3 {
        let bytes = envelope(&[1, 2, 1, 2], 2, &[("child/0", empty), ("child/1", empty)]);
        let saved = std::collections::BTreeMap::from([("DefaultChildren".into(), bytes)]);
        let parent = restored(saved.clone(), None);
        let options = ChildrenOptions {
            codec: Some(NavStateCodec::new(
                |_: &State| Ok(None),
                move |_| match variant {
                    0 => Ok(Some(vec![entry(1, ChildStatus::Resumed)])),
                    1 => Ok(Some(vec![
                        entry(1, ChildStatus::Resumed),
                        entry(1, ChildStatus::Destroyed),
                    ])),
                    _ => Err("decode error".into()),
                },
            )),
            ..Default::default()
        };
        let result = children(
            &parent,
            &SimpleNavigation::<()>::new(),
            || panic!("error must not fallback"),
            options,
            |s, _| s,
            |_, c| c,
            |_, _| panic!("error must not create"),
        );
        assert!(result.is_err());
        if variant == 1 {
            assert!(matches!(
                result,
                Err(NavigationError::DuplicateConfiguration)
            ));
        }
        assert_eq!(parent.state_store().save().unwrap(), saved);
        assert_eq!(parent.instance_store().len().unwrap(), 0);
        assert_eq!(parent.back_handler().handler_count(), 0);
    }
    let parent = root();
    let options = ChildrenOptions {
        codec: Some(NavStateCodec::new(
            |_: &State| Err("encode error".into()),
            |_| Ok(None),
        )),
        ..Default::default()
    };
    let value = children(
        &parent,
        &SimpleNavigation::<()>::new(),
        || vec![entry(1, ChildStatus::Resumed)],
        options,
        |s, _| s,
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    value.get()[0]
        .instance()
        .unwrap()
        .state_store()
        .register("panic", || panic!("encode error precedes providers"))
        .unwrap();
    assert!(
        parent
            .state_store()
            .save()
            .unwrap_err()
            .to_string()
            .contains("encode error")
    );
}
#[test]
fn reentrant_codec_and_child_provider_saves_freeze_state_items_keys_and_counter() {
    for in_codec in [false, true] {
        let parent = root();
        let source = SimpleNavigation::<State>::new();
        let codec_source = source.clone();
        let once = Rc::new(Cell::new(true));
        let flag = once.clone();
        let base = codec();
        let options = ChildrenOptions {
            codec: Some(NavStateCodec::new(
                move |n: &State| {
                    if in_codec && flag.replace(false) {
                        codec_source.navigate(vec![]);
                    } // encoding must still use the captured state
                    Ok(Some(n.iter().flat_map(|c| [*c.configuration, 2]).collect()))
                },
                move |b| {
                    let _ = &base;
                    Ok(Some(
                        b.chunks_exact(2)
                            .map(|b| entry(b[0], ChildStatus::Resumed))
                            .collect(),
                    ))
                },
            )),
            ..Default::default()
        };
        let value = children(
            &parent,
            &source,
            || vec![entry(1, ChildStatus::Resumed)],
            options,
            |_, e| e.clone(),
            |_, c| c,
            |_, ctx| ctx,
        )
        .unwrap();
        let child_source = source.clone();
        value.get()[0]
            .instance()
            .unwrap()
            .state_store()
            .register("nested", move || {
                if !in_codec {
                    child_source.navigate(vec![]);
                }
                vec![42]
            })
            .unwrap();
        let saved = parent.state_store().save().unwrap();
        assert!(value.get().is_empty());
        let parent = restored(saved, None);
        let value = children(
            &parent,
            &SimpleNavigation::new(),
            || panic!("restore"),
            persistent(),
            |_, e: &State| e.clone(),
            |_, c| c,
            |_, ctx| ctx,
        )
        .unwrap();
        assert_eq!(value.get()[0].key(), "child/0");
        assert_eq!(**value.get()[0].configuration(), 1);
        assert_eq!(
            value.get()[0]
                .instance()
                .unwrap()
                .state_store()
                .consume_bytes("nested")
                .unwrap(),
            Some(vec![42])
        );
    }
}

#[test]
fn unobserved_explicit_event_failure_panics_and_source_can_retry() {
    let parent = root();
    let source = SimpleNavigation::<State>::new();
    let value = children(
        &parent,
        &source,
        || vec![entry(1, ChildStatus::Resumed)],
        ChildrenOptions::default(),
        |_, e| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        source.navigate(vec![
            entry(2, ChildStatus::Created),
            entry(2, ChildStatus::Destroyed),
        ])
    }));
    assert!(panic.is_err());
    assert_eq!(**value.get()[0].configuration(), 1);
    source.navigate(vec![entry(3, ChildStatus::Resumed)]);
    assert_eq!(**value.get()[0].configuration(), 3);
}

#[test]
fn rejected_automatic_back_panics_without_event_completion_and_can_retry() {
    let parent = root();
    let source = SimpleNavigation::<State>::new();
    let fail = Rc::new(Cell::new(true));
    let failing = fail.clone();
    let complete = Rc::new(Cell::new(0));
    let completed = complete.clone();
    let options = ChildrenOptions {
        back_transformer: Some(Rc::new(move |n: &State| {
            let n = n.clone();
            let failing = failing.clone();
            (n.len() > 1).then(|| {
                let action: Rc<BackAction<State>> = Rc::new(move || {
                    if failing.replace(false) {
                        vec![
                            entry(9, ChildStatus::Resumed),
                            entry(9, ChildStatus::Created),
                        ]
                    } else {
                        n[..1].to_vec()
                    }
                });
                action
            })
        })),
        on_event_complete: Some(Rc::new(move |_: &State, result| {
            assert!(result.is_ok());
            completed.set(completed.get() + 1);
        })),
        ..Default::default()
    };
    let value = children(
        &parent,
        &source,
        || {
            vec![
                entry(1, ChildStatus::Created),
                entry(2, ChildStatus::Resumed),
            ]
        },
        options,
        |_, e| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parent
            .back_handler()
            .back()))
        .is_err()
    );
    assert_eq!(value.get().len(), 2);
    assert_eq!(complete.get(), 0);
    source.navigate(vec![
        entry(3, ChildStatus::Resumed),
        entry(4, ChildStatus::Created),
    ]);
    assert_eq!(complete.get(), 1);
    assert!(parent.back_handler().back());
    assert_eq!(value.get().len(), 1);
    assert_eq!(**value.get()[0].configuration(), 3);
    assert_eq!(complete.get(), 1);
}

struct DropState {
    records: State,
    armed: Rc<Cell<bool>>,
    callback: Rc<dyn Fn()>,
}
impl Clone for DropState {
    fn clone(&self) -> Self {
        Self {
            records: self.records.clone(),
            armed: self.armed.clone(),
            callback: self.callback.clone(),
        }
    }
}
impl Drop for DropState {
    fn drop(&mut self) {
        if self.armed.replace(false) {
            (self.callback)();
        }
    }
}
impl NavState<u8> for DropState {
    fn children(&self) -> State {
        self.records.clone()
    }
}
#[test]
fn previous_state_drop_can_save_coherent_committed_children_without_engine_borrow() {
    let parent = root();
    let source = SimpleNavigation::<u8>::new();
    let keeper = parent.state_store();
    let saved = Rc::new(RefCell::new(None));
    let output = saved.clone();
    let armed = Rc::new(Cell::new(false));
    let initial = DropState {
        records: vec![entry(1, ChildStatus::Resumed)],
        armed: armed.clone(),
        callback: Rc::new(move || {
            *output.borrow_mut() = Some(keeper.save().unwrap());
        }),
    };
    let options = ChildrenOptions {
        codec: Some(NavStateCodec::new(
            |n: &DropState| Ok(Some(vec![*n.records[0].configuration, 2])),
            |_| Ok(None),
        )),
        ..Default::default()
    };
    let value = children(
        &parent,
        &source,
        || initial,
        options,
        |mut n, e| {
            n.records = vec![entry(*e, ChildStatus::Resumed)];
            n
        },
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    armed.set(true);
    source.navigate(2);
    assert_eq!(**value.get()[0].configuration(), 2);
    let parent2 = restored(saved.borrow_mut().take().unwrap(), None);
    let restored = children(
        &parent2,
        &SimpleNavigation::new(),
        || panic!("must restore coherent Drop save"),
        persistent(),
        |_, e: &State| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(**restored.get()[0].configuration(), 2);
    assert_eq!(restored.get()[0].key(), value.get()[0].key());
    parent.lifecycle().destroy().unwrap();
}
#[test]
fn previous_state_drop_navigation_is_fifo_and_parent_destroy_reports_error_without_publication() {
    for destroy in [false, true] {
        let parent = root();
        let source = SimpleNavigation::<u8>::new();
        let nested = source.clone();
        let lifecycle = parent.lifecycle();
        let armed = Rc::new(Cell::new(false));
        let initial = DropState {
            records: vec![entry(1, ChildStatus::Resumed)],
            armed: armed.clone(),
            callback: Rc::new(move || {
                if destroy {
                    lifecycle.destroy().unwrap();
                } else {
                    nested.navigate(3);
                }
            }),
        };
        let log = Rc::new(RefCell::new(Vec::new()));
        let complete = log.clone();
        let options = ChildrenOptions {
            on_event_complete: Some(Rc::new(move |e: &u8, r| {
                complete.borrow_mut().push((*e, r.err()));
            })),
            ..Default::default()
        };
        let value = children(
            &parent,
            &source,
            || initial,
            options,
            |mut n, e| {
                n.records = vec![entry(*e, ChildStatus::Resumed)];
                n
            },
            |_, c| c,
            |_, ctx| ctx,
        )
        .unwrap();
        let old = value.get();
        armed.set(true);
        source.navigate(2);
        if destroy {
            assert_eq!(*log.borrow(), [(2, Some(NavigationError::Destroyed))]);
            assert_eq!(**value.get()[0].configuration(), 1);
            assert!(
                old[0]
                    .instance()
                    .unwrap()
                    .lifecycle()
                    .state()
                    .is_destroyed()
            );
            assert_eq!(parent.back_handler().handler_count(), 0);
        } else {
            assert_eq!(*log.borrow(), [(2, None), (3, None)]);
            assert_eq!(**value.get()[0].configuration(), 3);
            parent.lifecycle().destroy().unwrap();
        }
    }
}
#[test]
fn previous_state_drop_panic_keeps_committed_scopes_and_next_navigation_repairs_publication() {
    let parent = root();
    let source = SimpleNavigation::<u8>::new();
    let nested = source.clone();
    let armed = Rc::new(Cell::new(false));
    let initial = DropState {
        records: vec![entry(1, ChildStatus::Resumed)],
        armed: armed.clone(),
        callback: Rc::new(move || {
            nested.navigate(3);
            panic!("previous navigation state Drop");
        }),
    };
    let created = Rc::new(RefCell::new(Vec::new()));
    let output = created.clone();
    let value = children(
        &parent,
        &source,
        || initial,
        ChildrenOptions::default(),
        |mut n, e| {
            n.records = vec![entry(*e, ChildStatus::Resumed)];
            n
        },
        |_, c| c,
        move |_, ctx| {
            output
                .borrow_mut()
                .push((ctx.lifecycle(), ctx.instance_store()));
            ctx
        },
    )
    .unwrap();
    armed.set(true);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.navigate(2))).is_err());
    assert_eq!(created.borrow().len(), 2);
    assert!(created.borrow()[0].0.state().is_destroyed());
    assert_eq!(created.borrow()[1].0.state(), LifecycleState::Resumed);
    assert!(!created.borrow()[1].1.is_destroyed().unwrap());
    assert_eq!(**value.get()[0].configuration(), 1);
    source.navigate(2);
    assert_eq!(created.borrow().len(), 2);
    assert_eq!(**value.get()[0].configuration(), 2);
    parent.lifecycle().destroy().unwrap();
}

struct CloneState {
    records: State,
    callback: Rc<dyn Fn()>,
}
impl Clone for CloneState {
    fn clone(&self) -> Self {
        (self.callback)();
        Self {
            records: self.records.clone(),
            callback: self.callback.clone(),
        }
    }
}
impl NavState<u8> for CloneState {
    fn children(&self) -> State {
        self.records.clone()
    }
}
#[test]
fn navigation_state_clone_can_emit_navigation_during_external_save_without_borrow_or_mixed_frame() {
    let parent = root();
    let source = SimpleNavigation::<u8>::new();
    let nested = source.clone();
    let armed = Rc::new(Cell::new(false));
    let trigger = armed.clone();
    let initial = CloneState {
        records: vec![entry(1, ChildStatus::Resumed)],
        callback: Rc::new(move || {
            if trigger.replace(false) {
                nested.navigate(2);
            }
        }),
    };
    let options = ChildrenOptions {
        codec: Some(NavStateCodec::new(
            |n: &CloneState| Ok(Some(vec![*n.records[0].configuration, 2])),
            |_| Ok(None),
        )),
        ..Default::default()
    };
    let value = children(
        &parent,
        &source,
        || initial,
        options,
        |mut n, e| {
            n.records = vec![entry(*e, ChildStatus::Resumed)];
            n
        },
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    armed.set(true);
    let saved = parent.state_store().save().unwrap();
    assert_eq!(**value.get()[0].configuration(), 2);
    let parent2 = restored(saved, None);
    let value2 = children(
        &parent2,
        &SimpleNavigation::new(),
        || panic!("must restore coherent clone save"),
        persistent(),
        |_, e: &State| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(**value2.get()[0].configuration(), 1);
    assert_eq!(value2.get()[0].key(), "child/0");
    parent.lifecycle().destroy().unwrap();
}
struct CancelPanicSource;
impl NavigationSource<()> for CancelPanicSource {
    fn subscribe(&self, _: Rc<dyn Fn(Rc<()>)>) -> decompose_rs::value::Subscription {
        decompose_rs::value::Subscription::from_cancel(|| panic!("custom source cancellation"))
    }
}
#[test]
fn custom_source_cancellation_panic_propagates_after_parent_cleanup_completes() {
    let parent = root();
    let value = children(
        &parent,
        &CancelPanicSource,
        || {
            vec![
                entry(1, ChildStatus::Resumed),
                entry(2, ChildStatus::Started),
            ]
        },
        ChildrenOptions::default(),
        |n, _| n,
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    let old = value.get();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        parent.lifecycle().destroy()
    }));
    assert!(panic.is_err());
    assert_eq!(parent.back_handler().handler_count(), 0);
    assert!(
        !parent
            .state_store()
            .is_registered("DefaultChildren")
            .unwrap()
    );
    for child in old.iter() {
        assert!(child.instance().unwrap().lifecycle().state().is_destroyed());
    }
}

#[test]
fn child_and_custom_source_cleanup_panics_preserve_first_panic_and_finish_other_children() {
    let parent = root();
    let log = Rc::new(RefCell::new(Vec::new()));
    let output = log.clone();
    let value = children(
        &parent,
        &CancelPanicSource,
        || {
            vec![
                entry(1, ChildStatus::Resumed),
                entry(2, ChildStatus::Started),
            ]
        },
        ChildrenOptions::default(),
        |n, _| n,
        |_, c| c,
        move |c, ctx| {
            let id = *c;
            let output = output.clone();
            let _ = ctx
                .lifecycle()
                .do_on_destroy(move || {
                    output.borrow_mut().push(id);
                    if id == 2 {
                        panic!("child cleanup first");
                    }
                })
                .detach();
            ctx
        },
    )
    .unwrap();
    let old = value.get();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        parent.lifecycle().destroy()
    }))
    .unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"child cleanup first"));
    assert_eq!(*log.borrow(), [2, 1]);
    assert_eq!(parent.back_handler().handler_count(), 0);
    assert!(
        !parent
            .state_store()
            .is_registered("DefaultChildren")
            .unwrap()
    );
    for child in old.iter() {
        assert!(child.instance().unwrap().lifecycle().state().is_destroyed());
    }
}
#[test]
fn initial_hook_unwind_preserves_original_panic_when_source_cancellation_also_panics() {
    let parent = root();
    let captured = Rc::new(RefCell::new(None));
    let output = captured.clone();
    let options = ChildrenOptions {
        on_state_changed: Some(Rc::new(|_: &State, _| {
            panic!("original initialization hook")
        })),
        ..Default::default()
    };
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        children(
            &parent,
            &CancelPanicSource,
            || vec![entry(1, ChildStatus::Resumed)],
            options,
            |n, _| n,
            |_, c| c,
            move |_, ctx| {
                *output.borrow_mut() = Some((ctx.lifecycle(), ctx.instance_store()));
                ctx
            },
        )
    }))
    .err()
    .expect("initial hook must panic");
    assert_eq!(
        panic.downcast_ref::<&str>(),
        Some(&"original initialization hook")
    );
    assert_eq!(parent.back_handler().handler_count(), 0);
    assert!(
        !parent
            .state_store()
            .is_registered("DefaultChildren")
            .unwrap()
    );
    assert!(captured.borrow().as_ref().unwrap().0.state().is_destroyed());
    assert!(
        captured
            .borrow()
            .as_ref()
            .unwrap()
            .1
            .is_destroyed()
            .unwrap()
    );
    assert_eq!(parent.instance_store().len().unwrap(), 0);
}

#[test]
fn construction_cancel_panic_restores_consumed_bytes_and_removes_fresh_retention_after_back_dispatch_error()
 {
    let saved = std::collections::BTreeMap::from([(
        "DefaultChildren".into(),
        envelope(&[1, 2], 1, &[("child/0", b"DCS1\0\0\0\0")]),
    )]);
    let parent = restored(saved.clone(), None);
    let nested = parent.clone();
    let panic_seen = Rc::new(Cell::new(false));
    let panicked = panic_seen.clone();
    parent
        .back_handler()
        .register(0, true, move |_, _| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                children(
                    &nested,
                    &CancelPanicSource,
                    || panic!("restored initial must not run"),
                    ChildrenOptions {
                        codec: Some(codec()),
                        ..Default::default()
                    },
                    |n, _: &()| n,
                    |_, c| c,
                    |_, ctx| ctx,
                )
            }));
            panicked.set(result.is_err());
        })
        .unwrap();
    assert!(parent.back_handler().back());
    assert!(panic_seen.get());
    assert_eq!(parent.instance_store().len().unwrap(), 0);
    assert!(
        !parent
            .state_store()
            .is_registered("DefaultChildren")
            .unwrap()
    );
    assert_eq!(parent.state_store().save().unwrap(), saved);
    assert_eq!(parent.back_handler().handler_count(), 1);
    let retried = children(
        &parent,
        &SimpleNavigation::<State>::new(),
        || panic!("rollback must return bytes"),
        persistent(),
        |_, e| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(**retried.get()[0].configuration(), 1);
}
#[test]
fn construction_cancel_panic_preserves_existing_retained_scope_and_restored_input() {
    let instances = InstanceStore::from(InstanceKeeper::new());
    let parent = restored(Default::default(), Some(instances.clone()));
    let value = children(
        &parent,
        &SimpleNavigation::<State>::new(),
        || vec![entry(1, ChildStatus::Resumed)],
        persistent(),
        |_, e| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    let child_instances = value.get()[0].instance().unwrap().instance_store();
    let sentinel = child_instances.get_or_create("sentinel", || 42u8).unwrap();
    let saved = parent.state_store().save().unwrap();
    parent.lifecycle().destroy().unwrap();
    let parent = restored(saved.clone(), Some(instances.clone()));
    let nested = parent.clone();
    parent
        .back_handler()
        .register(0, true, move |_, _| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                children(
                    &nested,
                    &CancelPanicSource,
                    || vec![entry(2, ChildStatus::Resumed)],
                    ChildrenOptions {
                        codec: Some(NavStateCodec::new(|_: &State| Ok(None), |_| Ok(None))),
                        ..Default::default()
                    },
                    |n, _: &()| n,
                    |_, c| c,
                    |_, ctx| ctx,
                )
            }));
            assert!(result.is_err());
        })
        .unwrap();
    assert!(parent.back_handler().back());
    assert_eq!(parent.instance_store().len().unwrap(), 1);
    assert!(!child_instances.is_destroyed().unwrap());
    assert_eq!(parent.state_store().save().unwrap(), saved);
    let retry = children(
        &parent,
        &SimpleNavigation::<State>::new(),
        || panic!("rollback must restore"),
        persistent(),
        |_, e| e.clone(),
        |_, c| c,
        |_, ctx| ctx,
    )
    .unwrap();
    let restored_instances = retry.get()[0].instance().unwrap().instance_store();
    assert!(child_instances.ptr_eq(&restored_instances));
    assert!(Rc::ptr_eq(
        &sentinel,
        &restored_instances
            .get_or_create("sentinel", || 99u8)
            .unwrap()
    ));
}

#[test]
fn pre_engine_panics_preserve_original_unwind_when_source_cancellation_panics() {
    const MODE: &str = "DECOMPOSE_GENERIC_EARLY_PANIC_CASE";
    let Ok(mode) = std::env::var(MODE) else {
        let mut failures = Vec::new();
        for mode in ["initial", "decoder", "clone"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "pre_engine_panics_preserve_original_unwind_when_source_cancellation_panics",
                    "--nocapture",
                ])
                .env(MODE, mode)
                .output()
                .unwrap();
            if !output.status.success() {
                failures.push((mode, output.status));
            }
        }
        assert!(
            failures.is_empty(),
            "early panic subprocess failures: {failures:?}"
        );
        return;
    };
    let saved = if mode == "decoder" {
        std::collections::BTreeMap::from([(
            "DefaultChildren".into(),
            envelope(&[1, 2], 1, &[("child/0", b"DCS1\0\0\0\0")]),
        )])
    } else {
        Default::default()
    };
    let parent = restored(saved.clone(), None);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match mode.as_str() {
        "initial" => {
            let _ = children(
                &parent,
                &CancelPanicSource,
                || -> State { panic!("original initial state") },
                ChildrenOptions::default(),
                |n, _| n,
                |_, c| c,
                |_, ctx| ctx,
            );
        }
        "decoder" => {
            let options = ChildrenOptions {
                codec: Some(NavStateCodec::new(
                    |_: &State| Ok(None),
                    |_| -> Result<Option<State>, String> { panic!("original decoder") },
                )),
                ..Default::default()
            };
            let _ = children(
                &parent,
                &CancelPanicSource,
                || panic!("decoder must not fallback"),
                options,
                |n, _| n,
                |_, c| c,
                |_, ctx| ctx,
            );
        }
        "clone" => {
            let initial = CloneState {
                records: vec![entry(1, ChildStatus::Resumed)],
                callback: Rc::new(|| panic!("original pre-engine clone")),
            };
            let _ = children(
                &parent,
                &CancelPanicSource,
                || initial,
                ChildrenOptions::default(),
                |n, _| n,
                |_, c| c,
                |_, ctx| ctx,
            );
        }
        _ => unreachable!(),
    }))
    .expect_err("initialization must panic");
    let expected = match mode.as_str() {
        "initial" => "original initial state",
        "decoder" => "original decoder",
        _ => "original pre-engine clone",
    };
    assert_eq!(panicked.downcast_ref::<&str>().copied(), Some(expected));
    assert!(
        !parent
            .state_store()
            .is_registered("DefaultChildren")
            .unwrap()
    );
    assert_eq!(parent.state_store().save().unwrap(), saved);
    assert_eq!(parent.instance_store().len().unwrap(), 0);
    assert_eq!(parent.back_handler().handler_count(), 0);
}
