use decompose_rs::{ComponentContext, LifecycleState, TestComponentContext};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = TestComponentContext::new();
    let child = context.child_context("editor")?;
    child
        .state_store()
        .register("draft", || b"saved draft".to_vec())?;
    let retained = context.instance_store().put("service", 42u8)?;
    context.lifecycle().pause()?;
    let recreated = context.recreate(true)?;
    assert_eq!(context.lifecycle().state(), LifecycleState::Destroyed);
    assert_eq!(recreated.lifecycle().state(), LifecycleState::Started);
    let service = recreated.instance_store().get::<u8>("service")?.unwrap();
    assert!(std::rc::Rc::ptr_eq(&retained, &service));
    let child = recreated.child_context("editor")?;
    assert_eq!(
        child.state_store().consume_bytes("draft").unwrap(),
        Some(b"saved draft".to_vec())
    );
    let fresh = recreated.recreate(false)?;
    assert!(fresh.instance_store().is_empty().unwrap());
    println!("Configuration recreation retained services; process recreation reset them.");
    Ok(())
}
