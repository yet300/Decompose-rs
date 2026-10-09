use decompose_rs::router::stack::{StackCodec, StackNavigation, StackOptions, child_stack};
use decompose_rs::{
    ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState, StateKeeper,
};
use std::{cell::RefCell, rc::Rc};

// Routes and components do not need Clone.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    Home,
    Details(u32),
    Settings,
}

fn resumed() -> LifecycleRegistry {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    lifecycle
}
fn codec() -> StackCodec<Route> {
    StackCodec::new(
        |routes: &[Rc<Route>]| {
            let mut bytes = Vec::new();
            for route in routes {
                let (tag, id) = match route.as_ref() {
                    Route::Home => (0, 0u32),
                    Route::Details(id) => (1, *id),
                    Route::Settings => (2, 0),
                };
                bytes.push(tag);
                bytes.extend_from_slice(&id.to_le_bytes());
            }
            Ok(bytes)
        },
        |bytes| {
            if bytes.len() % 5 != 0 {
                return Err("invalid route payload length".into());
            }
            bytes
                .chunks_exact(5)
                .map(|record| {
                    let id = u32::from_le_bytes(record[1..].try_into().unwrap());
                    match record[0] {
                        0 if id == 0 => Ok(Route::Home),
                        1 => Ok(Route::Details(id)),
                        2 if id == 0 => Ok(Route::Settings),
                        _ => Err("invalid route tag".into()),
                    }
                })
                .collect()
        },
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = DefaultComponentContext::new(resumed());
    let navigation = StackNavigation::new();
    let options = StackOptions {
        handle_back_button: true,
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let stack = child_stack(
        &root,
        &navigation,
        vec![Route::Home],
        options,
        |_, context| context,
    )?;
    let _subscription = stack.subscribe(|state| {
        println!(
            "active: {:?}, depth: {}",
            state.active.configuration,
            state.items().count()
        );
    });
    navigation.push_new(Route::Details(7));
    navigation.push_new(Route::Details(7)); // Same active configuration: no-op.
    root.back_handler().back();
    navigation.push(Route::Settings);
    let saved = root.state_keeper().borrow().save()?;
    root.lifecycle().destroy()?;

    let restored = DefaultComponentContext::with_state_keeper(
        resumed(),
        Rc::new(RefCell::new(StateKeeper::with_restored(saved))),
    );
    let options = StackOptions {
        codec: Some(codec()),
        ..StackOptions::default()
    };
    let restored_stack = child_stack(
        &restored,
        &StackNavigation::new(),
        vec![Route::Home],
        options,
        |_, context| context,
    )?;
    println!(
        "restored: {:?}, depth: {}",
        restored_stack.get().active.configuration,
        restored_stack.get().items().count()
    );
    restored.lifecycle().destroy()?;
    Ok(())
}
