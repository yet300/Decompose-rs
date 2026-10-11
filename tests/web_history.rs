use decompose_rs::router::webhistory::*;
use decompose_rs::value::MutableValue;
use decompose_rs::{ComponentContext, DefaultComponentContext};
use essenty_lifecycle::LifecycleRegistry;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn codec() -> RouteCodec<u8> {
    RouteCodec::new(
        |v| Ok(vec![*v]),
        |b| {
            b.first()
                .copied()
                .ok_or_else(|| HistoryError::Codec("empty".into()))
        },
    )
}
fn route(key: u8) -> Route<u8> {
    Route::new(Rc::new(key), format!("/{key}/"))
}
fn navigation(source: &MutableValue<Vec<Route<u8>>>) -> Rc<dyn WebNavigation> {
    let target = source.clone();
    Rc::new(TypedWebNavigation::new(
        source.value(),
        codec(),
        move |keys, done| {
            let old = target.get();
            target.set(
                keys.into_iter()
                    .map(|key| {
                        old.iter()
                            .find(|item| item.key == key)
                            .cloned()
                            .unwrap_or_else(|| route(*key))
                    })
                    .collect(),
            );
            done(Ok(()));
        },
    ))
}

#[test]
fn recursive_attachment_rewrites_existing_entry_and_preserves_index() {
    let source = MutableValue::new(vec![route(1)]);
    let backend = Rc::new(MemoryHistoryBackend::new());
    let first = HistoryController::attach(navigation(&source), backend.clone()).unwrap();
    source.set(vec![route(1), route(2)]);
    drop(first);
    let replacement = MutableValue::new(vec![route(8), route(9), route(10)]);
    let _binding = HistoryController::attach(navigation(&replacement), backend.clone()).unwrap();
    assert_eq!(backend.current().unwrap().url, "/10");
    assert_eq!(backend.current().unwrap().state.unwrap().index, 1);
    assert_eq!(backend.entries().len(), 2);
}

#[test]
fn push_pop_forward_and_rewrite_keep_browser_and_navigation_in_sync() {
    let source = MutableValue::new(vec![route(1)]);
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(navigation(&source), backend.clone()).unwrap();
    assert_eq!(backend.entries().len(), 1);
    source.set(vec![route(1), route(2), route(3)]);
    assert_eq!(backend.entries().len(), 3);
    backend.go(-1, Box::new(|r| r.unwrap()));
    assert_eq!(*source.get().last().unwrap().key, 2);
    backend.go(1, Box::new(|r| r.unwrap()));
    assert_eq!(*source.get().last().unwrap().key, 3);
    source.set(vec![route(1)]);
    assert_eq!(backend.position(), 0);
    source.set(vec![route(8), route(9)]);
    assert_eq!(backend.entries().len(), 2);
    assert_eq!(backend.entries()[1].url, "/9");
}

#[test]
fn nested_active_history_merges_routes_and_restores_children() {
    let child = MutableValue::new(vec![route(2)]);
    let child_nav = navigation(&child);
    let mut parent = route(1);
    parent.child = Some(child_nav);
    parent.parameters = vec![("q".into(), "old".into()), ("x".into(), "a b".into())];
    let source = MutableValue::new(vec![parent]);
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(navigation(&source), backend.clone()).unwrap();
    let mut item = route(3);
    item.parameters = vec![("q".into(), "a&б".into())];
    child.set(vec![route(2), item]);
    assert_eq!(backend.entries().len(), 2);
    assert_eq!(backend.entries()[1].url, "/1/3?q=a%26%D0%B1&x=a%20b");
    backend.go(-1, Box::new(|r| r.unwrap()));
    assert_eq!(*child.get().last().unwrap().key, 2);
}

#[test]
fn drop_disconnects_both_backend_and_navigation() {
    let source = MutableValue::new(vec![route(1)]);
    let backend = Rc::new(MemoryHistoryBackend::new());
    let binding = HistoryController::attach(navigation(&source), backend.clone()).unwrap();
    source.set(vec![route(1), route(2)]);
    drop(binding);
    source.set(vec![route(1), route(2), route(3)]);
    assert_eq!(backend.entries().len(), 2);
    backend.go(-1, Box::new(|r| r.unwrap()));
    assert_eq!(*source.get().last().unwrap().key, 3);
    assert_eq!(backend.listener_count(), 0);
}

#[test]
fn denial_returns_to_actual_previous_index_even_with_initial_back_stack() {
    let source = MutableValue::new(vec![route(1), route(2)]);
    let target = source.clone();
    let nav = Rc::new(
        TypedWebNavigation::new(source.value(), codec(), move |keys, done| {
            target.set(keys.into_iter().map(|key| route(*key)).collect());
            done(Ok(()));
        })
        .with_before_navigate(|| false),
    );
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(nav, backend.clone()).unwrap();
    source.set(vec![route(1), route(2), route(3)]);
    backend.go(-1, Box::new(|r| r.unwrap()));
    assert_eq!(backend.position(), 1);
    assert_eq!(*source.get().last().unwrap().key, 3);
}

