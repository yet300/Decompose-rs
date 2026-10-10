#![allow(deprecated)] // Legacy context interoperability; see rust_api.rs for opaque stores.
use decompose_rs::router::pages::{Pages, PagesCodec, PagesNavigation, PagesOptions, child_pages};
use decompose_rs::{
    ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState, StateKeeper,
    StateStore,
};
use std::{cell::Cell, rc::Rc};

// Neither configurations nor components implement Clone.
#[derive(PartialEq, Eq)]
struct Tab(u8);
struct TabComponent {
    context: DefaultComponentContext,
    counter: Rc<Cell<u8>>,
    generation: usize,
}

fn resumed() -> LifecycleRegistry {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    lifecycle
}
fn codec() -> PagesCodec<Tab> {
    PagesCodec::new(
        |items: &[Rc<Tab>]| Ok(items.iter().map(|tab| tab.0).collect()),
        |bytes| Ok(bytes.iter().copied().map(Tab).collect()),
    )
}
fn factory(generations: Rc<Cell<usize>>) -> impl Fn(&Tab, DefaultComponentContext) -> TabComponent {
    move |_, context| {
        let restored = context
            .state_store()
            .consume_bytes("counter")
            .unwrap()
            .and_then(|bytes| bytes.first().copied())
            .unwrap_or(0);
        let counter = Rc::new(Cell::new(restored));
        let saved_counter = counter.clone();
        context
            .state_store()
            .register("counter", move || vec![saved_counter.get()])
            .unwrap();
        let generation = generations.get() + 1;
        generations.set(generation);
        TabComponent {
            context,
            counter,
            generation,
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = DefaultComponentContext::new(resumed());
    let navigation = PagesNavigation::new();
    let generations = Rc::new(Cell::new(0));
    let pages = child_pages(
        &root,
        &navigation,
        Pages::new((0..5).map(Tab).collect(), 0)?,
        PagesOptions {
            codec: Some(codec()),
            handle_back_button: true,
            ..PagesOptions::default()
        },
        factory(generations.clone()),
    )?;
    assert_eq!(generations.get(), 2); // Only selection and its neighbor exist.
    let first_snapshot = pages.get();
    let first_key = first_snapshot.items[0].key().to_owned();
    let first = first_snapshot.items[0].instance().unwrap().clone();
    first.counter.set(17);

    navigation.select_or_panic(3);
    assert!(pages.get().items[0].instance().is_none());
    assert_eq!(first.context.lifecycle().state(), LifecycleState::Destroyed);
    navigation.select_or_panic(0);
    let recreated_snapshot = pages.get();
    let recreated = recreated_snapshot.items[0].instance().unwrap();
    assert_eq!(recreated_snapshot.items[0].key(), first_key);
    assert!(!Rc::ptr_eq(&first, recreated));
    assert_eq!(recreated.counter.get(), 17);
    println!(
        "page 0: same key, saved counter {}, new generation {} -> {}",
        recreated.counter.get(),
        first.generation,
        recreated.generation,
    );

    navigation.select_or_panic(3);
    assert!(root.back_handler().back());
    assert_eq!(pages.get().selected_index, Some(2));
    navigation.select_or_panic(3);
    let saved = root.state_store().save()?;
    root.lifecycle().destroy()?;
    let restored_root = DefaultComponentContext::with_state_store(
        resumed(),
        StateStore::from(StateKeeper::with_restored(saved)),
    );
    let restored_navigation = PagesNavigation::new();
    let restored_pages = child_pages(
        &restored_root,
        &restored_navigation,
        Pages::default(), // Saved membership and selection override this initial state.
        PagesOptions {
            codec: Some(codec()),
            ..PagesOptions::default()
        },
        factory(generations),
    )?;
    assert_eq!(restored_pages.get().selected_index, Some(3));
    assert_eq!(restored_pages.get().items.len(), 5);
    assert!(restored_pages.get().items[0].instance().is_none());
    restored_navigation.select_or_panic(0);
    let restored_snapshot = restored_pages.get();
    assert_eq!(restored_snapshot.items[0].key(), first_key);
    assert_eq!(
        restored_snapshot.items[0].instance().unwrap().counter.get(),
        17
    );
    println!("restored selection: 3; recreated page 0 counter: 17");
    restored_root.lifecycle().destroy()?;
    Ok(())
}
