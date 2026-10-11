use decompose_rs::router::webhistory::*;
use decompose_rs::value::MutableValue;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn route(key: u8) -> Route<u8> {
    Route::from_key(key, key.to_string())
}
fn nav(source: &MutableValue<Vec<Route<u8>>>, decodes: &Rc<Cell<usize>>) -> Rc<dyn WebNavigation> {
    let target = source.clone();
    let count = decodes.clone();
    Rc::new(TypedWebNavigation::new(
        source.value(),
        RouteCodec::new(
            |key| Ok(vec![*key]),
            move |bytes| {
                count.set(count.get() + 1);
                bytes
                    .first()
                    .copied()
                    .ok_or_else(|| HistoryError::Codec("obsolete".into()))
            },
        ),
        move |keys, done| {
            target.set(keys.into_iter().map(|key| route(*key)).collect());
            done(Ok(()));
        },
    ))
}
fn state(index: usize, version: Option<&str>, key: u8) -> HistoryEntry {
    HistoryEntry {
        state: Some(HistoryState {
            index,
            version: version.map(str::to_owned),
            nodes: vec![HistoryNode {
                key: vec![key],
                path: key.to_string(),
                children: vec![],
            }],
        }),
        url: format!("/{key}"),
    }
}

#[test]
fn queued_recreation_uses_original_pop_url_and_preserves_latest_host_position() {
    let source = MutableValue::new(vec![route(3)]);
    let replacement = MutableValue::new(vec![route(9)]);
    let decodes = Rc::new(Cell::new(0));
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![
                state(0, Some("old"), 1),
                state(1, Some("old"), 2),
                state(2, Some("new"), 3),
            ],
            2,
        )
        .unwrap(),
    );
    let pending = Rc::new(RefCell::new(Vec::<HistoryRecreationCompletion>::new()));
    let out = pending.clone();
    let requests = Rc::new(RefCell::new(Vec::new()));
    let log = requests.clone();
    let _binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |request, done| {
                log.borrow_mut().push(request.url);
                out.borrow_mut().push(done);
            })),
        },
    )
    .unwrap();
    backend.go(-2, Box::new(|r| r.unwrap()));
    backend.go(1, Box::new(|r| r.unwrap()));
    let done = pending.borrow_mut().remove(0);
    done(Ok(HistoryRoot::new(nav(&replacement, &decodes))));
    assert_eq!(*requests.borrow(), ["/1", "/2"]);
    assert_eq!(backend.position(), 1);
    assert_eq!(backend.entries()[0].url, "/9");
    let second = MutableValue::new(vec![route(8)]);
    let done = pending.borrow_mut().remove(0);
    done(Ok(HistoryRoot::new(nav(&second, &decodes))));
    assert_eq!(
        backend
            .entries()
            .iter()
            .map(|entry| entry.url.as_str())
            .collect::<Vec<_>>(),
        ["/9", "/8", "/3"]
    );
    assert_eq!(backend.position(), 1);
    backend.go(1, Box::new(|r| r.unwrap()));
    assert_eq!(*second.get().last().unwrap().key, 3);
    assert_eq!(backend.position(), 2);
}

#[test]
fn late_recreation_preserves_unmanaged_entry_and_updates_actual_managed_destination() {
    let source = MutableValue::new(vec![route(3)]);
    let replacement = MutableValue::new(vec![route(9)]);
    let decodes = Rc::new(Cell::new(0));
    let external = HistoryEntry {
        state: None,
        url: "/external".into(),
    };
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![
                external.clone(),
                state(0, Some("old"), 1),
                state(1, Some("new"), 3),
            ],
            2,
        )
        .unwrap(),
    );
    let pending = Rc::new(RefCell::new(None::<HistoryRecreationCompletion>));
    let out = pending.clone();
    let _binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |request, done| {
                assert_eq!(request.url, "/1");
                *out.borrow_mut() = Some(done);
            })),
        },
    )
    .unwrap();
    backend.go(-1, Box::new(|r| r.unwrap()));
    backend.go(-1, Box::new(|r| r.unwrap()));
    let done = pending.borrow_mut().take().unwrap();
    done(Ok(HistoryRoot::new(nav(&replacement, &decodes))));
    assert_eq!(backend.entries()[0], external);
    assert_eq!(backend.entries()[1].url, "/9");
    assert_eq!(backend.entries()[2].url, "/3");
    assert_eq!(backend.position(), 0);
    backend.go(1, Box::new(|r| r.unwrap()));
    assert_eq!(*replacement.get().last().unwrap().key, 9);
    assert_eq!(backend.position(), 1);
}