#[test]
fn deferred_navigation_completion_keeps_feedback_suppressed() {
    let source = MutableValue::new(vec![route(1)]);
    let pending = Rc::new(RefCell::new(None::<(Vec<Rc<u8>>, HistoryCompletion)>));
    let output = pending.clone();
    let nav = Rc::new(TypedWebNavigation::new(
        source.value(),
        codec(),
        move |keys, done| {
            *output.borrow_mut() = Some((keys, done));
        },
    ));
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(nav, backend.clone()).unwrap();
    source.set(vec![route(1), route(2)]);
    backend.go(-1, Box::new(|r| r.unwrap()));
    let (keys, done) = pending.borrow_mut().take().unwrap();
    source.set(keys.into_iter().map(|key| route(*key)).collect());
    assert_eq!(backend.entries().len(), 2);
    done(Ok(()));
    source.set(vec![route(1), route(3)]);
    assert_eq!(backend.entries().len(), 2);
    assert_eq!(backend.entries()[1].url, "/3");
}

#[test]
fn error_handler_can_reenter_and_bad_history_remains_recoverable() {
    let source = MutableValue::new(vec![route(1)]);
    let backend = Rc::new(MemoryHistoryBackend::new());
    let target = source.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    let binding = HistoryController::attach_with_error_handler(
        navigation(&source),
        backend.clone(),
        move |error| {
            output.borrow_mut().push(error);
            target.set(vec![route(8)]);
        },
    )
    .unwrap();
    source.set(Vec::new());
    assert_eq!(*seen.borrow(), [HistoryError::EmptyHistory]);
    assert_eq!(
        binding.errors().get().as_ref(),
        &Some(HistoryError::EmptyHistory)
    );
    assert_eq!(backend.entries()[0].url, "/8");
}

#[test]
fn destroy_cancels_all_subscriptions_and_releases_navigation_captures() {
    let lifecycle = LifecycleRegistry::new();
    let source = MutableValue::new(vec![route(1)]);
    let backend = Rc::new(MemoryHistoryBackend::new());
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let target = source.clone();
    let nav = Rc::new(TypedWebNavigation::new(
        source.value(),
        codec(),
        move |keys, done| {
            let _ = &capture;
            target.set(keys.into_iter().map(|key| route(*key)).collect());
            done(Ok(()));
        },
    ));
    let binding = HistoryController::attach(nav, backend.clone())
        .unwrap()
        .with_lifecycle(&lifecycle);
    lifecycle.destroy().unwrap();
    assert_eq!(backend.listener_count(), 0);
    assert!(weak.upgrade().is_none());
    source.set(vec![route(1), route(2)]);
    assert_eq!(backend.entries().len(), 1);
    drop(binding);
    assert!(weak.upgrade().is_none());
}

