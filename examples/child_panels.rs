use decompose_rs::router::panels::{
    ChildPanelsMode, Panels, PanelsCodec, PanelsNavigation, PanelsNavigator, PanelsOptions,
    child_panels,
};
use decompose_rs::{
    ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState, StateKeeper,
    StateStore,
};
use std::{cell::Cell, rc::Rc};

// Every role has its own configuration and component type; none needs Clone.
#[derive(PartialEq, Eq)]
struct MainConfig(u8);
#[derive(PartialEq, Eq)]
struct DetailsConfig(u8);
#[derive(PartialEq, Eq)]
struct ExtraConfig(u8);
struct MainComponent(DefaultComponentContext);
struct DetailsComponent {
    context: DefaultComponentContext,
    edits: Rc<Cell<u8>>,
}
struct ExtraComponent(DefaultComponentContext);

fn resumed() -> LifecycleRegistry {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    lifecycle
}
fn codec() -> PanelsCodec<MainConfig, DetailsConfig, ExtraConfig> {
    PanelsCodec::new(
        |main: &MainConfig, details: Option<&DetailsConfig>, extra: Option<&ExtraConfig>| {
            Ok(vec![
                main.0,
                u8::from(details.is_some()),
                details.map_or(0, |details| details.0),
                u8::from(extra.is_some()),
                extra.map_or(0, |extra| extra.0),
            ])
        },
        |bytes| {
            let [main, details_flag, details, extra_flag, extra] = bytes else {
                return Err("expected five configuration bytes".into());
            };
            let details = match (*details_flag, *details) {
                (0, 0) => None,
                (1, value) => Some(DetailsConfig(value)),
                _ => return Err("invalid details configuration".into()),
            };
            let extra = match (*extra_flag, *extra) {
                (0, 0) => None,
                (1, value) => Some(ExtraConfig(value)),
                _ => return Err("invalid extra configuration".into()),
            };
            Ok((MainConfig(*main), details, extra))
        },
    )
}
fn details_factory(_: &DetailsConfig, context: DefaultComponentContext) -> DetailsComponent {
    let restored = context
        .state_store()
        .consume_bytes("edits")
        .unwrap()
        .and_then(|bytes| bytes.first().copied())
        .unwrap_or(0);
    let edits = Rc::new(Cell::new(restored));
    let saved_edits = edits.clone();
    context
        .state_store()
        .register("edits", move || vec![saved_edits.get()])
        .unwrap();
    DetailsComponent { context, edits }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = DefaultComponentContext::new(resumed());
    let navigation = PanelsNavigation::new();
    let mut initial = Panels::new(MainConfig(1));
    initial.details = Some(Rc::new(DetailsConfig(7)));
    initial.extra = Some(Rc::new(ExtraConfig(9)));
    let panels = child_panels(
        &root,
        &navigation,
        initial,
        PanelsOptions {
            handle_back_button: true,
            codec: Some(codec()),
            on_state_changed: Some(Rc::new(|new, old| {
                println!(
                    "mode: {:?}; details: {}; extra: {}; initial: {}",
                    new.mode,
                    new.details.is_some(),
                    new.extra.is_some(),
                    old.is_none(),
                );
            })),
            ..PanelsOptions::default()
        },
        |_, context| MainComponent(context),
        details_factory,
        |_, context| ExtraComponent(context),
    )?;
    let single = panels.get();
    assert_eq!(
        single.main.instance.0.lifecycle().state(),
        LifecycleState::Created
    );
    assert_eq!(
        single
            .extra
            .as_ref()
            .unwrap()
            .instance
            .0
            .lifecycle()
            .state(),
        LifecycleState::Resumed
    );
    let details = single.details.as_ref().unwrap().instance.clone();
    details.edits.set(23);

    navigation.set_mode_or_panic(ChildPanelsMode::Triple);
    assert!(Rc::ptr_eq(
        &single.main.instance,
        &panels.get().main.instance
    ));
    assert!(Rc::ptr_eq(
        &details,
        &panels.get().details.as_ref().unwrap().instance
    ));
    assert_eq!(details.context.lifecycle().state(), LifecycleState::Resumed);
    assert!(!root.back_handler().back()); // Automatic pop is disabled in Triple.
    navigation.set_mode_or_panic(ChildPanelsMode::Single);
    assert!(root.back_handler().back()); // Extra is removed before Details.
    assert!(panels.get().extra.is_none());
    assert!(panels.get().details.is_some());
    assert_eq!(
        single
            .extra
            .as_ref()
            .unwrap()
            .instance
            .0
            .lifecycle()
            .state(),
        LifecycleState::Destroyed
    );
    assert_eq!(details.context.lifecycle().state(), LifecycleState::Resumed);

    navigation.set_mode_or_panic(ChildPanelsMode::Dual);
    let saved = root.state_store().save()?;
    root.lifecycle().destroy()?;
    let restored_root = DefaultComponentContext::with_state_store(
        resumed(),
        StateStore::from(StateKeeper::with_restored(saved)),
    );
    let restored = child_panels(
        &restored_root,
        &PanelsNavigation::new(),
        Panels::new(MainConfig(99)),
        PanelsOptions {
            codec: Some(codec()),
            ..PanelsOptions::default()
        },
        |_, context| MainComponent(context),
        details_factory,
        |_, context| ExtraComponent(context),
    )?;
    let snapshot = restored.get();
    assert_eq!(snapshot.mode, ChildPanelsMode::Dual);
    assert_eq!(snapshot.main.configuration.0, 1);
    assert!(snapshot.extra.is_none());
    assert_eq!(snapshot.details.as_ref().unwrap().instance.edits.get(), 23);
    assert_eq!(
        snapshot.main.instance.0.lifecycle().state(),
        LifecycleState::Resumed
    );
    println!("restored Dual mode and Details edits: 23");
    restored_root.lifecycle().destroy()?;
    Ok(())
}
