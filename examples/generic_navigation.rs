#![allow(deprecated)] // Legacy context interoperability; see rust_api.rs for opaque stores.
use decompose_rs::router::children::{
    ChildItem, ChildNavState, ChildStatus, ChildrenOptions, NavState, NavStateCodec,
    SimpleNavigation, children,
};
use decompose_rs::{
    ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState, StateKeeper,
    StateStore,
};
use std::{cell::Cell, rc::Rc};

// Configurations, events, components and custom output do not need Clone.
#[derive(PartialEq, Eq)]
struct PhotoId(u8);
enum Event {
    Add(u8),
    Hide(u8),
    Show(u8),
}
struct GalleryState {
    revision: u8,
    entries: Vec<ChildNavState<PhotoId>>,
}
impl Clone for GalleryState {
    fn clone(&self) -> Self {
        Self {
            revision: self.revision,
            entries: self.entries.clone(),
        }
    }
}
impl NavState<PhotoId> for GalleryState {
    fn children(&self) -> Vec<ChildNavState<PhotoId>> {
        self.entries.clone()
    }
}
struct Gallery {
    revision: u8,
    photos: Vec<ChildItem<PhotoId, Photo>>,
}
struct Photo {
    context: DefaultComponentContext,
    edits: Rc<Cell<u8>>,
    generation: usize,
}
fn resumed() -> LifecycleRegistry {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    lifecycle
}
fn options() -> ChildrenOptions<Event, GalleryState> {
    ChildrenOptions {
        codec: Some(NavStateCodec::<GalleryState>::new(
            |state| {
                let mut bytes = vec![state.revision];
                for entry in &state.entries {
                    bytes.extend_from_slice(&[
                        entry.configuration.0,
                        match entry.status {
                            ChildStatus::Destroyed => 0,
                            ChildStatus::Created => 1,
                            ChildStatus::Started => 2,
                            ChildStatus::Resumed => 3,
                        },
                    ]);
                }
                Ok(Some(bytes))
            },
            |bytes| {
                let Some((&revision, entries)) = bytes.split_first() else {
                    return Err("missing gallery revision".into());
                };
                if entries.len() % 2 != 0 {
                    return Err("truncated photo descriptor".into());
                }
                let entries = entries
                    .chunks_exact(2)
                    .map(|entry| {
                        let status = match entry[1] {
                            0 => ChildStatus::Destroyed,
                            1 => ChildStatus::Created,
                            2 => ChildStatus::Started,
                            3 => ChildStatus::Resumed,
                            _ => return Err("unknown photo status".to_owned()),
                        };
                        Ok(ChildNavState::new(PhotoId(entry[0]), status))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                Ok(Some(GalleryState { revision, entries }))
            },
        )),
        back_transformer: Some(Rc::new(|state| {
            if state.entries.is_empty() {
                return None;
            }
            let current = state.clone();
            Some(Rc::new(move || {
                let mut next = current.clone();
                next.entries.pop();
                next.revision += 1;
                next
            }))
        })),
        on_event_complete: Some(Rc::new(|_, result| {
            result.unwrap();
        })),
        ..ChildrenOptions::default()
    }
}
fn transform(mut state: GalleryState, event: &Event) -> GalleryState {
    match event {
        Event::Add(id) => state
            .entries
            .push(ChildNavState::new(PhotoId(*id), ChildStatus::Resumed)),
        Event::Hide(id) | Event::Show(id) => {
            let entry = state
                .entries
                .iter_mut()
                .find(|entry| entry.configuration.0 == *id)
                .unwrap();
            entry.status = if matches!(event, Event::Hide(_)) {
                ChildStatus::Destroyed
            } else {
                ChildStatus::Resumed
            };
        }
    }
    state.revision += 1;
    state
}
fn project(state: &GalleryState, photos: Vec<ChildItem<PhotoId, Photo>>) -> Gallery {
    Gallery {
        revision: state.revision,
        photos,
    }
}
fn factory(generations: Rc<Cell<usize>>) -> impl Fn(&PhotoId, DefaultComponentContext) -> Photo {
    move |_, context| {
        generations.set(generations.get() + 1);
        let restored = context
            .state_store()
            .consume_bytes("edits")
            .unwrap()
            .and_then(|bytes| bytes.first().copied())
            .unwrap_or(0);
        let edits = Rc::new(Cell::new(restored));
        let saved = edits.clone();
        context
            .state_store()
            .register("edits", move || vec![saved.get()])
            .unwrap();
        Photo {
            context,
            edits,
            generation: generations.get(),
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = DefaultComponentContext::new(resumed());
    let navigation = SimpleNavigation::<Event>::new();
    let generations = Rc::new(Cell::new(0));
    let gallery = children(
        &root,
        &navigation,
        || GalleryState {
            revision: 0,
            entries: Vec::new(),
        },
        options(),
        transform,
        project,
        factory(generations.clone()),
    )?;
    assert!(gallery.get().photos.is_empty());
    assert!(!root.back_handler().back());

    navigation.navigate(Event::Add(1));
    navigation.navigate(Event::Add(2));
    let snapshot = gallery.get();
    let first = snapshot.photos[1].instance().unwrap().clone();
    let key = snapshot.photos[1].key().to_owned();
    first.edits.set(17);
    navigation.navigate(Event::Hide(2));
    assert_eq!(first.context.lifecycle().state(), LifecycleState::Destroyed);
    assert!(gallery.get().photos[1].instance().is_none());
    navigation.navigate(Event::Show(2));
    let second = gallery.get().photos[1].instance().unwrap().clone();
    assert!(!Rc::ptr_eq(&first, &second));
    assert_eq!(gallery.get().photos[1].key(), key);
    assert_eq!(second.edits.get(), 17);
    println!(
        "custom gallery revision {}: photo 2 generation {} -> {}, same key/state",
        gallery.get().revision,
        first.generation,
        second.generation
    );

    root.lifecycle().move_to(LifecycleState::Started)?;
    assert_eq!(second.context.lifecycle().state(), LifecycleState::Started);
    navigation.navigate(Event::Hide(2));
    let saved = root.state_store().save()?;
    root.lifecycle().destroy()?;

    let restored_root = DefaultComponentContext::with_state_store(
        resumed(),
        StateStore::from(StateKeeper::with_restored(saved)),
    );
    let restored_navigation = SimpleNavigation::<Event>::new();
    let restored = children(
        &restored_root,
        &restored_navigation,
        || -> GalleryState { panic!("valid saved state skips the initial factory") },
        options(),
        transform,
        project,
        factory(generations),
    )?;
    assert_eq!(restored.get().photos.len(), 2);
    assert!(restored.get().photos[1].instance().is_none());
    assert_eq!(restored.get().photos[1].key(), key);
    restored_navigation.navigate(Event::Show(2));
    assert_eq!(restored.get().photos[1].instance().unwrap().edits.get(), 17);
    assert!(restored_root.back_handler().back());
    assert_eq!(restored.get().photos.len(), 1);
    assert!(restored_root.back_handler().back());
    assert!(restored.get().photos.is_empty());
    assert!(!restored_root.back_handler().back());
    println!("restored inactive photo edits 17; conditional Back reaches an empty gallery");
    restored_root.lifecycle().destroy()?;
    Ok(())
}