#[test]
fn stack_adapter_supports_nonclone_config_and_queued_back_in_observer() {
    use decompose_rs::router::stack::*;
    #[derive(PartialEq, Eq)]
    struct Config(u8);
    let context = DefaultComponentContext::new(LifecycleRegistry::new());
    let navigator = StackNavigation::new();
    let stack = child_stack(
        &context,
        &navigator,
        vec![Config(1)],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    let codec = RouteCodec::new(|key: &Config| Ok(vec![key.0]), |bytes| Ok(Config(bytes[0])));
    let backend = Rc::new(MemoryHistoryBackend::new());
    let web = stack_web_navigation(navigator.clone(), stack.clone(), codec, true, |child| {
        RouteInfo::path(child.configuration.0.to_string())
    });
    let _binding = HistoryController::attach(Rc::new(web), backend.clone())
        .unwrap()
        .with_lifecycle(&context.lifecycle());
    navigator.push_or_panic(Config(2));
    let host = backend.clone();
    let once = Cell::new(true);
    let _observer = stack.subscribe(move |state| {
        if state.active.configuration.0 == 3 && once.replace(false) {
            host.go(-1, Box::new(|r| r.unwrap()));
        }
    });
    navigator.push_or_panic(Config(3));
    assert_eq!(stack.get().active.configuration.0, 2);
    assert_eq!(backend.entries().len(), 3);
    assert_eq!(backend.position(), 1);
    backend.go(1, Box::new(|r| r.unwrap()));
    assert_eq!(stack.get().active.configuration.0, 3);
}

struct DeferredHost {
    memory: MemoryHistoryBackend,
    pending: RefCell<Option<(isize, HistoryCompletion)>>,
}
impl DeferredHost {
    fn new() -> Self {
        Self {
            memory: MemoryHistoryBackend::new(),
            pending: RefCell::new(None),
        }
    }
    fn finish(&self) {
        let (delta, done) = self.pending.borrow_mut().take().unwrap();
        self.memory.go(delta, done);
    }
}
impl HistoryBackend for DeferredHost {
    fn current_destination(&self) -> Option<HistoryDestination> {
        self.memory.current_destination()
    }
    fn replace(&self, entry: HistoryEntry) -> Result<(), HistoryError> {
        self.memory.replace(entry)
    }
    fn replace_destination(
        &self,
        destination: &HistoryDestination,
        entry: HistoryEntry,
    ) -> Result<(), HistoryError> {
        self.memory.replace_destination(destination, entry)
    }
    fn push(&self, entry: HistoryEntry) -> Result<(), HistoryError> {
        self.memory.push(entry)
    }
    fn go(&self, delta: isize, done: HistoryCompletion) {
        *self.pending.borrow_mut() = Some((delta, done));
    }
    fn subscribe(
        &self,
        observer: Rc<dyn Fn(HistoryDestination)>,
    ) -> decompose_rs::value::Subscription {
        self.memory.subscribe(observer)
    }
}

#[test]
fn deferred_host_movement_resynchronizes_intervening_model_changes() {
    let source = MutableValue::new(vec![route(1)]);
    let backend = Rc::new(DeferredHost::new());
    let _binding = HistoryController::attach(navigation(&source), backend.clone()).unwrap();
    source.set(vec![route(1), route(2), route(3)]);
    source.set(vec![route(8), route(9)]);
    source.set(vec![route(8), route(9), route(10)]);
    backend.finish();
    assert_eq!(backend.memory.entries().len(), 3);
    assert_eq!(backend.memory.entries()[2].url, "/10");
}

#[test]
fn initial_multiple_items_create_one_entry_and_pop_clamps_at_managed_boundary() {
    let source = MutableValue::new(vec![route(1), route(2), route(3)]);
    let backend = Rc::new(
        MemoryHistoryBackend::with_entries(
            vec![
                HistoryEntry {
                    state: None,
                    url: "/external".into(),
                },
                HistoryEntry {
                    state: None,
                    url: "/app".into(),
                },
            ],
            1,
        )
        .unwrap(),
    );
    let _binding = HistoryController::attach(navigation(&source), backend.clone()).unwrap();
    assert_eq!(backend.entries().len(), 2);
    source.set(vec![route(1)]);
    assert_eq!(backend.position(), 1);
    assert_eq!(backend.entries()[0].url, "/external");
    assert_eq!(backend.entries()[1].state.as_ref().unwrap().nodes.len(), 1);
}

#[test]
fn only_active_nested_navigation_is_observed_and_switching_cancels_old_child() {
    let first = MutableValue::new(vec![route(2)]);
    let second = MutableValue::new(vec![route(4)]);
    let mut parent_one = route(1);
    parent_one.child = Some(navigation(&first));
    let mut parent_two = route(3);
    parent_two.child = Some(navigation(&second));
    let source = MutableValue::new(vec![parent_one.clone(), parent_two]);
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(navigation(&source), backend.clone()).unwrap();
    first.set(vec![route(2), route(5)]);
    assert_eq!(backend.entries().len(), 1);
    source.set(vec![parent_one]);
    assert_eq!(backend.entries()[0].url, "/1/5");
    second.set(vec![route(4), route(6)]);
    assert_eq!(backend.entries().len(), 1);
    first.set(vec![route(2), route(5), route(7)]);
    assert_eq!(backend.entries().len(), 2);
    assert_eq!(backend.entries()[1].url, "/1/7");
}

#[test]
fn construction_failure_releases_resources_and_bad_codec_restore_reports_error() {
    let source = MutableValue::new(Vec::<Route<u8>>::new());
    let backend = Rc::new(MemoryHistoryBackend::new());
    assert!(matches!(
        HistoryController::attach(navigation(&source), backend.clone()),
        Err(HistoryError::EmptyHistory)
    ));
    assert_eq!(backend.listener_count(), 0);
    let source = MutableValue::new(vec![route(1)]);
    let binding = HistoryController::attach(navigation(&source), backend.clone()).unwrap();
    source.set(vec![route(1), route(2)]);
    backend.go(-1, Box::new(|r| r.unwrap()));
    backend
        .replace(HistoryEntry {
            state: Some(HistoryState {
                version: None,
                index: 0,
                nodes: vec![HistoryNode {
                    path: String::new(),
                    key: Vec::new(),
                    children: Vec::new(),
                }],
            }),
            url: "/broken".into(),
        })
        .unwrap();
    backend.go(0, Box::new(|r| r.unwrap()));
    assert!(
        matches!(binding.errors().get().as_ref(), Some(HistoryError::Codec(message)) if message == "empty")
    );
    assert_eq!(backend.entries()[0].url, "/1");
    source.set(vec![route(1), route(8)]);
    assert_eq!(backend.entries()[1].url, "/8");
}

#[test]
fn child_guard_runs_before_parent_and_denial_short_circuits() {
    let child = MutableValue::new(vec![route(2)]);
    let called = Rc::new(RefCell::new(Vec::new()));
    let calls = called.clone();
    let child_nav = Rc::new(
        TypedWebNavigation::new(child.value(), codec(), |_, done| done(Ok(())))
            .with_before_navigate(move || {
                calls.borrow_mut().push("child");
                false
            }),
    );
    let mut root = route(1);
    root.child = Some(child_nav);
    let source = MutableValue::new(vec![root]);
    let calls = called.clone();
    let nav = Rc::new(
        TypedWebNavigation::new(source.value(), codec(), |_, done| done(Ok(())))
            .with_before_navigate(move || {
                calls.borrow_mut().push("parent");
                true
            }),
    );
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(nav, backend.clone()).unwrap();
    child.set(vec![route(2), route(3)]);
    backend.go(-1, Box::new(|r| r.unwrap()));
    assert_eq!(*called.borrow(), ["child"]);
    assert_eq!(backend.position(), 1);
}

#[test]
fn destroy_during_deferred_host_move_prevents_late_writes() {
    let lifecycle = LifecycleRegistry::new();
    let source = MutableValue::new(vec![route(1)]);
    let backend = Rc::new(DeferredHost::new());
    let _binding = HistoryController::attach(navigation(&source), backend.clone())
        .unwrap()
        .with_lifecycle(&lifecycle);
    source.set(vec![route(1), route(2)]);
    source.set(vec![route(8), route(9)]);
    lifecycle.destroy().unwrap();
    backend.finish();
    assert_eq!(backend.memory.entries().len(), 2);
    assert_eq!(backend.memory.entries()[0].url, "/1");
    assert_eq!(backend.memory.listener_count(), 0);
}

#[test]
fn pages_adapter_replaces_selection_and_restores_whole_state() {
    use decompose_rs::router::pages::*;
    let context = decompose_rs::TestComponentContext::new();
    let nav = PagesNavigation::new();
    let pages = child_pages(
        &context,
        &nav,
        Pages::new(vec![1u8, 2], 0).unwrap(),
        PagesOptions::default(),
        |_, _| (),
    )
    .unwrap();
    let codec = RouteCodec::new(
        |state: &Pages<u8>| {
            let mut bytes = vec![state.selected_index.map_or(255, |i| i as u8)];
            bytes.extend(state.items.iter().map(|c| **c));
            Ok(bytes)
        },
        |bytes| {
            let (&selected, items) = bytes
                .split_first()
                .ok_or_else(|| HistoryError::Codec("empty pages key".into()))?;
            Ok(Pages {
                items: items.iter().copied().map(Rc::new).collect(),
                selected_index: (selected != 255).then_some(selected as usize),
            })
        },
    );
    let web = pages_web_navigation(nav.clone(), pages.clone(), codec, |state| {
        RouteInfo::path(format!("page/{}", state.selected_index.unwrap_or(255)))
    });
    let saved = web.history().unwrap()[0].key.clone();
    let backend = Rc::new(MemoryHistoryBackend::new());
    let web = Rc::new(web);
    let _binding = HistoryController::attach(web.clone(), backend.clone()).unwrap();
    nav.select_or_panic(1);
    assert_eq!(backend.entries().len(), 1);
    assert_eq!(backend.entries()[0].url, "/page/1");
    web.navigate(vec![saved], Box::new(|r| r.unwrap()));
    assert_eq!(pages.get().selected_index, Some(0));
    assert_eq!(pages.get().items.len(), 2);
    nav.clear_or_panic();
    assert_eq!(web.history().unwrap().len(), 1);
}

#[test]
fn panels_adapter_restores_membership_preserving_current_mode() {
    use decompose_rs::router::panels::*;
    let context = decompose_rs::TestComponentContext::new();
    let nav = PanelsNavigation::<u8, u8>::new();
    let panels = child_panels_without_extra(
        &context,
        &nav,
        Panels {
            main: Rc::new(1),
            details: Some(Rc::new(2)),
            extra: None,
            mode: ChildPanelsMode::Dual,
        },
        PanelsOptions::default(),
        |_, _| (),
        |_, _| (),
    )
    .unwrap();
    let codec = RouteCodec::new(
        |key: &PanelRouteKey<u8, u8>| {
            Ok(vec![*key.main, key.details.as_ref().map_or(255, |c| **c)])
        },
        |bytes| {
            if bytes.len() != 2 {
                return Err(HistoryError::Codec("panel key".into()));
            }
            Ok(PanelRouteKey {
                main: Rc::new(bytes[0]),
                details: (bytes[1] != 255).then(|| Rc::new(bytes[1])),
                extra: None,
            })
        },
    );
    let web = Rc::new(panels_web_navigation(
        nav.clone(),
        panels.clone(),
        codec,
        |state| RouteInfo::path(state.main.configuration.to_string()),
    ));
    let saved = web.history().unwrap()[0].key.clone();
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(web.clone(), backend.clone()).unwrap();
    nav.set_panels_or_panic(7, None, None);
    nav.set_mode_or_panic(ChildPanelsMode::Single);
    assert_eq!(backend.entries().len(), 1);
    web.navigate(vec![saved], Box::new(|r| r.unwrap()));
    assert_eq!(*panels.get().main.configuration, 1);
    assert_eq!(*panels.get().details.as_ref().unwrap().configuration, 2);
    assert_eq!(panels.get().mode, ChildPanelsMode::Single);
}

#[test]
fn startup_rewrites_existing_managed_state_and_history_disabled_stack_uses_active_only() {
    use decompose_rs::router::stack::*;
    let context = decompose_rs::TestComponentContext::new();
    let nav = StackNavigation::new();
    let stack = child_stack(
        &context,
        &nav,
        vec![1u8, 2],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    let web = Rc::new(stack_web_navigation(
        nav.clone(),
        stack.clone(),
        codec(),
        false,
        |child| RouteInfo::path(child.configuration.to_string()),
    ));
    assert_eq!(web.history().unwrap().len(), 1);
    let original = HistoryEntry {
        state: Some(HistoryState {
            version: None,
            index: 0,
            nodes: vec![HistoryNode {
                path: String::new(),
                key: vec![9],
                children: vec![],
            }],
        }),
        url: "/saved".into(),
    };
    let backend = Rc::new(MemoryHistoryBackend::with_entries(vec![original.clone()], 0).unwrap());
    let _binding = HistoryController::attach(web, backend.clone()).unwrap();
    assert_eq!(backend.entries()[0].url, "/2"); // current upstream rewrites without restoring
    assert_eq!(*stack.get().active.configuration, 2);
    nav.push_or_panic(3);
    assert_eq!(backend.entries().len(), 1);
    backend.replace(original).unwrap();
    backend.go(0, Box::new(|r| r.unwrap()));
    assert_eq!(*stack.get().active.configuration, 9);
    assert!(stack.get().back_stack.is_empty());
}

struct CyclicModel {
    value: MutableValue<bool>,
    weak: RefCell<Option<std::rc::Weak<dyn WebNavigation>>>,
    reads: Cell<usize>,
}
impl WebNavigation for CyclicModel {
    fn history(&self) -> Result<Vec<HistoryItem>, HistoryError> {
        self.reads.set(self.reads.get() + 1);
        assert!(self.reads.get() < 30, "cycle recursion was not detected");
        Ok(vec![HistoryItem {
            key: vec![1],
            path: "1".into(),
            parameters: vec![],
            child: if *self.value.get() {
                self.weak.borrow().as_ref().unwrap().upgrade()
            } else {
                None
            },
        }])
    }
    fn subscribe(&self, observer: Rc<dyn Fn()>) -> decompose_rs::value::Subscription {
        let initial = Cell::new(true);
        self.value.value().subscribe(move |_| {
            if !initial.replace(false) {
                observer()
            }
        })
    }
    fn navigate(&self, _: Vec<Vec<u8>>, done: HistoryCompletion) {
        done(Ok(()));
    }
}

#[test]
fn newly_cyclic_tree_reports_error_instead_of_recursing_on_host_back() {
    let model = Rc::new(CyclicModel {
        value: MutableValue::new(false),
        weak: RefCell::new(None),
        reads: Cell::new(0),
    });
    let navigation: Rc<dyn WebNavigation> = model.clone();
    *model.weak.borrow_mut() = Some(Rc::downgrade(&navigation));
    let backend = Rc::new(MemoryHistoryBackend::new());
    let binding = HistoryController::attach(navigation, backend.clone()).unwrap();
    model.value.set(true);
    assert_eq!(
        binding.errors().get().as_ref(),
        &Some(HistoryError::CyclicNavigation)
    );
    model.reads.set(0);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        backend.go(0, Box::new(|r| r.unwrap()))
    }));
    assert!(result.is_ok());
    assert_eq!(
        binding.errors().get().as_ref(),
        &Some(HistoryError::CyclicNavigation)
    );
}

