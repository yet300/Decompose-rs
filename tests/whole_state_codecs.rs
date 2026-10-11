use decompose_rs::{
    router::{NavigationError, items::*, pages::*, panels::*, slot::*},
    *,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

#[derive(Clone, Copy, Debug)]
enum Family {
    Pages,
    Items,
    Panels,
}
const FAMILIES: [Family; 3] = [Family::Pages, Family::Items, Family::Panels];
impl Family {
    fn key(self) -> &'static str {
        match self {
            Self::Pages => "DefaultChildPages",
            Self::Items => "DefaultChildItems",
            Self::Panels => "DefaultChildPanels",
        }
    }
}
#[derive(Clone, Default)]
struct Policy {
    decodes: Rc<Cell<usize>>,
    factories: Rc<Cell<usize>>,
    transform: Rc<Cell<bool>>,
    invalid: Rc<Cell<bool>>,
    wrong_count: Rc<Cell<bool>>,
    duplicate: Rc<Cell<bool>>,
    decline: Rc<Cell<bool>>,
    skip: Rc<Cell<bool>>,
    encode_error: Rc<Cell<bool>>,
    decode_error: Rc<Cell<bool>>,
    factory_panic: Rc<Cell<bool>>,
}
struct ChildData {
    restored: Option<Vec<u8>>,
    context: DefaultComponentContext,
}
impl Policy {
    fn factory(&self, c: &u8, context: DefaultComponentContext) -> ChildData {
        self.factories.set(self.factories.get() + 1);
        assert!(!self.factory_panic.get(), "factory panic");
        let restored = context.state_store().consume_bytes("nested").unwrap();
        let value = *c;
        context
            .state_store()
            .register("nested", move || vec![value])
            .unwrap();
        ChildData { restored, context }
    }
    fn encoded(&self, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
        if self.encode_error.get() {
            Err("encode error".into())
        } else {
            Ok(bytes)
        }
    }
    fn decoding(&self) -> Result<(), String> {
        self.decodes.set(self.decodes.get() + 1);
        if self.decode_error.get() {
            Err("decode error".into())
        } else {
            Ok(())
        }
    }
}
struct View {
    members: Vec<u8>,
    keys: Vec<String>,
    restored: Vec<Option<Vec<u8>>>,
    states: Vec<LifecycleState>,
    metadata: usize,
}
fn root(saved: BTreeMap<String, Vec<u8>>) -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::with_stores(
        lifecycle,
        StateStore::from(StateKeeper::with_restored(saved)),
        InstanceStore::from(InstanceKeeper::new()),
        SharedBack::new(),
    )
}
fn build(
    family: Family,
    parent: &DefaultComponentContext,
    policy: &Policy,
    initial: impl FnOnce() -> Vec<u8>,
) -> Result<View, NavigationError> {
    let e = policy.clone();
    let d = policy.clone();
    let s = policy.clone();
    let r = policy.clone();
    let f = policy.clone();
    let factory = move |c: &u8, context| f.factory(c, context);
    let (members, children, metadata) = match family {
        Family::Pages => {
            let codec = PagesCodec::new_state(
                move |p: &Pages<u8>| {
                    e.encoded(
                        std::iter::once(p.selected_index.unwrap_or(255) as u8)
                            .chain(p.items.iter().map(|c| **c))
                            .collect(),
                    )
                },
                move |b| {
                    d.decoding()?;
                    let mut p = Pages {
                        items: b[1..].iter().copied().map(Rc::new).collect(),
                        selected_index: (b[0] != 255).then_some(b[0] as usize),
                    };
                    if d.transform.get() {
                        p.selected_index = Some(0);
                    }
                    if d.invalid.get() {
                        p.selected_index = Some(p.items.len());
                    }
                    if d.wrong_count.get() {
                        p.items.push(Rc::new(255));
                    }
                    if d.duplicate.get() {
                        p.items[1] = p.items[0].clone();
                    }
                    Ok(p)
                },
            )
            .should_save(move |_| !s.skip.get())
            .should_restore(move |_| !r.decline.get());
            let value = child_pages_with_initial(
                parent,
                &PagesNavigation::new(),
                || Pages::new(initial(), 1).unwrap(),
                PagesOptions {
                    codec: Some(codec.clone()),
                    ..Default::default()
                },
                factory,
            )?;
            let p = value.get();
            (
                p.items.iter().map(|c| **c.configuration()).collect(),
                p.items
                    .iter()
                    .filter_map(|c| {
                        if let PageChild::Created(c) = c {
                            Some(c.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
                p.selected_index.unwrap_or(255),
            )
        }
        Family::Items => {
            let codec = ItemsCodec::new_state(
                move |p: &Items<u8>| {
                    e.encoded(
                        p.items
                            .iter()
                            .flat_map(|c| {
                                [
                                    **c,
                                    match p.active_items.get(c) {
                                        None => 0,
                                        Some(ActiveLifecycleState::Created) => 1,
                                        Some(ActiveLifecycleState::Started) => 2,
                                        Some(ActiveLifecycleState::Resumed) => 3,
                                    },
                                ]
                            })
                            .collect(),
                    )
                },
                move |b| {
                    d.decoding()?;
                    let mut p = Items::new(b.chunks_exact(2).map(|b| b[0]).collect()).unwrap();
                    for (c, b) in p.items.iter().zip(b.chunks_exact(2)) {
                        let status = if d.transform.get() { 3 } else { b[1] };
                        if let Some(status) = match status {
                            1 => Some(ActiveLifecycleState::Created),
                            2 => Some(ActiveLifecycleState::Started),
                            3 => Some(ActiveLifecycleState::Resumed),
                            _ => None,
                        } {
                            p.active_items.insert(c.clone(), status);
                        }
                    }
                    if d.invalid.get() {
                        p.items.pop();
                    }
                    if d.duplicate.get() {
                        p.items[1] = p.items[0].clone();
                    }
                    Ok(p)
                },
            )
            .should_save(move |_| !s.skip.get())
            .should_restore(move |_| !r.decline.get());
            let value = child_items_with_initial(
                parent,
                &ItemsNavigation::new(),
                || {
                    let mut p = Items::new(initial()).unwrap();
                    for c in &p.items {
                        p.active_items
                            .insert(c.clone(), ActiveLifecycleState::Started);
                    }
                    p
                },
                ItemsOptions {
                    codec: Some(codec.clone()),
                    ..Default::default()
                },
                factory,
            )?;
            let p = value.get();
            let children = p
                .items
                .iter()
                .filter_map(|c| p.active_items.get(c).map(|c| c.child.clone()))
                .collect::<Vec<_>>();
            let metadata = usize::from(
                children
                    .iter()
                    .all(|c| c.instance.context.lifecycle().state() == LifecycleState::Resumed),
            );
            (p.items.iter().map(|c| **c).collect(), children, metadata)
        }
        Family::Panels => {
            let codec: PanelsCodec<u8, u8, u8> = PanelsCodec::new_state(
                move |p: &Panels<u8, u8, u8>| {
                    e.encoded(vec![
                        match p.mode {
                            ChildPanelsMode::Single => 0,
                            ChildPanelsMode::Dual => 1,
                            ChildPanelsMode::Triple => 2,
                        },
                        *p.main,
                        p.details.as_deref().copied().unwrap_or(255),
                        p.extra.as_deref().copied().unwrap_or(255),
                    ])
                },
                move |b| {
                    d.decoding()?;
                    let mut p = Panels {
                        main: Rc::new(b[1]),
                        details: (b[2] != 255).then(|| Rc::new(b[2])),
                        extra: (b[3] != 255).then(|| Rc::new(b[3])),
                        mode: match b[0] {
                            0 => ChildPanelsMode::Single,
                            1 => ChildPanelsMode::Dual,
                            _ => ChildPanelsMode::Triple,
                        },
                    };
                    if d.transform.get() {
                        p.mode = ChildPanelsMode::Single;
                    }
                    if d.invalid.get() {
                        p.extra = Some(Rc::new(255));
                    }
                    Ok(p)
                },
            )
            .should_save(move |_| !s.skip.get())
            .should_restore(move |_| !r.decline.get());
            let fd = policy.clone();
            let fe = policy.clone();
            let value = child_panels_with_initial(
                parent,
                &PanelsNavigation::new(),
                || {
                    let c = initial();
                    Panels {
                        main: Rc::new(c[0]),
                        details: c.get(1).copied().map(Rc::new),
                        extra: None,
                        mode: ChildPanelsMode::Dual,
                    }
                },
                PanelsOptions {
                    codec: Some(codec.clone()),
                    ..Default::default()
                },
                factory,
                move |c, ctx| fd.factory(c, ctx),
                move |c, ctx| fe.factory(c, ctx),
            )?;
            let p = value.get();
            let children = std::iter::once(p.main.clone())
                .chain(p.details.clone())
                .chain(p.extra.clone())
                .collect::<Vec<_>>();
            (
                children.iter().map(|c| *c.configuration).collect(),
                children,
                usize::from(p.mode != ChildPanelsMode::Single),
            )
        }
    };
    Ok(View {
        members,
        keys: children.iter().map(|c| c.key.clone()).collect(),
        restored: children
            .iter()
            .map(|c| c.instance.restored.clone())
            .collect(),
        states: children
            .iter()
            .map(|c| c.instance.context.lifecycle().state())
            .collect(),
        metadata,
    })
}
fn saved(family: Family) -> (BTreeMap<String, Vec<u8>>, View) {
    let p = root(BTreeMap::new());
    let view = build(family, &p, &Policy::default(), || vec![7, 8]).unwrap();
    (p.state_store().save().unwrap(), view)
}

#[test]
fn whole_state_decoders_override_metadata_and_preserve_nested_state_and_keys() {
    for family in FAMILIES {
        let (saved, old) = saved(family);
        let p = root(saved);
        let policy = Policy::default();
        policy.transform.set(true);
        let restored = build(family, &p, &policy, || {
            panic!("accepted restore initialized")
        })
        .unwrap();
        assert_eq!(restored.members, old.members, "{family:?}");
        assert_eq!(restored.keys, old.keys);
        assert_eq!(restored.restored, vec![Some(vec![7]), Some(vec![8])]);
        assert_eq!(policy.decodes.get(), 1);
        match family {
            Family::Pages => {
                assert_eq!(restored.metadata, 0);
                assert_eq!(
                    restored.states,
                    [LifecycleState::Resumed, LifecycleState::Created]
                );
            }
            Family::Items => {
                assert_eq!(restored.metadata, 1);
                assert_eq!(
                    restored.states,
                    [LifecycleState::Resumed, LifecycleState::Resumed]
                );
            }
            Family::Panels => {
                assert_eq!(restored.metadata, 0);
                assert_eq!(
                    restored.states,
                    [LifecycleState::Created, LifecycleState::Resumed]
                );
            }
        }
    }
}
#[test]
fn invalid_whole_state_is_rejected_before_factories_and_restoration_is_retryable() {
    for family in FAMILIES {
        let (saved, _) = saved(family);
        let p = root(saved.clone());
        let policy = Policy::default();
        policy.invalid.set(true);
        policy.decline.set(true);
        assert!(
            build(family, &p, &policy, || panic!(
                "invalid decoded state initialized"
            ))
            .is_err()
        );
        assert_eq!(policy.factories.get(), 0);
        assert_eq!(p.state_store().save().unwrap(), saved);
        policy.invalid.set(false);
        policy.decline.set(false);
        assert_eq!(
            build(family, &p, &policy, || panic!("retry initialized"))
                .unwrap()
                .members,
            [7, 8]
        );
    }
}
#[test]
fn invalid_envelope_metadata_is_rejected_before_whole_state_decoder() {
    for family in FAMILIES {
        let (mut saved, _) = saved(family);
        let b = saved.get_mut(family.key()).unwrap();
        match family {
            Family::Pages => b[12..20].copy_from_slice(&99u64.to_le_bytes()),
            Family::Items => {
                let payload_len = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
                b[20 + payload_len] = 255;
            }
            Family::Panels => b[12] = 255,
        }
        let p = root(saved.clone());
        let policy = Policy::default();
        assert!(build(family, &p, &policy, || panic!("corrupt data initialized")).is_err());
        assert_eq!(policy.decodes.get(), 0);
        assert_eq!(policy.factories.get(), 0);
        assert_eq!(p.state_store().save().unwrap(), saved);
    }
}
#[test]
fn whole_state_codecs_keep_save_and_restore_decline_policies() {
    for family in FAMILIES {
        let (saved, _) = saved(family);
        let p = root(saved);
        let policy = Policy::default();
        policy.decline.set(true);
        policy.skip.set(true);
        policy.encode_error.set(true);
        let mut initial_calls = 0;
        let fresh = build(family, &p, &policy, || {
            initial_calls += 1;
            vec![9, 10]
        })
        .unwrap();
        assert_eq!(fresh.members, [9, 10]);
        assert_eq!(fresh.restored, [None, None]);
        assert_eq!(initial_calls, 1);
        assert!(p.state_store().save().unwrap().is_empty());
    }
}
#[test]
fn whole_state_decode_errors_and_factory_panics_preserve_retryable_saved_bytes() {
    for family in FAMILIES {
        let (saved, _) = saved(family);
        let p = root(saved.clone());
        let policy = Policy::default();
        policy.decode_error.set(true);
        assert!(
            matches!(build(family, &p, &policy, || panic!("codec failure initialized")), Err(NavigationError::StateSource(message)) if message.to_string() == "decode error")
        );
        assert_eq!(policy.factories.get(), 0);
        assert_eq!(p.state_store().save().unwrap(), saved);
        policy.decode_error.set(false);
        policy.factory_panic.set(true);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(
                family,
                &p,
                &policy,
                || panic!("factory failure initialized")
            )))
            .is_err()
        );
        assert_eq!(p.state_store().save().unwrap(), saved);
        policy.factory_panic.set(false);
        assert_eq!(
            build(family, &p, &policy, || panic!("retry initialized"))
                .unwrap()
                .restored,
            [Some(vec![7]), Some(vec![8])]
        );
    }
}

#[test]
fn whole_state_decoders_cannot_change_page_count_or_introduce_duplicates() {
    for family in [Family::Pages, Family::Items] {
        let (saved, _) = saved(family);
        let p = root(saved.clone());
        let policy = Policy::default();
        policy.duplicate.set(true);
        assert!(matches!(
            build(family, &p, &policy, || panic!("duplicate initialized")),
            Err(NavigationError::DuplicateConfiguration)
        ));
        assert_eq!(policy.factories.get(), 0);
        assert_eq!(p.state_store().save().unwrap(), saved);
        policy.duplicate.set(false);
        if matches!(family, Family::Pages) {
            policy.wrong_count.set(true);
            assert!(matches!(
                build(family, &p, &policy, || panic!("wrong count initialized")),
                Err(NavigationError::StateSource(_))
            ));
            assert_eq!(policy.factories.get(), 0);
            assert_eq!(p.state_store().save().unwrap(), saved);
            policy.wrong_count.set(false);
        }
        build(family, &p, &policy, || panic!("valid retry initialized")).unwrap();
    }
}

#[test]
fn whole_state_codecs_restore_empty_pages_and_items_without_initializing() {
    let pages: PagesCodec<u8> = PagesCodec::new_state(
        |p| {
            assert!(p.items.is_empty());
            assert_eq!(p.selected_index, None);
            Ok(vec![])
        },
        |b| {
            assert!(b.is_empty());
            Ok(Pages::default())
        },
    );
    let items: ItemsCodec<u8> = ItemsCodec::new_state(
        |p| {
            assert!(p.items.is_empty());
            assert!(p.active_items.is_empty());
            Ok(vec![])
        },
        |b| {
            assert!(b.is_empty());
            Ok(Items::default())
        },
    );
    let p = root(BTreeMap::new());
    child_pages(
        &p,
        &PagesNavigation::new(),
        Pages::default(),
        PagesOptions {
            codec: Some(pages.clone()),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    child_items(
        &p,
        &ItemsNavigation::new(),
        Items::default(),
        ItemsOptions {
            codec: Some(items.clone()),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let p = root(p.state_store().save().unwrap());
    let restored_pages = child_pages_with_initial(
        &p,
        &PagesNavigation::new(),
        || panic!("empty pages initialized"),
        PagesOptions {
            codec: Some(pages),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let restored_items = child_items_with_initial(
        &p,
        &ItemsNavigation::new(),
        || panic!("empty items initialized"),
        ItemsOptions {
            codec: Some(items),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(restored_pages.get().items.is_empty());
    assert!(restored_items.get().items.is_empty());
}

#[test]
fn whole_state_items_can_restore_all_inactive_and_lazily_recover_saved_children() {
    let (saved, _) = saved(Family::Items);
    let p = root(saved);
    let policy = Policy::default();
    let f = policy.clone();
    let items = child_items_with_initial(
        &p,
        &ItemsNavigation::new(),
        || panic!("inactive items initialized"),
        ItemsOptions {
            codec: Some(ItemsCodec::new_state(
                |p: &Items<u8>| Ok(p.items.iter().map(|c| **c).collect()),
                |b| {
                    Items::new(b.chunks_exact(2).map(|b| b[0]).collect()).map_err(|e| e.to_string())
                },
            )),
            ..Default::default()
        },
        move |c, ctx| f.factory(c, ctx),
    )
    .unwrap();
    assert!(items.get().active_items.is_empty());
    assert_eq!(policy.factories.get(), 0);
    let child = items.get_or_create(&7).unwrap();
    assert_eq!(child.restored, Some(vec![7]));
    assert_eq!(policy.factories.get(), 1);
}

#[test]
fn new_state_supports_panels_without_extra_and_receives_mode_during_encoding() {
    let codec: PanelsCodec<u8, u8> = PanelsCodec::new_state(
        |p| {
            assert_eq!(p.mode, ChildPanelsMode::Dual);
            Ok(vec![*p.main, **p.details.as_ref().unwrap()])
        },
        |b| {
            Ok(Panels {
                main: Rc::new(b[0]),
                details: Some(Rc::new(b[1])),
                extra: None,
                mode: ChildPanelsMode::Single,
            })
        },
    );
    let p = root(BTreeMap::new());
    child_panels_without_extra(
        &p,
        &PanelsNavigation::new(),
        Panels {
            main: Rc::new(1),
            details: Some(Rc::new(2)),
            extra: None,
            mode: ChildPanelsMode::Dual,
        },
        PanelsOptions {
            codec: Some(codec.clone()),
            ..Default::default()
        },
        |_, ctx| ctx,
        |_, ctx| ctx,
    )
    .unwrap();
    let p = root(p.state_store().save().unwrap());
    let panels = child_panels_without_extra_with_initial(
        &p,
        &PanelsNavigation::new(),
        || panic!("accepted panels initialized"),
        PanelsOptions {
            codec: Some(codec),
            ..Default::default()
        },
        |_, ctx| ctx,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(panels.get().mode, ChildPanelsMode::Single);
}

#[test]
fn nullable_slot_restoration_accepts_empty_and_keeps_the_identity_counter() {
    let parent = root(BTreeMap::new());
    child_slot(
        &parent,
        &SlotNavigation::new(),
        Some(7u8),
        SlotOptions {
            codec: Some(SlotCodec::new(|c| Ok(vec![*c]), |b| Ok(b[0]))),
            ..Default::default()
        },
        |c, ctx| {
            let value = *c;
            ctx.state_store()
                .register("nested", move || vec![value])
                .unwrap();
            ctx
        },
    )
    .unwrap();
    let saved = parent.state_store().save().unwrap();
    let parent = root(saved);
    let navigation = SlotNavigation::new();
    let restored = Rc::new(RefCell::new(Vec::new()));
    let out = restored.clone();
    let slot = child_slot_with_initial(
        &parent,
        &navigation,
        || panic!("accepted empty slot initialized"),
        SlotOptions {
            codec: Some(SlotCodec::new_state(
                |c| Ok(vec![*c]),
                |b| {
                    assert_eq!(b, [7]);
                    Ok(None)
                },
            )),
            ..Default::default()
        },
        move |_: &u8, ctx| {
            out.borrow_mut()
                .push(ctx.state_store().consume_bytes("nested").unwrap());
            ctx
        },
    )
    .unwrap();
    assert!(slot.get().child.is_none());
    assert!(restored.borrow().is_empty());
    let empty = parent.state_store().save().unwrap();
    assert_eq!(empty["DefaultChildSlot"].len(), 16);
    navigation.activate_or_panic(9);
    let current = slot.get();
    assert_eq!(current.child.as_ref().unwrap().key, "child/1");
    assert_eq!(&*restored.borrow(), &[None]);
}

#[test]
fn nullable_slot_decoder_errors_and_factory_panics_preserve_saved_data_for_retry() {
    let p = root(BTreeMap::new());
    child_slot(
        &p,
        &SlotNavigation::new(),
        Some(7u8),
        SlotOptions {
            codec: Some(SlotCodec::new(|c| Ok(vec![*c]), |b| Ok(b[0]))),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = p.state_store().save().unwrap();
    let p = root(saved.clone());
    let error = child_slot_with_initial(
        &p,
        &SlotNavigation::new(),
        || panic!("slot error initialized"),
        SlotOptions {
            codec: Some(SlotCodec::new_state(
                |c: &u8| Ok(vec![*c]),
                |_| Err("slot decode error".into()),
            )),
            ..Default::default()
        },
        |_, ctx| ctx,
    );
    assert!(
        matches!(error, Err(NavigationError::StateSource(message)) if message.to_string() == "slot decode error")
    );
    assert_eq!(p.state_store().save().unwrap(), saved);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| child_slot_with_initial(
            &p,
            &SlotNavigation::new(),
            || panic!("slot factory error initialized"),
            SlotOptions {
                codec: Some(SlotCodec::new_state(|c| Ok(vec![*c]), |b| Ok(Some(b[0])))),
                ..Default::default()
            },
            |_: &u8, _| panic!("slot factory panic")
        )))
        .is_err()
    );
    assert_eq!(p.state_store().save().unwrap(), saved);
    let slot = child_slot_with_initial(
        &p,
        &SlotNavigation::new(),
        || panic!("slot retry initialized"),
        SlotOptions {
            codec: Some(SlotCodec::new_state(|c: &u8| Ok(vec![*c]), |_| Ok(None))),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(slot.get().child.is_none());
}

#[test]
fn nullable_slot_preserves_empty_dsl1_and_memory_modes_without_codec_callbacks() {
    let empty_codec = SlotCodec::new_state(
        |_: &u8| panic!("empty slot encoded"),
        |_| panic!("empty slot decoded"),
    )
    .should_save(|c| {
        assert!(c.is_none());
        true
    })
    .should_restore(|c| {
        assert!(c.is_none());
        true
    });
    let p = root(BTreeMap::new());
    child_slot(
        &p,
        &SlotNavigation::new(),
        None,
        SlotOptions {
            codec: Some(empty_codec.clone()),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = p.state_store().save().unwrap();
    let p = root(saved.clone());
    let slot = child_slot_with_initial(
        &p,
        &SlotNavigation::new(),
        || panic!("saved empty slot initialized"),
        SlotOptions {
            codec: Some(empty_codec),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(slot.get().child.is_none());
    assert_eq!(p.state_store().save().unwrap(), saved);
    let p = TestComponentContext::new();
    p.state_store()
        .enable_memory_navigation("DefaultChildSlot")
        .unwrap();
    let memory_codec = SlotCodec::new_state(
        |_: &u8| panic!("memory slot encoded"),
        |_| panic!("memory slot decoded"),
    );
    child_slot(
        &p,
        &SlotNavigation::new(),
        Some(7),
        SlotOptions {
            codec: Some(memory_codec.clone()),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let recreated = p.recreate(true).unwrap();
    let slot = child_slot_with_initial(
        &recreated,
        &SlotNavigation::new(),
        || panic!("memory slot initialized"),
        SlotOptions {
            codec: Some(memory_codec),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(*slot.get().child.as_ref().unwrap().configuration, 7);
}

#[test]
fn whole_state_decoders_can_read_existing_configuration_codec_payloads() {
    let p = root(BTreeMap::new());
    child_pages(
        &p,
        &PagesNavigation::new(),
        Pages::new(vec![7u8, 8], 1).unwrap(),
        PagesOptions {
            codec: Some(PagesCodec::new(
                |c| Ok(c.iter().map(|c| **c).collect()),
                |b| Ok(b.to_vec()),
            )),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let mut items = Items::new(vec![7u8, 8]).unwrap();
    items
        .active_items
        .insert(Rc::new(7), ActiveLifecycleState::Started);
    child_items(
        &p,
        &ItemsNavigation::new(),
        items,
        ItemsOptions {
            codec: Some(ItemsCodec::new(
                |c| Ok(c.iter().map(|c| **c).collect()),
                |b| Ok(b.to_vec()),
            )),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    child_panels_without_extra(
        &p,
        &PanelsNavigation::new(),
        Panels {
            main: Rc::new(7u8),
            details: Some(Rc::new(8)),
            extra: None,
            mode: ChildPanelsMode::Dual,
        },
        PanelsOptions {
            codec: Some(PanelsCodec::without_extra(
                |m, d| Ok(vec![*m, *d.unwrap()]),
                |b| Ok((b[0], Some(b[1]))),
            )),
            ..Default::default()
        },
        |_, ctx| ctx,
        |_, ctx| ctx,
    )
    .unwrap();
    let p = root(p.state_store().save().unwrap());
    let pages = child_pages_with_initial(
        &p,
        &PagesNavigation::new(),
        || panic!("legacy pages initialized"),
        PagesOptions {
            codec: Some(PagesCodec::new_state(
                |_: &Pages<u8>| panic!("not saving"),
                |b| {
                    assert_eq!(b, [7, 8]);
                    Ok(Pages::new(b.to_vec(), 0).unwrap())
                },
            )),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let items = child_items_with_initial(
        &p,
        &ItemsNavigation::new(),
        || panic!("legacy items initialized"),
        ItemsOptions {
            codec: Some(ItemsCodec::new_state(
                |_: &Items<u8>| panic!("not saving"),
                |b| {
                    assert_eq!(b, [7, 8]);
                    Ok(Items::new(b.to_vec()).unwrap())
                },
            )),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let panels = child_panels_without_extra_with_initial(
        &p,
        &PanelsNavigation::new(),
        || panic!("legacy panels initialized"),
        PanelsOptions {
            codec: Some(PanelsCodec::new_state(
                |_: &Panels<u8, u8>| panic!("not saving"),
                |b| {
                    assert_eq!(b, [7, 8]);
                    Ok(Panels {
                        main: Rc::new(b[0]),
                        details: Some(Rc::new(b[1])),
                        extra: None,
                        mode: ChildPanelsMode::Single,
                    })
                },
            )),
            ..Default::default()
        },
        |_, ctx| ctx,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(pages.get().selected_index, Some(0));
    assert!(items.get().active_items.is_empty());
    assert_eq!(panels.get().mode, ChildPanelsMode::Single);
}
