use decompose_rs::router::{
    slot::{SlotCodec, SlotNavigation, SlotOptions, child_slot},
    stack::{StackNavigation, StackOptions, child_stack},
};
use decompose_rs::{
    ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState, StateKeeper,
    StateStore,
};
use std::rc::Rc;

// Neither routes nor dialogs need Clone.
#[derive(Debug, PartialEq, Eq)]
enum Screen {
    Home,
    Details(u32),
}
#[derive(Debug, PartialEq, Eq)]
struct Dialog {
    title: String,
}

fn resumed() -> LifecycleRegistry {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    lifecycle
}
fn dialog_codec() -> SlotCodec<Dialog> {
    SlotCodec::new(
        |dialog: &Dialog| Ok(dialog.title.as_bytes().to_vec()),
        |bytes| {
            String::from_utf8(bytes.to_vec())
                .map(|title| Dialog { title })
                .map_err(|error| error.to_string())
        },
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = DefaultComponentContext::new(resumed());
    let screens = StackNavigation::new();
    let stack = child_stack(
        &root,
        &screens,
        vec![Screen::Home],
        StackOptions {
            key: "screens".into(),
            handle_back_button: true,
            ..StackOptions::default()
        },
        |_, context| context,
    )?;
    let dialogs = SlotNavigation::new();
    let slot = child_slot(
        &root,
        &dialogs,
        None,
        SlotOptions {
            key: "dialog".into(),
            handle_back_button: true,
            codec: Some(dialog_codec()),
        },
        |_, context| context,
    )?;
    let _subscription = slot.subscribe(|snapshot| {
        println!(
            "dialog: {:?}",
            snapshot
                .child
                .as_ref()
                .map(|child| &child.configuration.title)
        );
    });
    screens.push_or_panic(Screen::Details(7));
    dialogs.activate_or_panic(Dialog {
        title: "Unsaved changes".into(),
    });
    let previous = slot.get();
    dialogs.activate_or_panic(Dialog {
        title: "Unsaved changes".into(),
    });
    assert!(Rc::ptr_eq(
        &previous.child.as_ref().unwrap().instance,
        &slot.get().child.as_ref().unwrap().instance
    ));

    // With no child Back handlers, the later slot callback closes the dialog first.
    assert!(root.back_handler().back());
    assert!(slot.get().child.is_none());
    assert_eq!(stack.get().items().count(), 2);
    assert_eq!(
        previous
            .child
            .as_ref()
            .unwrap()
            .instance
            .lifecycle()
            .state(),
        LifecycleState::Destroyed
    );
    assert!(root.back_handler().back());
    assert_eq!(stack.get().items().count(), 1);

    // A persistent empty slot overrides a nonempty initial configuration on restore.
    let saved = root.state_store().save()?;
    assert!(saved.contains_key("dialog"));
    root.lifecycle().destroy()?;
    let restored = DefaultComponentContext::with_state_store(
        resumed(),
        StateStore::from(StateKeeper::with_restored(saved)),
    );
    let restored_slot = child_slot(
        &restored,
        &SlotNavigation::new(),
        Some(Dialog {
            title: "Must not reappear".into(),
        }),
        SlotOptions {
            key: "dialog".into(),
            codec: Some(dialog_codec()),
            ..SlotOptions::default()
        },
        |_, context| context,
    )?;
    assert!(restored_slot.get().child.is_none());
    println!("restored dialog: empty");
    restored.lifecycle().destroy()?;
    Ok(())
}