#[test]
fn consecutive_host_moves_wait_for_deferred_restore_completions() {
    let source = MutableValue::new(vec![route(1)]);
    let pending = Rc::new(RefCell::new(std::collections::VecDeque::<(
        Vec<Rc<u8>>,
        HistoryCompletion,
    )>::new()));
    let out = pending.clone();
    let nav = Rc::new(TypedWebNavigation::new(
        source.value(),
        codec(),
        move |keys, done| out.borrow_mut().push_back((keys, done)),
    ));
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(nav, backend.clone()).unwrap();
    source.set(vec![route(1), route(2), route(3)]);
    backend.go(-1, Box::new(|r| r.unwrap()));
    backend.go(-1, Box::new(|r| r.unwrap()));
    assert_eq!(pending.borrow().len(), 1);
    let (keys, done) = pending.borrow_mut().pop_front().unwrap();
    source.set(keys.into_iter().map(|key| route(*key)).collect());
    done(Ok(()));
    assert_eq!(pending.borrow().len(), 1);
    assert_eq!(backend.entries()[0].url, "/1");
    let (keys, done) = pending.borrow_mut().pop_front().unwrap();
    source.set(keys.into_iter().map(|key| route(*key)).collect());
    done(Ok(()));
    assert_eq!(*source.get().last().unwrap().key, 1);
    assert_eq!(backend.position(), 0);
    assert_eq!(backend.entries()[1].url, "/2");
}

