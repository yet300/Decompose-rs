use decompose_rs::router::stack::StackNavigator;
use decompose_rs::{ComponentContext, LifecycleState, TestComponentContext};
use std::{cell::Cell, rc::Rc};

#[test]
fn public_context_controls_lifecycle_nested_state_and_back() {
    let context = TestComponentContext::new();
    assert_eq!(context.lifecycle().state(), LifecycleState::Resumed);
    let child = context.child_context("child").unwrap();
    child.state_store().register("answer", || vec![42]).unwrap();
    let calls = Rc::new(Cell::new(0));
    let output = calls.clone();
    child
        .back_handler()
        .register(0, true, move |_, _| output.set(output.get() + 1))
        .unwrap();
    assert!(context.back_handler().back());
    assert_eq!(calls.get(), 1);
    context.lifecycle().pause().unwrap();
    assert_eq!(child.lifecycle().state(), LifecycleState::Started);
    let recreated = context.recreate(false).unwrap();
    assert_eq!(context.lifecycle().state(), LifecycleState::Destroyed);
    assert_eq!(recreated.lifecycle().state(), LifecycleState::Started);
    let child = recreated.child_context("child").unwrap();
    assert_eq!(
        child.state_store().consume_bytes("answer").unwrap(),
        Some(vec![42])
    );
    assert!(!recreated.back_handler().can_handle());
}

#[test]
fn configuration_change_retains_instances_and_process_death_destroys_them() {
    let context = TestComponentContext::new();
    let retained = context.instance_store().put("answer", 42u8).unwrap();
    let old = context.instance_store();
    let recreated = context.recreate(true).unwrap();
    assert!(old.ptr_eq(&recreated.instance_store()));
    assert!(!old.is_destroyed().unwrap());
    assert!(Rc::ptr_eq(
        &retained,
        &old.get::<u8>("answer").unwrap().unwrap()
    ));
    let fresh = recreated.recreate(false).unwrap();
    assert!(old.is_destroyed().unwrap());
    assert!(!old.ptr_eq(&fresh.instance_store()));
    assert_eq!(fresh.instance_store().len().unwrap(), 0);
}

#[test]
fn initialized_recreation_preserves_state_and_destroyed_recreation_is_rejected() {
    let context = TestComponentContext::initialized();
    let recreated = context.recreate(true).unwrap();
    assert_eq!(recreated.lifecycle().state(), LifecycleState::Initialized);
    assert_eq!(context.lifecycle().state(), LifecycleState::Destroyed);
    assert!(context.recreate(true).is_err());
}

#[test]
fn save_failure_leaves_lifecycle_and_retained_scope_available_for_retry() {
    let context = TestComponentContext::new();
    let fail = Rc::new(Cell::new(true));
    let switch = fail.clone();
    context
        .state_store()
        .register_fallible("state", move || {
            if switch.get() {
                Err("save failed".into())
            } else {
                Ok(vec![1])
            }
        })
        .unwrap();
    context.instance_store().put("instance", 42u8).unwrap();
    assert!(context.recreate(false).is_err());
    assert_eq!(context.lifecycle().state(), LifecycleState::Resumed);
    assert!(!context.instance_store().is_destroyed().unwrap());
    assert!(context.instance_store().contains("instance").unwrap());
    fail.set(false);
    let recreated = context.recreate(false).unwrap();
    assert_eq!(
        recreated.state_store().consume_bytes("state").unwrap(),
        Some(vec![1])
    );
}

#[test]
fn memory_navigation_survives_configuration_change_and_durable_save_omits_it() {
    use decompose_rs::router::stack::{StackNavigation, StackOptions, child_stack};
    let context = TestComponentContext::new();
    context
        .state_store()
        .enable_memory_navigation("DefaultChildStack")
        .unwrap();
    let source = StackNavigation::new();
    let first = child_stack(
        &context,
        &source,
        vec![1u8],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    source.push_or_panic(2);
    let keys: Vec<_> = first.get().items().map(|child| child.key.clone()).collect();
    assert!(context.save_bytes().unwrap().is_empty());
    let recreated = context.recreate(true).unwrap();
    let restored = child_stack(
        &recreated,
        &source,
        vec![9],
        StackOptions::default(),
        |_, _| (),
    )
    .unwrap();
    assert_eq!(*restored.get().active.configuration, 2);
    assert_eq!(
        restored
            .get()
            .items()
            .map(|child| child.key.clone())
            .collect::<Vec<_>>(),
        keys
    );
    let fresh = recreated.recreate(false).unwrap();
    let reset = child_stack(&fresh, &source, vec![9], StackOptions::default(), |_, _| ()).unwrap();
    assert_eq!(*reset.get().active.configuration, 9);
}