struct DeferredRecreations {
    backend: Rc<MemoryHistoryBackend>,
    source: MutableValue<Vec<Route<u8>>>,
    requests: Rc<RefCell<Vec<String>>>,
    pending: Rc<RefCell<std::collections::VecDeque<HistoryRecreationCompletion>>>,
    binding: HistoryController,
}
impl DeferredRecreations {
    fn new(entries: Vec<HistoryEntry>, position: usize) -> Self {
        let backend = Rc::new(MemoryHistoryBackend::with_entries(entries, position).unwrap());
        let source = MutableValue::new(vec![route(3)]);
        let requests = Rc::new(RefCell::new(Vec::new()));
        let pending = Rc::new(RefCell::new(std::collections::VecDeque::new()));
        let log = requests.clone();
        let out = pending.clone();
        let binding = HistoryController::attach_with_options(
            nav(&source, &Rc::new(Cell::new(0))),
            backend.clone(),
            HistoryOptions {
                version: Some("new".into()),
                recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |request, done| {
                    log.borrow_mut().push(request.url);
                    out.borrow_mut().push_back(done);
                })),
            },
        )
        .unwrap();
        Self {
            backend,
            source,
            requests,
            pending,
            binding,
        }
    }
    fn complete(&self, key: u8) -> MutableValue<Vec<Route<u8>>> {
        let source = MutableValue::new(vec![route(key)]);
        let done = self.pending.borrow_mut().pop_front().unwrap();
        done(Ok(HistoryRoot::new(nav(&source, &Rc::new(Cell::new(0))))));
        source
    }
}

#[test]
fn several_queued_destinations_use_identity_despite_duplicate_and_gapped_managed_indices() {
    let external = HistoryEntry {
        state: None,
        url: "/external".into(),
    };
    let host = DeferredRecreations::new(
        vec![
            external.clone(),
            state(10, Some("old"), 1),
            state(10, Some("old"), 2),
            state(80, Some("old"), 4),
            state(81, Some("new"), 3),
        ],
        4,
    );
    host.backend.go(-3, Box::new(|r| r.unwrap()));
    host.backend.go(1, Box::new(|r| r.unwrap()));
    host.backend.go(1, Box::new(|r| r.unwrap()));
    let _first = host.complete(9);
    assert_eq!(*host.requests.borrow(), ["/1", "/2"]);
    assert_eq!(host.backend.position(), 3);
    let _second = host.complete(8);
    assert_eq!(*host.requests.borrow(), ["/1", "/2", "/4"]);
    let third = host.complete(7);
    assert_eq!(host.backend.position(), 3);
    assert_eq!(host.backend.entries()[0], external);
    assert_eq!(
        host.backend
            .entries()
            .iter()
            .map(|entry| entry.url.as_str())
            .collect::<Vec<_>>(),
        ["/external", "/9", "/8", "/7", "/3"]
    );
    assert_eq!(host.backend.entries()[1].state.as_ref().unwrap().index, 10);
    assert_eq!(host.backend.entries()[2].state.as_ref().unwrap().index, 10);
    assert_eq!(host.backend.entries()[3].state.as_ref().unwrap().index, 80);
    host.backend.go(1, Box::new(|r| r.unwrap()));
    assert_eq!(*third.get().last().unwrap().key, 3);
}

#[test]
fn failed_deferred_recreation_drains_queue_without_undoing_later_host_navigation() {
    let host = DeferredRecreations::new(
        vec![
            state(0, Some("old"), 1),
            state(1, Some("old"), 2),
            state(2, Some("new"), 3),
        ],
        2,
    );
    host.backend.go(-2, Box::new(|r| r.unwrap()));
    host.backend.go(1, Box::new(|r| r.unwrap()));
    let done = host.pending.borrow_mut().pop_front().unwrap();
    done(Err(HistoryError::Backend("factory failed".into())));
    assert_eq!(*host.requests.borrow(), ["/1", "/2"]);
    assert_eq!(host.backend.position(), 1);
    assert_eq!(host.backend.entries()[0].url, "/1");
    assert_eq!(
        host.binding.errors().get().as_ref(),
        &Some(HistoryError::Backend("factory failed".into()))
    );
    let second = host.complete(8);
    assert_eq!(host.backend.entries()[1].url, "/8");
    host.backend.go(1, Box::new(|r| r.unwrap()));
    assert_eq!(*second.get().last().unwrap().key, 3);
}

#[test]
fn failure_and_model_changes_while_unmanaged_preserve_external_history() {
    let external = HistoryEntry {
        state: None,
        url: "/external".into(),
    };
    for success in [false, true] {
        let host = DeferredRecreations::new(
            vec![
                external.clone(),
                state(0, Some("old"), 1),
                state(1, Some("new"), 3),
            ],
            2,
        );
        host.backend.go(-1, Box::new(|r| r.unwrap()));
        host.backend.go(-1, Box::new(|r| r.unwrap()));
        let active = if success {
            host.complete(9)
        } else {
            let done = host.pending.borrow_mut().pop_front().unwrap();
            done(Err(HistoryError::Backend("factory failed".into())));
            host.source.clone()
        };
        active.set(vec![route(10)]);
        assert_eq!(host.backend.entries()[0], external);
        assert_eq!(host.backend.position(), 0);
        assert_eq!(
            host.backend.entries()[1].url,
            if success { "/9" } else { "/1" }
        );
        host.backend.go(2, Box::new(|r| r.unwrap()));
        assert_eq!(*active.get().last().unwrap().key, 3);
    }
}