#[test]
fn malformed_managed_index_is_typed_error_and_does_not_wrap_host_movement() {
    let source = MutableValue::new(vec![route(1)]);
    let backend = Rc::new(MemoryHistoryBackend::new());
    let binding = HistoryController::attach(navigation(&source), backend.clone()).unwrap();
    backend
        .replace(HistoryEntry {
            state: Some(HistoryState {
                version: None,
                index: usize::MAX,
                nodes: vec![HistoryNode {
                    path: String::new(),
                    key: vec![1],
                    children: vec![],
                }],
            }),
            url: "/bad-index".into(),
        })
        .unwrap();
    backend.go(0, Box::new(|r| r.unwrap()));
    assert!(matches!(
        binding.errors().get().as_ref(),
        Some(HistoryError::Backend(_))
    ));
}

#[test]
fn denied_queued_back_returns_to_last_completed_restore() {
    let source = MutableValue::new(vec![route(1)]);
    let pending = Rc::new(RefCell::new(None::<(Vec<Rc<u8>>, HistoryCompletion)>));
    let out = pending.clone();
    let allowed = Rc::new(Cell::new(true));
    let guard = allowed.clone();
    let nav = Rc::new(
        TypedWebNavigation::new(source.value(), codec(), move |keys, done| {
            *out.borrow_mut() = Some((keys, done))
        })
        .with_before_navigate(move || guard.get()),
    );
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(nav, backend.clone()).unwrap();
    source.set(vec![route(1), route(2), route(3)]);
    backend.go(-1, Box::new(|r| r.unwrap()));
    backend.go(-1, Box::new(|r| r.unwrap()));
    let (keys, done) = pending.borrow_mut().take().unwrap();
    source.set(keys.into_iter().map(|key| route(*key)).collect());
    allowed.set(false);
    done(Ok(()));
    assert_eq!(backend.position(), 1);
    assert_eq!(*source.get().last().unwrap().key, 2);
}

