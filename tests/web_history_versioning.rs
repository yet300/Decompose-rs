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
