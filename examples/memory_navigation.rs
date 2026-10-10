use decompose_rs::router::stack::{
    StackNavigation, StackOptions, child_stack, child_stack_with_initial,
};
use decompose_rs::{ComponentContext, TestComponentContext};
use std::rc::Rc;

#[derive(PartialEq, Eq)]
struct Screen(u8); // no Clone and no byte codec

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = TestComponentContext::new();
    context
        .state_keeper()
        .borrow_mut()
        .enable_memory_navigation("DefaultChildStack")?;
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &context,
        &navigation,
        vec![Screen(1)],
        StackOptions::default(),
        |_, child| {
            child
                .state_keeper()
                .borrow_mut()
                .register("draft", || b"unsent".to_vec())
                .unwrap();
            child
        },
    )?;
    navigation.push(Screen(2));
    let original = stack.get().active.clone();
    assert!(context.state_keeper().borrow().save()?.is_empty()); // no navigation or nested draft reaches disk
    let recreated = context.recreate(true)?;
    let restored = child_stack_with_initial(
        &recreated,
        &StackNavigation::<Screen>::new(),
        || panic!("memory restoration skips initial state"),
        StackOptions::default(),
        |_, child| child,
    )?;
    assert!(Rc::ptr_eq(
        &original.configuration,
        &restored.get().active.configuration
    ));
    assert_eq!(original.key, restored.get().active.key);
    assert_eq!(
        restored
            .get()
            .active
            .instance
            .state_keeper()
            .borrow_mut()
            .consume_bytes("draft"),
        Some(b"unsent".to_vec())
    );
    let process = recreated.recreate(false)?;
    let reset = child_stack(
        &process,
        &StackNavigation::new(),
        vec![Screen(9)],
        StackOptions::default(),
        |_, _| (),
    )?;
    assert_eq!(reset.get().active.configuration.0, 9);
    println!(
        "Memory recreation kept Screen(2), its stable key and draft; process recreation reset to Screen(9)."
    );
    Ok(())
}
