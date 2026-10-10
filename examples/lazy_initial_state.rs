#![allow(deprecated)] // Legacy context interoperability; see rust_api.rs for opaque stores.
use decompose_rs::router::stack::{
    StackCodec, StackNavigation, StackOptions, child_stack_with_initial,
};
use decompose_rs::{
    ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState, StateKeeper,
    StateStore,
};
use std::rc::Rc;

// Neither the route nor the component implements Clone.
#[derive(Debug, PartialEq, Eq)]
struct Route {
    id: u32,
}

struct Component {
    context: DefaultComponentContext,
    visits: u32,
}

fn resumed() -> LifecycleRegistry {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    lifecycle
}

fn options() -> StackOptions<Route> {
    StackOptions {
        codec: Some(StackCodec::new(
            |routes: &[Rc<Route>]| {
                Ok(routes
                    .iter()
                    .flat_map(|route| route.id.to_le_bytes())
                    .collect())
            },
            |bytes| {
                if bytes.len() % 4 != 0 {
                    return Err("invalid route payload length".into());
                }
                Ok(bytes
                    .chunks_exact(4)
                    .map(|record| Route {
                        id: u32::from_le_bytes(record.try_into().unwrap()),
                    })
                    .collect())
            },
        )),
        ..StackOptions::default()
    }
}

fn component(_: &Route, context: DefaultComponentContext) -> Component {
    let visits = context
        .state_store()
        .consume_bytes("visits")
        .unwrap()
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .unwrap_or(1);
    context
        .state_store()
        .register("visits", move || (visits + 1).to_le_bytes().to_vec())
        .unwrap();
    Component { context, visits }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = DefaultComponentContext::new(resumed());
    let navigation = StackNavigation::new();
    let owned_route = Route { id: 7 };
    let mut initial_calls = 0;

    // FnOnce consumes the route and borrows a local mutable counter.
    let stack = child_stack_with_initial(
        &root,
        &navigation,
        || {
            initial_calls += 1;
            vec![owned_route]
        },
        options(),
        component,
    )?;
    assert_eq!(initial_calls, 1);
    assert_eq!(stack.get().active.configuration.id, 7);
    navigation.push_or_panic(Route { id: 9 });
    let before = stack.get();
    let saved = root.state_store().save()?;
    root.lifecycle().destroy()?;
    assert_eq!(before.active.instance.visits, 1);
    assert!(
        before
            .active
            .instance
            .context
            .lifecycle()
            .state()
            .is_destroyed()
    );

    let restored_root = DefaultComponentContext::with_state_store(
        resumed(),
        StateStore::from(StateKeeper::with_restored(saved)),
    );
    let restored = child_stack_with_initial(
        &restored_root,
        &StackNavigation::new(),
        || -> Vec<Route> { panic!("restoration must skip the initializer") },
        options(),
        component,
    )?;
    let after = restored.get();
    assert_eq!(after.active.configuration.id, 9);
    assert_eq!(after.active.key, before.active.key);
    assert_eq!(after.active.instance.visits, 2);
    assert_eq!(after.back_stack.len(), 1);
    restored_root.lifecycle().destroy()?;

    println!(
        "initializer called once; restored routes, keys and child state skipped initialization"
    );
    Ok(())
}
