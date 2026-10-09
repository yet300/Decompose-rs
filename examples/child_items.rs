use decompose_rs::router::items::{
    ActiveLifecycleState, ItemMap, Items, ItemsCodec, ItemsNavigation, ItemsOptions, child_items,
};
use decompose_rs::{
    ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState, StateKeeper,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

// Neither configuration nor component needs Clone or Hash.
#[derive(PartialEq, Eq)]
struct RowId(u8);
struct Row {
    context: DefaultComponentContext,
    edits: Rc<Cell<u8>>,
    generation: usize,
}
fn resumed() -> LifecycleRegistry {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    lifecycle
}
fn codec() -> ItemsCodec<RowId> {
    ItemsCodec::<RowId>::new(
        |items| Ok(items.iter().map(|item| item.0).collect()),
        |bytes| Ok(bytes.iter().copied().map(RowId).collect()),
    )
}
fn factory(generations: Rc<Cell<usize>>) -> impl Fn(&RowId, DefaultComponentContext) -> Row {
    move |_, context| {
        generations.set(generations.get() + 1);
        let restored = context
            .state_keeper()
            .borrow_mut()
            .consume_bytes("edits")
            .and_then(|bytes| bytes.first().copied())
            .unwrap_or(0);
        let edits = Rc::new(Cell::new(restored));
        let saved_edits = edits.clone();
        context
            .state_keeper()
            .borrow_mut()
            .register("edits", move || vec![saved_edits.get()])
            .unwrap();
        Row {
            context,
            edits,
            generation: generations.get(),
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = DefaultComponentContext::new(resumed());
    let navigation = ItemsNavigation::new();
    let generations = Rc::new(Cell::new(0));
    let items = child_items(
        &root,
        &navigation,
        Items::new((0..5).map(RowId).collect())?,
        ItemsOptions {
            codec: Some(codec()),
            ..ItemsOptions::default()
        },
        factory(generations.clone()),
    )?;
    assert!(items.get().active_items.is_empty());
    assert_eq!(generations.get(), 0); // Membership alone invokes no factories.

    let first = items.get_or_create(&RowId(2))?;
    assert_eq!(first.context.lifecycle().state(), LifecycleState::Created);
    assert!(Rc::ptr_eq(&first, &items.get_or_create(&RowId(2))?));
    let key = items
        .get()
        .active_items
        .get(&RowId(2))
        .unwrap()
        .child
        .key
        .clone();
    first.edits.set(17);
    items.set_active_items(|mut active| {
        active.remove(&RowId(2));
        active
    });
    assert_eq!(first.context.lifecycle().state(), LifecycleState::Destroyed);
    let second = items.get_or_create(&RowId(2))?;
    assert!(!Rc::ptr_eq(&first, &second));
    assert_eq!(second.edits.get(), 17);
    assert_eq!(
        items.get().active_items.get(&RowId(2)).unwrap().child.key,
        key
    );
    println!(
        "row 2: same key, edits 17, new generation {} -> {}",
        first.generation, second.generation
    );

    // A viewport can request several independent live states. Row 2 becomes inactive.
    items.set_active_items(|_| {
        ItemMap::from_owned([
            (RowId(3), ActiveLifecycleState::Started),
            (RowId(4), ActiveLifecycleState::Resumed),
        ])
    });
    let row3 = items.get_or_create(&RowId(3))?;
    assert_eq!(row3.context.lifecycle().state(), LifecycleState::Started);
    let row4 = items.get_or_create(&RowId(4))?;
    assert_eq!(row4.context.lifecycle().state(), LifecycleState::Resumed);
    navigation.set_items(|mut configurations| {
        configurations.rotate_left(2);
        configurations
    });
    assert!(Rc::ptr_eq(&row3, &items.get_or_create(&RowId(3))?));
    assert!(!root.back_handler().back()); // No automatic pop or child handlers.
    assert_eq!(items.get().items.len(), 5);
    root.lifecycle().move_to(LifecycleState::Started)?;
    assert_eq!(row4.context.lifecycle().state(), LifecycleState::Started);
    assert_eq!(
        items
            .get()
            .active_items
            .get(&RowId(4))
            .unwrap()
            .lifecycle_state,
        ActiveLifecycleState::Resumed
    );

    let saved = root.state_keeper().borrow().save()?;
    root.lifecycle().destroy()?;
    let restored_root = DefaultComponentContext::with_state_keeper(
        resumed(),
        Rc::new(RefCell::new(StateKeeper::with_restored(saved))),
    );
    let restored = child_items(
        &restored_root,
        &ItemsNavigation::new(),
        Items::new(vec![RowId(99)])?,
        ItemsOptions {
            codec: Some(codec()),
            ..ItemsOptions::default()
        },
        factory(generations),
    )?;
    assert_eq!(
        restored
            .get()
            .items
            .iter()
            .map(|id| id.0)
            .collect::<Vec<_>>(),
        [2, 3, 4, 0, 1]
    );
    assert_eq!(restored.get().active_items.len(), 2);
    assert!(!restored.get().active_items.contains_key(&RowId(2)));
    assert_eq!(
        restored
            .get_or_create(&RowId(3))?
            .context
            .lifecycle()
            .state(),
        LifecycleState::Started
    );
    assert_eq!(
        restored
            .get_or_create(&RowId(4))?
            .context
            .lifecycle()
            .state(),
        LifecycleState::Resumed
    );
    let restored_row2 = restored.get_or_create(&RowId(2))?;
    assert_eq!(restored_row2.edits.get(), 17);
    println!("restored order/targets and inactive row 2 edits: 17");
    restored_root.lifecycle().destroy()?;
    Ok(())
}