#[test]
fn cancellation_discards_queued_targets_and_late_completion_releases_root() {
    for drop_controller in [false, true] {
        let host = DeferredRecreations::new(
            vec![
                state(0, Some("old"), 1),
                state(1, Some("old"), 2),
                state(2, Some("new"), 3),
            ],
            2,
        );
        host.backend.go(-2, Box::new(|r| r.unwrap()));
        host.backend.go(1, Box::new(|r| r.unwrap()));
        let original = host.backend.entries();
        let done = host.pending.borrow_mut().pop_front().unwrap();
        if drop_controller {
            drop(host.binding);
        } else {
            host.binding.cancel();
        }
        let replacement = MutableValue::new(vec![route(9)]);
        let navigation = nav(&replacement, &Rc::new(Cell::new(0)));
        let weak = Rc::downgrade(&navigation);
        done(Ok(HistoryRoot::new(navigation)));
        assert!(weak.upgrade().is_none());
        assert_eq!(*host.requests.borrow(), ["/1"]);
        assert_eq!(host.backend.entries(), original);
        assert_eq!(host.backend.position(), 1);
        assert_eq!(host.backend.listener_count(), 0);
    }
}

#[test]
fn changed_or_discarded_destination_rejects_late_completion_without_replacing_current_entry() {
    for discarded in [false, true] {
        let host = DeferredRecreations::new(
            vec![
                state(0, Some("new"), 0),
                state(1, Some("old"), 1),
                state(2, Some("new"), 3),
            ],
            2,
        );
        host.backend.go(-1, Box::new(|r| r.unwrap()));
        let external = HistoryEntry {
            state: None,
            url: "/external".into(),
        };
        if discarded {
            host.backend.go(-1, Box::new(|r| r.unwrap()));
            host.backend.push(external.clone()).unwrap();
        } else {
            host.backend.replace(external.clone()).unwrap();
        }
        let original = host.backend.entries();
        let _replacement = host.complete(9);
        assert_eq!(host.backend.entries(), original);
        assert_eq!(host.backend.current().unwrap(), external);
        assert_eq!(host.backend.position(), 1);
        assert!(matches!(
            host.binding.errors().get().as_ref(),
            Some(HistoryError::Backend(_))
        ));
    }
}

#[test]
fn reentrant_pop_after_synchronous_recreation_completion_retains_its_destination() {
    let source = MutableValue::new(vec![route(3)]);
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![
                state(0, Some("old"), 1),
                state(1, Some("old"), 2),
                state(2, Some("new"), 3),
            ],
            2,
        )
        .unwrap(),
    );
    let log = Rc::new(RefCell::new(Vec::new()));
    let requests = log.clone();
    let host = backend.clone();
    let last_root = Rc::new(RefCell::new(None));
    let output = last_root.clone();
    let _binding = HistoryController::attach_with_options(
        nav(&source, &Rc::new(Cell::new(0))),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |request, done| {
                let first = request.url == "/1";
                requests.borrow_mut().push(request.url);
                let fresh = MutableValue::new(vec![route(if first { 9 } else { 8 })]);
                done(Ok(HistoryRoot::new(nav(&fresh, &Rc::new(Cell::new(0))))));
                *output.borrow_mut() = Some(fresh);
                if first {
                    host.go(1, Box::new(|r| r.unwrap()));
                }
            })),
        },
    )
    .unwrap();
    backend.go(-2, Box::new(|r| r.unwrap()));
    assert_eq!(*log.borrow(), ["/1", "/2"]);
    assert_eq!(backend.position(), 1);
    assert_eq!(backend.entries()[0].url, "/9");
    assert_eq!(backend.entries()[1].url, "/8");
    backend.go(1, Box::new(|r| r.unwrap()));
    assert_eq!(
        *last_root
            .borrow()
            .as_ref()
            .unwrap()
            .get()
            .last()
            .unwrap()
            .key,
        3
    );
}

#[test]
fn backend_destination_identity_survives_replacement_and_rejects_reused_physical_slot() {
    let original = state(10, Some("old"), 1);
    let backend = MemoryHistoryBackend::with_entries(
        vec![
            state(9, Some("new"), 0),
            original.clone(),
            state(11, Some("new"), 3),
        ],
        1,
    )
    .unwrap();
    let destination = backend.current_destination().unwrap();
    assert_eq!(destination.location, destination.location.clone());
    assert_ne!(destination.location, HistoryLocation::new());
    backend.go(1, Box::new(|r| r.unwrap()));
    let replacement = state(10, Some("new"), 9);
    backend
        .replace_destination(&destination, replacement.clone())
        .unwrap();
    assert_eq!(backend.position(), 2);
    assert_eq!(backend.entries()[1], replacement);
    backend.go(-1, Box::new(|r| r.unwrap()));
    assert_eq!(
        backend.current_destination().unwrap().location,
        destination.location
    );
    // Recreate exactly the old bytes and URL in the same physical slot, on a new
    // forward branch. Snapshot equality alone must not authorize this write.
    backend.go(-1, Box::new(|r| r.unwrap()));
    backend.push(original).unwrap();
    let entries = backend.entries();
    assert!(
        backend
            .replace_destination(&destination, replacement)
            .is_err()
    );
    assert_eq!(backend.entries(), entries);
    assert_eq!(backend.position(), 1);
    assert_ne!(
        backend.current_destination().unwrap().location,
        destination.location
    );
}