#[test]
fn legacy_stack_initializes_all_paths_and_replaces_only_changed_suffix() {
    use decompose_rs::router::stack::*;
    let context = decompose_rs::TestComponentContext::new();
    let nav = StackNavigation::new();
    let stack = child_stack(
        &context,
        &nav,
        vec![1u8, 2, 3],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    let backend = Rc::new(MemoryHistoryBackend::new());
    let facade = StackHistoryController::new(
        backend.clone(),
        codec(),
        |c| format!("/{c}"),
        |path| {
            path.trim_matches('/')
                .parse::<u8>()
                .map_err(|e| HistoryError::Codec(e.to_string()))
        },
    );
    assert!(facade.history_paths().unwrap().is_empty());
    let _binding = facade
        .attach(nav.clone(), stack.clone(), |_, _| true)
        .unwrap();
    assert_eq!(backend.entries().len(), 3);
    assert_eq!(facade.history_paths().unwrap(), ["/1", "/2", "/3"]);
    nav.replace_all_or_panic(vec![1, 2, 4, 5]);
    assert_eq!(backend.entries().len(), 4);
    assert_eq!(backend.entries()[0].url, "/1");
    assert_eq!(backend.entries()[1].url, "/2");
    assert_eq!(facade.history_paths().unwrap(), ["/1", "/2", "/4", "/5"]);
    nav.replace_all_or_panic(vec![1, 8]);
    assert_eq!(backend.position(), 1);
    assert_eq!(backend.entries().len(), 2);
    assert_eq!(backend.entries()[1].url, "/8");
    backend.go(-1, Box::new(|r| r.unwrap()));
    assert_eq!(*stack.get().active.configuration, 1);
}

#[test]
fn legacy_stack_forward_uses_saved_path_mapper_and_target_guard() {
    use decompose_rs::router::stack::*;
    let context = decompose_rs::TestComponentContext::new();
    let nav = StackNavigation::new();
    let stack = child_stack(
        &context,
        &nav,
        vec![1u8],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    let backend = Rc::new(MemoryHistoryBackend::new());
    let parsed = Rc::new(Cell::new(0));
    let count = parsed.clone();
    let facade = StackHistoryController::new(
        backend.clone(),
        codec(),
        |c| format!("/{c}"),
        move |path| {
            count.set(count.get() + 1);
            path.trim_matches('/')
                .parse::<u8>()
                .map_err(|e| HistoryError::Codec(e.to_string()))
        },
    );
    let allowed = Rc::new(Cell::new(true));
    let allow = allowed.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = seen.clone();
    let _binding = facade
        .attach(nav.clone(), stack.clone(), move |new, old| {
            out.borrow_mut().push((
                new.iter().map(|v| **v).collect::<Vec<_>>(),
                old.iter().map(|v| **v).collect::<Vec<_>>(),
            ));
            allow.get()
        })
        .unwrap();
    nav.push_or_panic(2);
    backend.go(-1, Box::new(|r| r.unwrap()));
    assert_eq!(parsed.get(), 0);
    backend.go(1, Box::new(|r| r.unwrap()));
    assert_eq!(parsed.get(), 1);
    assert_eq!(*stack.get().active.configuration, 2);
    allowed.set(false);
    backend.go(-1, Box::new(|r| r.unwrap()));
    assert_eq!(backend.position(), 1);
    assert_eq!(*stack.get().active.configuration, 2);
    assert_eq!(seen.borrow().last().unwrap(), &(vec![1], vec![1, 2]));
}

#[test]
fn legacy_rewrite_first_keeps_forward_and_restore_active_match_ignores_lower_stack() {
    use decompose_rs::router::stack::*;
    let context = decompose_rs::TestComponentContext::new();
    let nav = StackNavigation::new();
    let stack = child_stack(
        &context,
        &nav,
        vec![1u8, 2, 3],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    let backend = Rc::new(MemoryHistoryBackend::new());
    let facade = StackHistoryController::new(
        backend.clone(),
        codec(),
        |c| format!("/{c}"),
        |path| {
            path.trim_matches('/')
                .parse::<u8>()
                .map_err(|e| HistoryError::Codec(e.to_string()))
        },
    );
    let binding = facade
        .attach(nav.clone(), stack.clone(), |_, _| true)
        .unwrap();
    nav.replace_all_or_panic(vec![8]);
    assert_eq!(backend.position(), 0);
    assert_eq!(backend.entries().len(), 3); // old controller intentionally retains forward entries
    assert_eq!(backend.entries()[0].url, "/8");
    backend.go(2, Box::new(|r| r.unwrap()));
    assert_eq!(
        stack
            .get()
            .items()
            .map(|c| *c.configuration)
            .collect::<Vec<_>>(),
        [8, 2, 3]
    );
    backend
        .replace(HistoryEntry {
            state: Some(HistoryState {
                version: None,
                index: 2,
                nodes: vec![HistoryNode {
                    key: vec![3],
                    path: "/3".into(),
                    children: vec![],
                }],
            }),
            url: "/3".into(),
        })
        .unwrap();
    backend.go(0, Box::new(|r| r.unwrap()));
    assert_eq!(stack.get().items().count(), 3);
    assert!(binding.errors().get().is_none());
    assert_eq!(facade.history_paths().unwrap(), ["/8", "/2", "/3"]);
}

#[test]
fn queued_denial_drains_later_back_and_returns_from_actual_host_position() {
    let source = MutableValue::new(vec![route(1)]);
    let pending = Rc::new(RefCell::new(None::<(Vec<Rc<u8>>, HistoryCompletion)>));
    let out = pending.clone();
    let guard_calls = Rc::new(Cell::new(0));
    let calls = guard_calls.clone();
    let navigation = Rc::new(
        TypedWebNavigation::new(source.value(), codec(), move |keys, done| {
            *out.borrow_mut() = Some((keys, done));
        })
        .with_before_navigate(move || {
            let count = calls.get() + 1;
            calls.set(count);
            count != 2
        }),
    );
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(navigation, backend.clone()).unwrap();
    source.set(vec![route(1), route(2), route(3), route(4)]);
    for _ in 0..3 {
        backend.go(-1, Box::new(|result| result.unwrap()));
    }
    let (keys, done) = pending.borrow_mut().take().unwrap();
    source.set(keys.into_iter().map(|key| route(*key)).collect());
    done(Ok(()));
    assert_eq!(guard_calls.get(), 3);
    assert_eq!(backend.position(), 2);
    let (keys, done) = pending.borrow_mut().take().unwrap();
    source.set(keys.into_iter().map(|key| route(*key)).collect());
    done(Ok(()));
    assert_eq!(*source.get().last().unwrap().key, 1);
    assert_eq!(backend.position(), 0);
}

#[test]
fn deferred_completion_codec_panic_does_not_suspend_later_model_changes() {
    let source = MutableValue::new(vec![route(1)]);
    let pending = Rc::new(RefCell::new(None::<(Vec<Rc<u8>>, HistoryCompletion)>));
    let out = pending.clone();
    let should_panic = Rc::new(Cell::new(false));
    let panic_flag = should_panic.clone();
    let codec = RouteCodec::new(
        move |key| {
            assert!(!panic_flag.get(), "codec panic after deferred completion");
            Ok(vec![*key])
        },
        |bytes| Ok(bytes[0]),
    );
    let navigation = Rc::new(TypedWebNavigation::new(
        source.value(),
        codec,
        move |keys, done| *out.borrow_mut() = Some((keys, done)),
    ));
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(navigation, backend.clone()).unwrap();
    source.set(vec![route(1), route(2)]);
    backend.go(-1, Box::new(|result| result.unwrap()));
    let (keys, done) = pending.borrow_mut().take().unwrap();
    source.set(keys.into_iter().map(|key| route(*key)).collect());
    should_panic.set(true);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| done(Ok(()))));
    assert!(result.is_err());
    should_panic.set(false);
    source.set(vec![route(1), route(3)]);
    assert_eq!(backend.position(), 1);
    assert_eq!(backend.current().unwrap().url, "/3");
}

#[test]
fn cancel_during_deferred_restore_prevents_new_child_restoration() {
    let child = MutableValue::new(vec![route(2)]);
    let mut parent = route(1);
    parent.child = Some(navigation(&child));
    let source = MutableValue::new(vec![parent]);
    let pending = Rc::new(RefCell::new(None::<HistoryCompletion>));
    let out = pending.clone();
    let navigation = Rc::new(TypedWebNavigation::new(
        source.value(),
        codec(),
        move |_, done| *out.borrow_mut() = Some(done),
    ));
    let backend = Rc::new(MemoryHistoryBackend::new());
    let binding = HistoryController::attach(navigation.clone(), backend.clone()).unwrap();
    child.set(vec![route(2), route(3)]);
    backend.go(-1, Box::new(|result| result.unwrap()));
    binding.cancel();
    let done = pending.borrow_mut().take().unwrap();
    done(Ok(()));
    assert_eq!(*child.get().last().unwrap().key, 3);
    assert_eq!(backend.listener_count(), 0);
}

#[test]
fn swapping_active_child_with_same_parent_key_replaces_current_entry() {
    let first = MutableValue::new(vec![route(2)]);
    let mut parent = route(1);
    parent.child = Some(navigation(&first));
    let source = MutableValue::new(vec![parent]);
    let backend = Rc::new(MemoryHistoryBackend::new());
    let _binding = HistoryController::attach(navigation(&source), backend.clone()).unwrap();
    let second = MutableValue::new(vec![route(2), route(3)]);
    let mut parent = route(1);
    parent.child = Some(navigation(&second));
    source.set(vec![parent]);
    assert_eq!(backend.entries().len(), 1);
    assert_eq!(backend.current().unwrap().url, "/1/3");
    first.set(vec![route(2), route(4)]);
    assert_eq!(backend.entries().len(), 1);
    second.set(vec![route(2), route(3), route(5)]);
    assert_eq!(backend.entries().len(), 2);
    assert_eq!(backend.current().unwrap().url, "/1/5");
}

struct PanicCancelNavigation {
    inner: Rc<dyn WebNavigation>,
    cancellations: Rc<Cell<usize>>,
}
impl WebNavigation for PanicCancelNavigation {
    fn history(&self) -> Result<Vec<HistoryItem>, HistoryError> {
        self.inner.history()
    }
    fn subscribe(&self, observer: Rc<dyn Fn()>) -> decompose_rs::value::Subscription {
        let token = self.inner.subscribe(observer);
        let count = self.cancellations.clone();
        decompose_rs::value::Subscription::from_cancel(move || {
            token.cancel();
            count.set(count.get() + 1);
            panic!("custom cancellation");
        })
    }
    fn navigate(&self, keys: Vec<Vec<u8>>, done: HistoryCompletion) {
        self.inner.navigate(keys, done)
    }
}
#[test]
fn multiple_cancellation_panics_finish_cleanup_without_aborting_original_unwind() {
    const MODE: &str = "DECOMPOSE_HISTORY_CANCELLATION_TEST";
    if std::env::var_os(MODE).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "multiple_cancellation_panics_finish_cleanup_without_aborting_original_unwind",
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
    let count = Rc::new(Cell::new(0));
    let child_source = MutableValue::new(vec![route(2)]);
    let child = Rc::new(PanicCancelNavigation {
        inner: navigation(&child_source),
        cancellations: count.clone(),
    });
    let mut parent = route(1);
    parent.child = Some(child);
    let source = MutableValue::new(vec![parent]);
    let root = Rc::new(PanicCancelNavigation {
        inner: navigation(&source),
        cancellations: count.clone(),
    });
    let backend = Rc::new(MemoryHistoryBackend::new());
    let binding = HistoryController::attach(root, backend.clone()).unwrap();
    let error =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| binding.cancel())).unwrap_err();
    assert_eq!(error.downcast_ref::<&str>(), Some(&"custom cancellation"));
    assert_eq!(count.get(), 2);
    assert_eq!(backend.listener_count(), 0);
    binding.cancel();
}
