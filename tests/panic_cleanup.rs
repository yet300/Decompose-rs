use decompose_rs::router::stack::*;
use decompose_rs::*;
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

struct PanickingPayload;
impl Drop for PanickingPayload {
    fn drop(&mut self) {
        std::panic::panic_any(Self);
    }
}

fn root() -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::new(lifecycle)
}

#[test]
fn rollback_double_panic_is_catchable_in_subprocess() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "rollback_subprocess_case", "--nocapture"])
        .env("DECOMPOSE_ROLLBACK_SUBPROCESS", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "subprocess failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn rollback_subprocess_case() {
    if std::env::var_os("DECOMPOSE_ROLLBACK_SUBPROCESS").is_none() {
        return;
    }
    let parent = root();
    let before = parent.lifecycle().subscriber_count();
    let stores = Rc::new(RefCell::new(Vec::<InstanceStore>::new()));
    let out = stores.clone();
    let observations = Rc::new(RefCell::new(Vec::new()));
    let observed = observations.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = child_stack(
            &parent,
            &StackNavigation::new(),
            vec![1u32, 2],
            StackOptions::default(),
            move |config, ctx| {
                out.borrow_mut().push(ctx.instance_store());
                retain_probe(&ctx.instance_store(), &observed);
                // Dropping the secondary panic's payload itself panics again.
                let _ = ctx
                    .lifecycle()
                    .do_on_destroy(|| std::panic::panic_any(PanickingPayload))
                    .detach();
                if *config == 2 {
                    panic!("factory panic");
                }
                ctx
            },
        );
    }));
    let payload = result.expect_err("factory panic must propagate");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"factory panic"));
    assert!(
        stores
            .borrow()
            .iter()
            .all(|store| store.is_destroyed().unwrap())
    );
    assert_eq!(parent.lifecycle().subscriber_count(), before);
    assert_eq!(parent.back_handler().handler_count(), 0);
    assert!(
        !parent
            .state_store()
            .is_registered("DefaultChildStack")
            .unwrap()
    );
    assert!(parent.instance_store().is_empty().unwrap());
    assert!(matches!(
        observations.borrow().as_slice(),
        [Ok(true), Ok(true)]
    ));
    child_stack(
        &parent,
        &StackNavigation::new(),
        vec![1u32, 2],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
}

#[test]
fn child_deactivation_releases_payload_after_marking_store_destroyed() {
    use decompose_rs::router::items::*;
    let parent = root();
    let navigation = ItemsNavigation::new();
    let observations = Rc::new(RefCell::new(Vec::new()));
    let out = observations.clone();
    let mut initial = Items::new(vec![1u32]).unwrap();
    initial
        .active_items
        .insert(Rc::new(1), ActiveLifecycleState::Created);
    let children = child_items(
        &parent,
        &navigation,
        initial,
        ItemsOptions::default(),
        move |_, ctx| {
            retain_probe(&ctx.instance_store(), &out);
            ctx
        },
    )
    .unwrap();
    let old = children.get_or_create(&1).unwrap();
    navigation.set_active_items_or_panic(|mut active| {
        active.clear();
        active
    });
    assert!(old.instance_store().is_destroyed().unwrap());
    assert!(matches!(observations.borrow().as_slice(), [Ok(true)]));
    assert!(children.get().active_items.is_empty());
}

#[test]
fn retained_child_scope_release_allows_reentrant_read_and_keeps_external_ownership() {
    let keeper = InstanceStore::default();
    let parent = DefaultComponentContext::with_instance_store(root().lifecycle(), keeper.clone());
    let child = parent.child_context("fixed").unwrap();
    let observations = Rc::new(RefCell::new(Vec::new()));
    retain_probe(&child.instance_store(), &observations);
    parent.lifecycle().destroy().unwrap();
    assert!(observations.borrow().is_empty());
    assert!(!keeper.is_destroyed().unwrap());
    keeper.destroy_all().unwrap();
    assert!(matches!(observations.borrow().as_slice(), [Ok(true)]));
    assert!(child.instance_store().is_destroyed().unwrap());
}

struct PanickingProbe {
    store: InstanceStore,
    drops: Rc<RefCell<Vec<&'static str>>>,
    name: &'static str,
}
impl Drop for PanickingProbe {
    fn drop(&mut self) {
        assert!(self.store.is_destroyed().unwrap());
        self.drops.borrow_mut().push(self.name);
        panic!("{}", self.name);
    }
}