#[test]
fn pop_destination_snapshot_precedes_reentrant_listener_writes() {
    let original = state(0, Some("old"), 1);
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(vec![original.clone(), state(1, Some("new"), 3)], 1)
            .unwrap(),
    );
    let writer = backend.clone();
    let _first = backend.subscribe(Rc::new(move |_| {
        writer.replace(state(0, Some("new"), 9)).unwrap();
    }));
    let received = Rc::new(RefCell::new(None));
    let output = received.clone();
    let _second = backend.subscribe(Rc::new(move |destination| {
        *output.borrow_mut() = Some(destination);
    }));
    backend.go(-1, Box::new(|r| r.unwrap()));
    let destination = received.borrow_mut().take().unwrap();
    assert_eq!(destination.entry, original);
    assert_eq!(backend.current().unwrap().url, "/9");
    assert!(
        backend
            .replace_destination(&destination, state(0, Some("new"), 8))
            .is_err()
    );
    assert_eq!(backend.current().unwrap().url, "/9");
}

#[test]
fn unsupported_destination_replacement_reports_error_and_keeps_old_binding_usable() {
    struct CurrentOnly(MemoryHistoryBackend);
    impl HistoryBackend for CurrentOnly {
        fn current_destination(&self) -> Option<HistoryDestination> {
            self.0.current_destination()
        }
        fn replace(&self, entry: HistoryEntry) -> Result<(), HistoryError> {
            self.0.replace(entry)
        }
        fn replace_destination(
            &self,
            _: &HistoryDestination,
            _: HistoryEntry,
        ) -> Result<(), HistoryError> {
            Err(HistoryError::Backend(
                "destination replacement unsupported".into(),
            ))
        }
        fn push(&self, entry: HistoryEntry) -> Result<(), HistoryError> {
            self.0.push(entry)
        }
        fn go(&self, delta: isize, completion: HistoryCompletion) {
            self.0.go(delta, completion);
        }
        fn subscribe(
            &self,
            observer: Rc<dyn Fn(HistoryDestination)>,
        ) -> decompose_rs::value::Subscription {
            self.0.subscribe(observer)
        }
    }
    let backend = Rc::new(CurrentOnly(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 3)],
            1,
        )
        .unwrap(),
    ));
    let source = MutableValue::new(vec![route(3)]);
    let replacement = MutableValue::new(vec![route(9)]);
    let new_navigation = nav(&replacement, &Rc::new(Cell::new(0)));
    let binding = HistoryController::attach_with_options(
        nav(&source, &Rc::new(Cell::new(0))),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |_, done| {
                done(Ok(HistoryRoot::new(new_navigation.clone())))
            })),
        },
    )
    .unwrap();
    backend.go(-1, Box::new(|r| r.unwrap()));
    assert_eq!(backend.0.entries()[0].url, "/1");
    assert_eq!(backend.0.position(), 1);
    assert_eq!(
        binding.errors().get().as_ref(),
        &Some(HistoryError::Backend(
            "destination replacement unsupported".into()
        ))
    );
    source.set(vec![route(3), route(4)]);
    assert_eq!(backend.current().unwrap().url, "/4");
    replacement.set(vec![route(9), route(10)]);
    assert_eq!(backend.current().unwrap().url, "/4");
}

#[test]
fn repeated_queued_visits_accept_controller_rewrites_and_keep_original_urls() {
    let host = DeferredRecreations::new(
        vec![
            state(0, Some("old"), 1),
            state(1, Some("old"), 2),
            state(2, Some("new"), 3),
        ],
        2,
    );
    host.backend.go(-2, Box::new(|r| r.unwrap()));
    host.backend.go(1, Box::new(|r| r.unwrap()));
    host.backend.go(-1, Box::new(|r| r.unwrap()));
    let _first = host.complete(9);
    let _second = host.complete(8);
    let third = host.complete(7);
    assert_eq!(*host.requests.borrow(), ["/1", "/2", "/1"]);
    assert!(
        host.binding.errors().get().is_none(),
        "error={:?}, entries={:?}",
        host.binding.errors().get(),
        host.backend.entries()
    );
    assert_eq!(
        host.backend
            .entries()
            .iter()
            .map(|entry| entry.url.as_str())
            .collect::<Vec<_>>(),
        ["/7", "/8", "/3"]
    );
    assert_eq!(host.backend.position(), 0);
    third.set(vec![route(7), route(6)]);
    assert_eq!(host.backend.entries()[0].url, "/7");
    assert_eq!(host.backend.current().unwrap().url, "/6");
}

