//! Run with `cargo run --example selective_persistence`.
use decompose_rs::{
    ComponentContext, DefaultComponentContext, InstanceKeeper, LifecycleRegistry, LifecycleState,
    SharedBack, StateKeeper,
    router::stack::{StackCodec, StackNavigation, StackOptions, child_stack_with_initial},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn main() {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    let context = DefaultComponentContext::new(lifecycle);
    let include_navigation = Rc::new(Cell::new(false));
    let include = include_navigation.clone();
    let codec = StackCodec::new(
        |routes: &[Rc<u8>]| Ok(routes.iter().map(|route| **route).collect()),
        |bytes| Ok(bytes.to_vec()),
    )
    .should_save(move |_| include.get())
    .should_restore(|routes| routes.iter().all(|route| **route < 10));
    child_stack_with_initial(
        &context,
        &StackNavigation::new(),
        || vec![1],
        StackOptions {
            codec: Some(codec.clone()),
            ..Default::default()
        },
        |_, context| context,
    )
    .unwrap();
    assert!(context.state_keeper().borrow().save().unwrap().is_empty());
    include_navigation.set(true);
    let saved = context.state_keeper().borrow().save().unwrap();
    assert!(saved.contains_key("DefaultChildStack"));

    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    let restored = DefaultComponentContext::with_all(
        lifecycle,
        Rc::new(RefCell::new(StateKeeper::with_restored(saved))),
        Rc::new(RefCell::new(InstanceKeeper::new())),
        SharedBack::new(),
    );
    context.lifecycle().destroy().unwrap();
    let stack = child_stack_with_initial(
        &restored,
        &StackNavigation::new(),
        || panic!("accepted state uses no initializer"),
        StackOptions {
            codec: Some(codec),
            ..Default::default()
        },
        |_, context| context,
    )
    .unwrap();
    assert_eq!(*stack.get().active.configuration, 1);
    println!("Skipped one save, then restored a selectively accepted stack.");
}