#[test]
fn multiple_retained_destructor_panics_are_catchable_in_subprocess() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "retained_panics_subprocess_case", "--nocapture"])
        .env("DECOMPOSE_RETAINED_SUBPROCESS", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "subprocess failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn retained_panics_subprocess_case() {
    if std::env::var_os("DECOMPOSE_RETAINED_SUBPROCESS").is_none() {
        return;
    }
    let parent = root();
    let store = parent.instance_store();
    let drops = Rc::new(RefCell::new(Vec::new()));
    for name in ["a", "b", "c"] {
        store
            .put(
                name,
                PanickingProbe {
                    store: store.clone(),
                    drops: drops.clone(),
                    name,
                },
            )
            .unwrap();
    }
    let result = catch_unwind(AssertUnwindSafe(|| parent.lifecycle().destroy().unwrap()));
    let payload = result.expect_err("first destructor panic must propagate");
    assert_eq!(payload.downcast_ref::<String>().unwrap(), "a");
    assert_eq!(&*drops.borrow(), &["a", "b", "c"]);
    assert!(store.is_destroyed().unwrap());
    assert!(store.is_empty().unwrap());
    store.destroy_all().unwrap();
    assert_eq!(drops.borrow().len(), 3);
}

#[test]
fn multiple_outgoing_destroy_panics_finish_cleanup_and_preserve_survivor_identity() {
    let parent = root();
    let navigation = StackNavigation::new();
    let destroyed = Rc::new(RefCell::new(Vec::new()));
    let out = destroyed.clone();
    let stack = child_stack(
        &parent,
        &navigation,
        vec![1u32, 2, 3],
        StackOptions::default(),
        move |config, ctx| {
            if *config > 1 {
                let config = *config;
                let out = out.clone();
                let _ = ctx
                    .lifecycle()
                    .do_on_destroy(move || {
                        out.borrow_mut().push(config);
                        panic!("destroy {config}");
                    })
                    .detach();
            }
            ctx
        },
    )
    .unwrap();
    let before = stack.get();
    let survivor = before.back_stack[0].instance.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        navigation.replace_all_or_panic(vec![1])
    }));
    assert_eq!(
        result
            .expect_err("first outgoing panic must propagate")
            .downcast_ref::<String>()
            .unwrap(),
        "destroy 3"
    );
    assert_eq!(&*destroyed.borrow(), &[3, 2]);
    assert!(
        before.back_stack[1]
            .instance
            .instance_store()
            .is_destroyed()
            .unwrap()
    );
    assert!(
        before
            .active
            .instance
            .instance_store()
            .is_destroyed()
            .unwrap()
    );
    assert!(!survivor.instance_store().is_destroyed().unwrap());
    navigation.replace_all_or_panic(vec![1]);
    assert!(Rc::ptr_eq(&survivor, &stack.get().active.instance));
}

#[test]
fn root_owned_retention_ends_when_router_destroy_callback_panics() {
    let parent = root();
    let observations = Rc::new(RefCell::new(Vec::new()));
    retain_probe(&parent.instance_store(), &observations);
    let out = observations.clone();
    let stack = child_stack(
        &parent,
        &StackNavigation::new(),
        vec![1u32, 2],
        StackOptions::default(),
        move |_, ctx| {
            retain_probe(&ctx.instance_store(), &out);
            let _ = ctx
                .lifecycle()
                .do_on_destroy(|| panic!("destroy callback"))
                .detach();
            ctx
        },
    )
    .unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| parent.lifecycle().destroy().unwrap()));
    assert!(result.is_err());
    assert!(parent.instance_store().is_destroyed().unwrap());
    assert!(
        stack
            .get()
            .items()
            .all(|child| child.instance.instance_store().is_destroyed().unwrap())
    );
    assert_eq!(observations.borrow().len(), 3);
    assert!(
        observations
            .borrow()
            .iter()
            .all(|entry| matches!(entry, Ok(true)))
    );
    assert_eq!(parent.back_handler().handler_count(), 0);
    assert_eq!(parent.state_store().provider_count().unwrap(), 0);
}

#[test]
fn rollback_pause_panic_still_finishes_destroy_and_releases_scope() {
    let parent = root();
    let callbacks = Rc::new(RefCell::new(Vec::new()));
    let out = callbacks.clone();
    let created = Rc::new(RefCell::new(None::<DefaultComponentContext>));
    let captured = created.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let _ = child_stack(
            &parent,
            &StackNavigation::new(),
            vec![1u32],
            StackOptions::default(),
            move |_, ctx| {
                *captured.borrow_mut() = Some(ctx.clone());
                let paused = out.clone();
                let _ = ctx
                    .lifecycle()
                    .do_on_pause(move || {
                        paused.borrow_mut().push("pause");
                        panic!("pause cleanup");
                    })
                    .detach();
                let destroyed = out.clone();
                let _ = ctx
                    .lifecycle()
                    .do_on_destroy(move || destroyed.borrow_mut().push("destroy"))
                    .detach();
                let _ = ctx
                    .lifecycle()
                    .do_on_resume(|| panic!("resume factory"))
                    .detach();
                ctx
            },
        );
    }));
    let payload = result.expect_err("initial resume panic must propagate");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"resume factory"));
    let old = created.borrow();
    let old = old.as_ref().unwrap();
    assert_eq!(old.lifecycle().state(), LifecycleState::Destroyed);
    assert!(old.instance_store().is_destroyed().unwrap());
    assert_eq!(&*callbacks.borrow(), &["pause", "destroy"]);
}