#[test]
fn reentrant_error_navigation_runs_after_older_queued_pop() {
    let source = MutableValue::new(vec![route(3)]);
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![
                state(0, Some("old"), 1),
                state(1, Some("old"), 2),
                state(2, Some("new"), 3),
            ],
            2,
        )
        .unwrap(),
    );
    let pending = Rc::new(RefCell::new(std::collections::VecDeque::<
        HistoryRecreationCompletion,
    >::new()));
    let output = pending.clone();
    let host = backend.clone();
    let _binding = HistoryController::attach_with_options_and_error_handler(
        nav(&source, &Rc::new(Cell::new(0))),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |_, done| {
                output.borrow_mut().push_back(done)
            })),
        },
        move |_| host.go(1, Box::new(|r| r.unwrap())),
    )
    .unwrap();
    backend.go(-2, Box::new(|r| r.unwrap()));
    backend.go(1, Box::new(|r| r.unwrap()));
    let done = pending.borrow_mut().pop_front().unwrap();
    done(Err(HistoryError::Backend("factory failed".into())));
    let fresh = MutableValue::new(vec![route(8)]);
    let done = pending.borrow_mut().pop_front().unwrap();
    done(Ok(HistoryRoot::new(nav(&fresh, &Rc::new(Cell::new(0))))));
    assert_eq!(backend.position(), 2);
    assert_eq!(backend.current().unwrap().url, "/3");
    assert_eq!(*fresh.get().last().unwrap().key, 3);
    assert_eq!(backend.entries()[0].url, "/1");
    assert_eq!(backend.entries()[1].url, "/8");
    fresh.set(vec![route(3), route(6)]);
    assert_eq!(backend.entries()[0].url, "/1");
    assert_eq!(backend.entries()[2].url, "/3");
    assert_eq!(backend.position(), 3);
}

#[test]
fn old_root_drop_panic_still_binds_replacement_model_and_lifecycle() {
    struct PanicOnDrop(Rc<dyn WebNavigation>);
    impl WebNavigation for PanicOnDrop {
        fn history(&self) -> Result<Vec<HistoryItem>, HistoryError> {
            self.0.history()
        }
        fn subscribe(&self, observer: Rc<dyn Fn()>) -> decompose_rs::value::Subscription {
            self.0.subscribe(observer)
        }
        fn navigate(&self, keys: Vec<Vec<u8>>, done: HistoryCompletion) {
            self.0.navigate(keys, done);
        }
    }
    impl Drop for PanicOnDrop {
        fn drop(&mut self) {
            panic!("old root destructor");
        }
    }
    let source = MutableValue::new(vec![route(2)]);
    let fresh = MutableValue::new(vec![route(9)]);
    let new_navigation = nav(&fresh, &Rc::new(Cell::new(0)));
    let lifecycle = decompose_rs::LifecycleRegistry::new();
    lifecycle
        .move_to(decompose_rs::LifecycleState::Resumed)
        .unwrap();
    let root_lifecycle = lifecycle.clone();
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let controller = HistoryController::attach_with_options(
        Rc::new(PanicOnDrop(nav(&source, &Rc::new(Cell::new(0))))),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |_, done| {
                done(Ok(
                    HistoryRoot::new(new_navigation.clone()).with_lifecycle(&root_lifecycle)
                ));
            })),
        },
    )
    .unwrap();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        backend.go(-1, Box::new(|_| {}))
    }))
    .unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"old root destructor"));
    fresh.set(vec![route(9), route(10)]);
    assert_eq!(backend.current().unwrap().url, "/10");
    lifecycle.destroy().unwrap();
    assert_eq!(backend.listener_count(), 0);
    fresh.set(vec![route(11)]);
    assert_eq!(backend.current().unwrap().url, "/10");
    drop(controller);
}
#[test]
fn mismatch_is_detected_before_codec_and_reject_restores_committed_position() {
    let source = MutableValue::new(vec![route(2)]);
    let decodes = Rc::new(Cell::new(0));
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            ..HistoryOptions::default()
        },
    )
    .unwrap();
    backend.go(-1, Box::new(|result| result.unwrap()));
    assert_eq!(decodes.get(), 0);
    assert_eq!(backend.position(), 1);
    assert!(matches!(
        binding.errors().get().as_ref(),
        Some(HistoryError::VersionMismatch(_))
    ));
}
#[test]
fn recreation_cancels_old_model_and_binds_new_model() {
    let source = MutableValue::new(vec![route(2)]);
    let replacement = MutableValue::new(vec![route(9)]);
    let decodes = Rc::new(Cell::new(0));
    let newnav = nav(&replacement, &decodes);
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let pending = Rc::new(RefCell::new(None::<HistoryRecreationCompletion>));
    let out = pending.clone();
    let binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |request, done| {
                assert_eq!(request.mismatch.saved, Some("old".into()));
                assert_eq!(request.url, "/1");
                *out.borrow_mut() = Some(done);
            })),
        },
    )
    .unwrap();
    backend.go(-1, Box::new(|result| result.unwrap()));
    source.set(vec![route(2), route(3)]);
    assert_eq!(backend.entries().len(), 2);
    pending.borrow_mut().take().unwrap()(Ok(HistoryRoot::new(newnav)));
    assert_eq!(backend.current().unwrap().url, "/9");
    assert_eq!(decodes.get(), 0);
    source.set(vec![route(2), route(4)]);
    assert_eq!(backend.current().unwrap().url, "/9");
    replacement.set(vec![route(9), route(10)]);
    assert_eq!(backend.current().unwrap().url, "/10");
    assert_eq!(backend.listener_count(), 1);
    binding.cancel();
    assert_eq!(backend.listener_count(), 0);
}