fn codec() -> StackCodec<u32> {
    StackCodec::new(
        |configs: &[Rc<u32>]| Ok(configs.iter().flat_map(|c| c.to_le_bytes()).collect()),
        |bytes: &[u8]| {
            Ok(bytes
                .chunks_exact(4)
                .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
                .collect())
        },
    )
}
#[test]
fn rollback_preserves_restored_input_and_preexisting_retained_identity() {
    let keeper = InstanceStore::default();
    let parent = DefaultComponentContext::with_instance_store(root().lifecycle(), keeper.clone());
    let options = || StackOptions {
        key: "retry".into(),
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let stack = child_stack(
        &parent,
        &StackNavigation::new(),
        vec![1u32, 2],
        options(),
        |_, ctx| {
            ctx.instance_store()
                .get_or_create("identity", || 42u32)
                .unwrap();
            ctx
        },
    )
    .unwrap();
    let identities = stack
        .get()
        .items()
        .map(|child| {
            child
                .instance
                .instance_store()
                .get::<u32>("identity")
                .unwrap()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let saved = parent.state_store().save().unwrap();
    parent.lifecycle().destroy().unwrap();
    let restored = DefaultComponentContext::with_stores(
        root().lifecycle(),
        StateStore::from(StateKeeper::with_restored(saved.clone())),
        keeper.clone(),
        SharedBack::new(),
    );
    let before = restored.lifecycle().subscriber_count();
    let attempt_stores = Rc::new(RefCell::new(Vec::<InstanceStore>::new()));
    let out = attempt_stores.clone();
    let failed = catch_unwind(AssertUnwindSafe(|| {
        let _ = child_stack(
            &restored,
            &StackNavigation::new(),
            vec![99u32],
            options(),
            move |config, ctx| {
                out.borrow_mut().push(ctx.instance_store());
                if *config == 2 {
                    panic!("retry factory");
                }
                let _ = ctx
                    .lifecycle()
                    .do_on_destroy(|| panic!("rollback destroy"))
                    .detach();
                ctx
            },
        );
    }));
    assert!(failed.is_err());
    assert!(
        attempt_stores
            .borrow()
            .iter()
            .all(|store| !store.is_destroyed().unwrap())
    );
    assert_eq!(restored.lifecycle().subscriber_count(), before);
    assert_eq!(restored.back_handler().handler_count(), 0);
    assert!(!restored.state_store().is_registered("retry").unwrap());
    assert_eq!(restored.state_store().save().unwrap(), saved);
    let retried = child_stack(
        &restored,
        &StackNavigation::new(),
        vec![99u32],
        options(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(
        retried
            .get()
            .items()
            .map(|child| *child.configuration)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    for (child, identity) in retried.get().items().zip(identities) {
        assert!(Rc::ptr_eq(
            &child
                .instance
                .instance_store()
                .get::<u32>("identity")
                .unwrap()
                .unwrap(),
            &identity
        ));
    }
}

struct Probe {
    store: InstanceStore,
    observations: Rc<RefCell<Vec<Result<bool, InstanceAccessError>>>>,
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.observations
            .borrow_mut()
            .push(self.store.is_destroyed());
    }
}
fn retain_probe(
    store: &InstanceStore,
    observations: &Rc<RefCell<Vec<Result<bool, InstanceAccessError>>>>,
) {
    store
        .put(
            "probe",
            Probe {
                store: store.clone(),
                observations: observations.clone(),
            },
        )
        .unwrap();
}

#[test]
fn root_release_allows_destructor_to_read_destroyed_store() {
    let parent = root();
    let observations = Rc::new(RefCell::new(Vec::new()));
    retain_probe(&parent.instance_store(), &observations);
    parent.lifecycle().destroy().unwrap();
    assert!(matches!(observations.borrow().as_slice(), [Ok(true)]));
}

#[test]
fn child_removal_allows_destructor_to_read_destroyed_store() {
    let parent = root();
    let navigation = StackNavigation::new();
    let observations = Rc::new(RefCell::new(Vec::new()));
    let out = observations.clone();
    let stack = child_stack(
        &parent,
        &navigation,
        vec![1u32],
        StackOptions::default(),
        move |_, ctx| {
            retain_probe(&ctx.instance_store(), &out);
            ctx
        },
    )
    .unwrap();
    navigation.replace_current_or_panic(2);
    assert!(matches!(observations.borrow().as_slice(), [Ok(true)]));
    assert!(
        !stack
            .get()
            .active
            .instance
            .instance_store()
            .is_destroyed()
            .unwrap()
    );
}