#[test]
fn matching_version_round_trip_and_all_writes_keep_version() {
    let source = MutableValue::new(vec![route(1)]);
    let decodes = Rc::new(Cell::new(0));
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("v1".into()),
            ..HistoryOptions::default()
        },
    )
    .unwrap();
    source.set(vec![route(1), route(2)]);
    assert!(
        backend
            .entries()
            .iter()
            .all(|entry| entry.state.as_ref().unwrap().version.as_deref() == Some("v1"))
    );
    backend.go(-1, Box::new(|result| result.unwrap()));
    backend.go(1, Box::new(|result| result.unwrap()));
    assert_eq!(*source.get().last().unwrap().key, 2);
    assert_eq!(decodes.get(), 3);
    source.set(vec![route(8), route(9)]);
    assert!(
        backend
            .entries()
            .iter()
            .all(|entry| entry.state.as_ref().unwrap().version.as_deref() == Some("v1"))
    );
}

#[test]
fn unversioned_legacy_entries_match_only_unversioned_options() {
    for version in [None, Some("current")] {
        let source = MutableValue::new(vec![route(2)]);
        let decodes = Rc::new(Cell::new(0));
        let backend = Rc::new(
            MemoryHistoryBackend::with_entries(vec![state(0, None, 1), state(1, None, 2)], 1)
                .unwrap(),
        );
        let binding = HistoryController::attach_with_options(
            nav(&source, &decodes),
            backend.clone(),
            HistoryOptions {
                version: version.map(str::to_owned),
                ..HistoryOptions::default()
            },
        )
        .unwrap();
        backend.go(-1, Box::new(|result| result.unwrap()));
        assert_eq!(decodes.get(), usize::from(version.is_none()));
        assert_eq!(backend.position(), usize::from(version.is_some()));
        binding.cancel();
    }
}
#[test]
fn mismatch_guard_denial_uses_host_offset_for_different_initial_stack_sizes() {
    for keys in [vec![9], vec![8, 9], vec![7, 8, 9, 10]] {
        let source = MutableValue::new(keys.into_iter().map(route).collect());
        let decodes = Rc::new(Cell::new(0));
        let recreated = Rc::new(Cell::new(0));
        let called = recreated.clone();
        let inner = nav(&source, &decodes);
        struct Guarded(Rc<dyn WebNavigation>);
        impl WebNavigation for Guarded {
            fn history(&self) -> Result<Vec<HistoryItem>, HistoryError> {
                self.0.history()
            }
            fn subscribe(&self, observer: Rc<dyn Fn()>) -> decompose_rs::value::Subscription {
                self.0.subscribe(observer)
            }
            fn navigate(&self, keys: Vec<Vec<u8>>, done: HistoryCompletion) {
                self.0.navigate(keys, done)
            }
            fn before_navigate(&self) -> bool {
                false
            }
            fn before_navigate_to(&self, _: &[HistoryNode]) -> Result<bool, HistoryError> {
                panic!("incompatible target must not be decoded by guard")
            }
        }
        let backend = Rc::new(
            MemoryHistoryBackend::with_entries(
                vec![
                    state(0, Some("old"), 1),
                    state(1, Some("old"), 2),
                    state(2, Some("old"), 3),
                ],
                2,
            )
            .unwrap(),
        );
        let _binding = HistoryController::attach_with_options(
            Rc::new(Guarded(inner)),
            backend.clone(),
            HistoryOptions {
                version: Some("new".into()),
                recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |_, _| {
                    called.set(called.get() + 1)
                })),
            },
        )
        .unwrap();
        backend.go(-1, Box::new(|result| result.unwrap()));
        assert_eq!(backend.position(), 2);
        assert_eq!(recreated.get(), 0);
        assert_eq!(decodes.get(), 0);
    }
}
#[test]
fn failed_recreation_restores_old_observers_and_allows_retry() {
    let source = MutableValue::new(vec![route(2)]);
    let replacement = MutableValue::new(vec![route(9)]);
    let decodes = Rc::new(Cell::new(0));
    let newnav = nav(&replacement, &decodes);
    let attempts = Rc::new(Cell::new(0));
    let count = attempts.clone();
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |_, done| {
                count.set(count.get() + 1);
                if count.get() == 1 {
                    done(Err(HistoryError::Backend("factory failed".into())));
                } else {
                    done(Ok(HistoryRoot::new(newnav.clone())));
                }
            })),
        },
    )
    .unwrap();
    backend.go(-1, Box::new(|result| result.unwrap()));
    assert_eq!(backend.position(), 1);
    assert!(
        matches!(binding.errors().get().as_ref(),Some(HistoryError::Backend(message)) if message=="factory failed")
    );
    source.set(vec![route(2), route(3)]);
    assert_eq!(backend.current().unwrap().url, "/3");
    backend.go(-2, Box::new(|result| result.unwrap()));
    assert_eq!(backend.current().unwrap().url, "/9");
    assert_eq!(attempts.get(), 2);
}
#[test]
fn cancel_during_recreation_releases_hook_and_ignores_late_completion() {
    let source = MutableValue::new(vec![route(2)]);
    let replacement = MutableValue::new(vec![route(9)]);
    let decodes = Rc::new(Cell::new(0));
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let captured = Rc::new(());
    let weak = Rc::downgrade(&captured);
    let pending = Rc::new(RefCell::new(None::<HistoryRecreationCompletion>));
    let out = pending.clone();
    let binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |_, done| {
                let _ = &captured;
                *out.borrow_mut() = Some(done);
            })),
        },
    )
    .unwrap();
    backend.go(-1, Box::new(|result| result.unwrap()));
    binding.cancel();
    assert!(weak.upgrade().is_none());
    assert_eq!(backend.listener_count(), 0);
    pending.borrow_mut().take().unwrap()(Ok(HistoryRoot::new(nav(&replacement, &decodes))));
    replacement.set(vec![route(9), route(10)]);
    source.set(vec![route(2), route(4)]);
    assert_eq!(backend.current().unwrap().url, "/1");
}
#[test]
fn reentrant_recreation_updates_are_queued_and_bind_latest_new_root() {
    let source = MutableValue::new(vec![route(2)]);
    let replacement = MutableValue::new(vec![route(9)]);
    let decodes = Rc::new(Cell::new(0));
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let newnav = nav(&replacement, &decodes);
    let old = source.clone();
    let fresh = replacement.clone();
    let _binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |_, done| {
                old.set(vec![route(5)]);
                done(Ok(HistoryRoot::new(newnav.clone())));
                fresh.set(vec![route(9), route(10)]);
            })),
        },
    )
    .unwrap();
    backend.go(-1, Box::new(|result| result.unwrap()));
    assert_eq!(backend.current().unwrap().url, "/10");
    replacement.set(vec![route(9), route(10), route(11)]);
    assert_eq!(backend.current().unwrap().url, "/11");
    assert_eq!(backend.listener_count(), 1);
}

#[test]
fn deferred_recreation_does_not_rewrite_a_later_host_target() {
    let source = MutableValue::new(vec![route(2)]);
    let replacement = MutableValue::new(vec![route(9)]);
    let decodes = Rc::new(Cell::new(0));
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let pending = Rc::new(RefCell::new(None::<HistoryRecreationCompletion>));
    let out = pending.clone();
    let _binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |_, done| {
                *out.borrow_mut() = Some(done)
            })),
        },
    )
    .unwrap();
    backend.go(-1, Box::new(|result| result.unwrap()));
    backend.go(1, Box::new(|result| result.unwrap()));
    pending.borrow_mut().take().unwrap()(Ok(HistoryRoot::new(nav(&replacement, &decodes))));
    assert_eq!(backend.entries()[0].url, "/9");
    assert_eq!(backend.entries()[1].url, "/2");
    assert_eq!(backend.position(), 1);
    assert_eq!(*replacement.get().last().unwrap().key, 2);
}

#[test]
fn recreation_factory_panic_preserves_panic_and_reconnects_old_model() {
    let source = MutableValue::new(vec![route(2)]);
    let decodes = Rc::new(Cell::new(0));
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let _binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(|_, _| panic!("factory panic"))),
        },
    )
    .unwrap();
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        backend.go(-1, Box::new(|_| {}))
    }))
    .unwrap_err();
    assert_eq!(error.downcast_ref::<&str>(), Some(&"factory panic"));
    source.set(vec![route(2), route(3)]);
    assert_eq!(backend.current().unwrap().url, "/3");
    assert_eq!(
        backend.current().unwrap().state.unwrap().version.as_deref(),
        Some("new")
    );
}

#[test]
fn recreation_detaches_old_lifecycle_and_binds_replacement_lifecycle() {
    use essenty_lifecycle::LifecycleRegistry;
    let old_lifecycle = LifecycleRegistry::new();
    let new_lifecycle = LifecycleRegistry::new();
    let source = MutableValue::new(vec![route(2)]);
    let replacement = MutableValue::new(vec![route(9)]);
    let decodes = Rc::new(Cell::new(0));
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let newnav = nav(&replacement, &decodes);
    let old = old_lifecycle.clone();
    let fresh = new_lifecycle.clone();
    let _binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |_, done| {
                old.destroy().unwrap();
                done(Ok(HistoryRoot::new(newnav.clone()).with_lifecycle(&fresh)));
            })),
        },
    )
    .unwrap()
    .with_lifecycle(&old_lifecycle);
    backend.go(-1, Box::new(|result| result.unwrap()));
    assert_eq!(backend.current().unwrap().url, "/9");
    assert_eq!(backend.listener_count(), 1);
    new_lifecycle.destroy().unwrap();
    assert_eq!(backend.listener_count(), 0);
    replacement.set(vec![route(9), route(10)]);
    assert_eq!(backend.current().unwrap().url, "/9");
}

#[test]
fn portable_semantic_trace_fixture_matches_history_contract() {
    for line in include_str!("fixtures/history-versioning.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split('|').collect();
        let version = |value: &str| (value != "-").then(|| value.to_owned());
        let source = MutableValue::new(
            fields[1]
                .split(',')
                .map(|key| route(key.parse().unwrap()))
                .collect::<Vec<_>>(),
        );
        let decodes = Rc::new(Cell::new(0));
        let target = source.clone();
        let count = decodes.clone();
        let allow = fields[4] == "true";
        let navigation = Rc::new(
            TypedWebNavigation::new(
                source.value(),
                RouteCodec::new(
                    |key| Ok(vec![*key]),
                    move |bytes| {
                        count.set(count.get() + 1);
                        Ok(bytes[0])
                    },
                ),
                move |keys, done| {
                    target.set(keys.into_iter().map(|key| route(*key)).collect());
                    done(Ok(()));
                },
            )
            .with_before_navigate(move || allow),
        );
        let saved = version(fields[2]);
        let backend = Rc::new(
            MemoryHistoryBackend::with_entries(
                vec![state(0, saved.as_deref(), 1), state(1, saved.as_deref(), 2)],
                1,
            )
            .unwrap(),
        );
        let _binding = HistoryController::attach_with_options(
            navigation,
            backend.clone(),
            HistoryOptions {
                version: version(fields[3]),
                ..HistoryOptions::default()
            },
        )
        .unwrap();
        backend.go(-1, Box::new(|result| result.unwrap()));
        assert_eq!(
            backend.position(),
            fields[5].parse::<usize>().unwrap(),
            "{} host position",
            fields[0]
        );
        assert_eq!(
            *source.get().last().unwrap().key,
            fields[6].parse::<u8>().unwrap(),
            "{} active key",
            fields[0]
        );
        assert_eq!(
            decodes.get(),
            fields[7].parse::<usize>().unwrap(),
            "{} decoding",
            fields[0]
        );
    }
}

#[test]
fn failed_recreation_reconnects_existing_root_lifecycle() {
    use essenty_lifecycle::LifecycleRegistry;
    let lifecycle = LifecycleRegistry::new();
    let source = MutableValue::new(vec![route(2)]);
    let decodes = Rc::new(Cell::new(0));
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let _binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(|_, done| {
                done(Err(HistoryError::Backend("failed".into())))
            })),
        },
    )
    .unwrap()
    .with_lifecycle(&lifecycle);
    backend.go(-1, Box::new(|result| result.unwrap()));
    lifecycle.destroy().unwrap();
    assert_eq!(backend.listener_count(), 0);
}

#[test]
fn portable_saved_root_gate_checks_versions_before_host_decoder() {
    for (current, saved, allowed) in [
        (None, None, true),
        (Some("v1"), Some("v1"), true),
        (Some("v2"), Some("v1"), false),
        (Some("v1"), None, false),
        (None, Some("v1"), false),
    ] {
        let options = HistoryOptions {
            version: current.map(str::to_owned),
            ..HistoryOptions::default()
        };
        let decodes = Cell::new(0);
        let result = options
            .validate_saved_version(saved)
            .map(|()| decodes.set(decodes.get() + 1));
        assert_eq!(result.is_ok(), allowed);
        assert_eq!(decodes.get(), usize::from(allowed));
        if let Err(mismatch) = result {
            assert_eq!(mismatch.saved.as_deref(), saved);
            assert_eq!(mismatch.current.as_deref(), current);
        }
    }
}

#[test]
fn invalid_replacement_history_keeps_old_binding_retryable() {
    let source = MutableValue::new(vec![route(2)]);
    let invalid = MutableValue::new(Vec::<Route<u8>>::new());
    let decodes = Rc::new(Cell::new(0));
    let newnav = nav(&invalid, &decodes);
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(move |_, done| {
                done(Ok(HistoryRoot::new(newnav.clone())))
            })),
        },
    )
    .unwrap();
    backend.go(-1, Box::new(|result| result.unwrap()));
    assert!(matches!(
        binding.errors().get().as_ref(),
        Some(HistoryError::EmptyHistory)
    ));
    assert_eq!(backend.position(), 1);
    source.set(vec![route(2), route(3)]);
    assert_eq!(backend.current().unwrap().url, "/3");
    assert_eq!(backend.listener_count(), 1);
}

#[test]
fn queued_recreated_root_drop_panic_does_not_abort_factory_unwind() {
    const MODE: &str = "DECOMPOSE_HISTORY_RECREATE_PANIC";
    if std::env::var_os(MODE).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "queued_recreated_root_drop_panic_does_not_abort_factory_unwind",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    struct PanickingRoot;
    impl Drop for PanickingRoot {
        fn drop(&mut self) {
            panic!("queued root drop");
        }
    }
    impl WebNavigation for PanickingRoot {
        fn history(&self) -> Result<Vec<HistoryItem>, HistoryError> {
            unreachable!()
        }
        fn subscribe(&self, _: Rc<dyn Fn()>) -> decompose_rs::value::Subscription {
            unreachable!()
        }
        fn navigate(&self, _: Vec<Vec<u8>>, _: HistoryCompletion) {
            unreachable!()
        }
    }
    let source = MutableValue::new(vec![route(2)]);
    let decodes = Rc::new(Cell::new(0));
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![state(0, Some("old"), 1), state(1, Some("new"), 2)],
            1,
        )
        .unwrap(),
    );
    let _binding = HistoryController::attach_with_options(
        nav(&source, &decodes),
        backend.clone(),
        HistoryOptions {
            version: Some("new".into()),
            recovery: HistoryRecoveryPolicy::Recreate(Rc::new(|_, done| {
                done(Ok(HistoryRoot::new(Rc::new(PanickingRoot))));
                panic!("original factory panic");
            })),
        },
    )
    .unwrap();
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        backend.go(-1, Box::new(|_| {}))
    }))
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<&str>(),
        Some(&"original factory panic")
    );
    source.set(vec![route(2), route(3)]);
    assert_eq!(backend.current().unwrap().url, "/3");
}
