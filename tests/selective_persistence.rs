use decompose_rs::{
    router::{items::*, pages::*, panels::*, slot::*, stack::*},
    *,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

#[derive(Clone, Copy, Debug)]
enum Family {
    Stack,
    Slot,
    Pages,
    Panels,
    Items,
}
const FAMILIES: [Family; 5] = [
    Family::Stack,
    Family::Slot,
    Family::Pages,
    Family::Panels,
    Family::Items,
];
impl Family {
    fn key(self) -> &'static str {
        match self {
            Self::Stack => "DefaultChildStack",
            Self::Slot => "DefaultChildSlot",
            Self::Pages => "DefaultChildPages",
            Self::Panels => "DefaultChildPanels",
            Self::Items => "DefaultChildItems",
        }
    }
}
#[derive(Clone)]
struct Policy {
    save: Rc<Cell<bool>>,
    restore: Rc<Cell<bool>>,
    saves: Rc<Cell<usize>>,
    restores: Rc<Cell<usize>>,
    child_saves: Rc<Cell<usize>>,
    fail_factory: Rc<Cell<bool>>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            save: Rc::new(Cell::new(true)),
            restore: Rc::new(Cell::new(true)),
            saves: Default::default(),
            restores: Default::default(),
            child_saves: Default::default(),
            fail_factory: Default::default(),
        }
    }
}
impl Policy {
    fn save(&self) -> bool {
        self.saves.set(self.saves.get() + 1);
        self.save.get()
    }
    fn restore(&self) -> bool {
        self.restores.set(self.restores.get() + 1);
        self.restore.get()
    }
    fn factory(&self, c: &u8, ctx: DefaultComponentContext) -> Component {
        if self.fail_factory.get() {
            panic!("factory failure");
        }
        let nested = ctx.state_store().consume_bytes("nested").unwrap();
        let calls = self.child_saves.clone();
        let n = *c;
        ctx.state_store()
            .register("nested", move || {
                calls.set(calls.get() + 1);
                vec![n]
            })
            .unwrap();
        let retained = ctx
            .instance_store()
            .get_or_create("retained", || *c)
            .unwrap();
        Component {
            nested,
            retained,
            context: ctx,
        }
    }
}
struct Component {
    nested: Option<Vec<u8>>,
    retained: Rc<u8>,
    context: DefaultComponentContext,
}
struct Snapshot {
    members: Vec<u8>,
    live: Vec<(String, Rc<Component>)>,
}
fn root(saved: BTreeMap<String, Vec<u8>>, instances: InstanceStore) -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::with_stores(
        lifecycle,
        StateStore::from(StateKeeper::with_restored(saved)),
        instances,
        SharedBack::new(),
    )
}
fn fresh() -> DefaultComponentContext {
    root(
        Default::default(),
        InstanceStore::from(InstanceKeeper::new()),
    )
}
fn encode(c: &[Rc<u8>]) -> Result<Vec<u8>, String> {
    Ok(c.iter().map(|c| **c).collect())
}
fn decode(b: &[u8]) -> Result<Vec<u8>, String> {
    Ok(b.to_vec())
}
fn build(
    family: Family,
    parent: &DefaultComponentContext,
    initial: impl FnOnce() -> Vec<u8>,
    policy: &Policy,
) -> Result<Snapshot, decompose_rs::router::NavigationError> {
    let s = policy.clone();
    let r = policy.clone();
    let f = policy.clone();
    let factory = move |c: &u8, ctx| f.factory(c, ctx);
    Ok(match family {
        Family::Stack => {
            let v = child_stack_with_initial(
                parent,
                &StackNavigation::new(),
                initial,
                StackOptions {
                    codec: Some(
                        StackCodec::new(encode, decode)
                            .should_save(move |_| s.save())
                            .should_restore(move |_| r.restore()),
                    ),
                    ..Default::default()
                },
                factory,
            )?;
            let snap = v.get();
            Snapshot {
                members: snap.items().map(|c| *c.configuration).collect(),
                live: snap
                    .items()
                    .map(|c| (c.key.clone(), c.instance.clone()))
                    .collect(),
            }
        }
        Family::Slot => {
            let v = child_slot_with_initial(
                parent,
                &SlotNavigation::new(),
                || initial().into_iter().next(),
                SlotOptions {
                    codec: Some(
                        SlotCodec::new(
                            |c: &u8| Ok(vec![*c]),
                            |b| b.first().copied().ok_or_else(|| "empty payload".into()),
                        )
                        .should_save(move |_| s.save())
                        .should_restore(move |_| r.restore()),
                    ),
                    ..Default::default()
                },
                factory,
            )?;
            let snap = v.get();
            Snapshot {
                members: snap.child.iter().map(|c| *c.configuration).collect(),
                live: snap
                    .child
                    .iter()
                    .map(|c| (c.key.clone(), c.instance.clone()))
                    .collect(),
            }
        }
        Family::Pages => {
            let v = child_pages_with_initial(
                parent,
                &PagesNavigation::new(),
                || Pages::new(initial(), 0).unwrap(),
                PagesOptions {
                    codec: Some(
                        PagesCodec::new(encode, decode)
                            .should_save(move |_| s.save())
                            .should_restore(move |_| r.restore()),
                    ),
                    ..Default::default()
                },
                factory,
            )?;
            let snap = v.get();
            Snapshot {
                members: snap.items.iter().map(|c| **c.configuration()).collect(),
                live: snap
                    .items
                    .iter()
                    .filter_map(|c| match c {
                        PageChild::Created(c) => Some((c.key.clone(), c.instance.clone())),
                        _ => None,
                    })
                    .collect(),
            }
        }
        Family::Panels => {
            let d = policy.clone();
            let e = policy.clone();
            let v = child_panels_with_initial(
                parent,
                &PanelsNavigation::new(),
                || {
                    let c = initial();
                    Panels {
                        main: Rc::new(c[0]),
                        details: c.get(1).copied().map(Rc::new),
                        extra: c.get(2).copied().map(Rc::new),
                        mode: ChildPanelsMode::Triple,
                    }
                },
                PanelsOptions {
                    codec: Some(
                        PanelsCodec::new(
                            |m: &u8, d: Option<&u8>, e: Option<&u8>| {
                                Ok([*m, d.copied().unwrap_or(255), e.copied().unwrap_or(255)]
                                    .to_vec())
                            },
                            |b| {
                                if b.len() != 3 {
                                    return Err("bad panels".into());
                                }
                                Ok((
                                    b[0],
                                    (b[1] != 255).then_some(b[1]),
                                    (b[2] != 255).then_some(b[2]),
                                ))
                            },
                        )
                        .should_save(move |_| s.save())
                        .should_restore(move |_| r.restore()),
                    ),
                    ..Default::default()
                },
                factory,
                move |c, ctx| d.factory(c, ctx),
                move |c, ctx| e.factory(c, ctx),
            )?;
            let snap = v.get();
            let children = std::iter::once(&snap.main)
                .chain(snap.details.iter())
                .chain(snap.extra.iter());
            Snapshot {
                members: children.clone().map(|c| *c.configuration).collect(),
                live: children
                    .map(|c| (c.key.clone(), c.instance.clone()))
                    .collect(),
            }
        }
        Family::Items => {
            let v = child_items_with_initial(
                parent,
                &ItemsNavigation::new(),
                || {
                    let mut items = Items::new(initial()).unwrap();
                    for c in &items.items {
                        items
                            .active_items
                            .insert(c.clone(), ActiveLifecycleState::Resumed);
                    }
                    items
                },
                ItemsOptions {
                    codec: Some(
                        ItemsCodec::new(encode, decode)
                            .should_save(move |_| s.save())
                            .should_restore(move |_| r.restore()),
                    ),
                    ..Default::default()
                },
                factory,
            )?;
            let snap = v.get();
            Snapshot {
                members: snap.items.iter().map(|c| **c).collect(),
                live: snap
                    .items
                    .iter()
                    .filter_map(|c| {
                        snap.active_items
                            .get(c)
                            .map(|a| (a.child.key.clone(), a.child.instance.clone()))
                    })
                    .collect(),
            }
        }
    })
}

#[test]
fn all_families_skip_each_save_before_child_providers_and_resume_saving() {
    for family in FAMILIES {
        let p = fresh();
        let policy = Policy::default();
        build(family, &p, || vec![7, 8, 9], &policy).unwrap();
        policy.save.set(false);
        assert!(p.state_store().save().unwrap().is_empty(), "{family:?}");
        assert_eq!(policy.child_saves.get(), 0, "{family:?}");
        policy.save.set(true);
        assert!(p.state_store().save().unwrap().contains_key(family.key()));
        assert!(policy.child_saves.get() > 0);
        policy.save.set(false);
        assert!(p.state_store().save().unwrap().is_empty());
        assert_eq!(policy.saves.get(), 3);
    }
}

#[test]
fn accepted_restore_is_lazy_and_preserves_nested_state_keys_and_retention() {
    for family in FAMILIES {
        let p = fresh();
        let policy = Policy::default();
        let old = build(family, &p, || vec![7, 8, 9], &policy).unwrap();
        let saved = p.state_store().save().unwrap();
        let instances = p.instance_store();
        p.lifecycle().destroy().unwrap();
        let recreated = root(saved, instances);
        let new = build(
            family,
            &recreated,
            || panic!("accepted restore initialized"),
            &policy,
        )
        .unwrap();
        assert_eq!(old.members, new.members, "{family:?}");
        for ((old_key, old), (new_key, new)) in old.live.iter().zip(&new.live) {
            assert_eq!(old_key, new_key);
            assert_eq!(new.nested, Some(vec![*old.retained]));
            assert!(Rc::ptr_eq(&old.retained, &new.retained));
        }
        assert_eq!(policy.restores.get(), 1);
    }
}

#[test]
fn declined_restore_uses_initializer_and_discards_nested_state_and_retention() {
    for family in FAMILIES {
        let p = fresh();
        let policy = Policy::default();
        let old = build(family, &p, || vec![7, 8, 9], &policy).unwrap();
        let saved = p.state_store().save().unwrap();
        let instances = p.instance_store();
        p.lifecycle().destroy().unwrap();
        policy.restore.set(false);
        let recreated = root(saved, instances);
        let mut initialized = 0;
        let new = build(
            family,
            &recreated,
            || {
                initialized += 1;
                vec![7, 8, 9]
            },
            &policy,
        )
        .unwrap();
        assert_eq!(initialized, 1, "{family:?}");
        for ((_, old), (_, new)) in old.live.iter().zip(&new.live) {
            assert_eq!(new.nested, None);
            assert!(!Rc::ptr_eq(&old.retained, &new.retained));
            assert!(old.context.instance_store().is_destroyed().unwrap());
        }
    }
}

#[test]
fn corrupt_envelopes_never_reach_decline_and_preserve_retry_input() {
    for family in FAMILIES {
        let p = fresh();
        let policy = Policy::default();
        build(family, &p, || vec![7, 8, 9], &policy).unwrap();
        let saved = p.state_store().save().unwrap();
        let bytes = &saved[family.key()];
        policy.restore.set(false);
        for n in 0..bytes.len() {
            let bad = BTreeMap::from([(family.key().to_owned(), bytes[..n].to_vec())]);
            let p = root(bad.clone(), InstanceStore::from(InstanceKeeper::new()));
            assert!(
                build(
                    family,
                    &p,
                    || panic!("corrupt restore initialized"),
                    &policy
                )
                .is_err(),
                "{family:?}, prefix {n}"
            );
            assert_eq!(p.state_store().save().unwrap(), bad);
            assert!(!p.state_store().is_registered(family.key()).unwrap());
        }
        assert_eq!(policy.restores.get(), 0);
    }
}

#[test]
fn declined_factory_panic_rolls_back_consumed_state_for_accepted_retry() {
    for family in FAMILIES {
        let p = fresh();
        let policy = Policy::default();
        let old = build(family, &p, || vec![7, 8, 9], &policy).unwrap();
        let saved = p.state_store().save().unwrap();
        let instances = p.instance_store();
        p.lifecycle().destroy().unwrap();
        let p = root(saved.clone(), instances);
        policy.restore.set(false);
        policy.fail_factory.set(true);
        assert!(catch_unwind(AssertUnwindSafe(|| build(family, &p, || vec![1], &policy))).is_err());
        assert_eq!(p.state_store().save().unwrap(), saved);
        assert!(!p.state_store().is_registered(family.key()).unwrap());
        policy.restore.set(true);
        policy.fail_factory.set(false);
        let retry = build(family, &p, || panic!("retry initialized"), &policy).unwrap();
        assert_eq!(retry.members, old.members);
        assert_eq!(retry.live[0].1.nested, Some(vec![7]));
        assert!(
            Rc::ptr_eq(&retry.live[0].1.retained, &old.live[0].1.retained),
            "{family:?}"
        );
    }
}

#[test]
fn declining_restore_cannot_hide_invalid_decoded_configuration_or_child_key() {
    for family in [Family::Stack, Family::Pages, Family::Items] {
        let p = fresh();
        let policy = Policy::default();
        build(family, &p, || vec![7, 8, 9], &policy).unwrap();
        let mut saved = p.state_store().save().unwrap();
        let bytes = saved.get_mut(family.key()).unwrap();
        let payload_start = if matches!(family, Family::Pages) {
            24
        } else {
            16
        };
        bytes[payload_start..payload_start + 3].copy_from_slice(&[7, 7, 7]);
        policy.restore.set(false);
        let p = root(saved.clone(), InstanceStore::from(InstanceKeeper::new()));
        assert!(matches!(
            build(
                family,
                &p,
                || panic!("invalid membership initialized"),
                &policy
            ),
            Err(decompose_rs::router::NavigationError::DuplicateConfiguration)
        ));
        assert_eq!(policy.restores.get(), 0);
        assert_eq!(p.state_store().save().unwrap(), saved);
    }
    for family in FAMILIES {
        let p = fresh();
        let policy = Policy::default();
        build(family, &p, || vec![7, 8, 9], &policy).unwrap();
        let mut saved = p.state_store().save().unwrap();
        let bytes = saved.get_mut(family.key()).unwrap();
        let key = bytes.windows(7).position(|b| b == b"child/0").unwrap();
        bytes[key + 6] = b'9';
        policy.restore.set(false);
        let p = root(saved.clone(), InstanceStore::from(InstanceKeeper::new()));
        assert!(build(family, &p, || panic!("invalid key initialized"), &policy).is_err());
        assert_eq!(policy.restores.get(), 0);
        assert_eq!(p.state_store().save().unwrap(), saved);
    }
}

#[test]
fn skip_precedes_fallible_configuration_encoder() {
    let p = fresh();
    child_stack(
        &p,
        &StackNavigation::new(),
        vec![1u8],
        StackOptions {
            codec: Some(
                StackCodec::new(|_| Err("encode failed".into()), decode).should_save(|_| false),
            ),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(p.state_store().save().unwrap().is_empty());
}

#[test]
fn restore_codec_error_is_not_a_decline() {
    let p = fresh();
    let policy = Policy::default();
    build(Family::Stack, &p, || vec![1], &policy).unwrap();
    let saved = p.state_store().save().unwrap();
    let p = root(saved.clone(), InstanceStore::from(InstanceKeeper::new()));
    let calls = Rc::new(Cell::new(0));
    let out = calls.clone();
    let result = child_stack_with_initial(
        &p,
        &StackNavigation::new(),
        || panic!("decoder error initialized"),
        StackOptions {
            codec: Some(
                StackCodec::new(encode, |_| Err("decode failed".into())).should_restore(
                    move |_| {
                        out.set(out.get() + 1);
                        false
                    },
                ),
            ),
            ..Default::default()
        },
        |_: &u8, ctx| ctx,
    );
    assert!(
        matches!(result, Err(decompose_rs::router::NavigationError::State(message)) if message == "decode failed")
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(p.state_store().save().unwrap(), saved);
}

#[test]
fn restore_predicate_navigation_is_queued_after_the_lazy_initial_state() {
    let p = fresh();
    let policy = Policy::default();
    build(Family::Stack, &p, || vec![1], &policy).unwrap();
    let saved = p.state_store().save().unwrap();
    let p = root(saved, InstanceStore::from(InstanceKeeper::new()));
    let nav = StackNavigation::new();
    let reentrant = nav.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = seen.clone();
    let value = child_stack_with_initial(
        &p,
        &nav,
        || vec![2],
        StackOptions {
            codec: Some(StackCodec::new(encode, decode).should_restore(move |_| {
                reentrant.push_or_panic(3);
                false
            })),
            ..Default::default()
        },
        move |c, ctx| {
            out.borrow_mut().push(*c);
            ctx
        },
    )
    .unwrap();
    assert_eq!(&*seen.borrow(), &[2, 3]);
    assert_eq!(
        value
            .get()
            .items()
            .map(|c| *c.configuration)
            .collect::<Vec<_>>(),
        [2, 3]
    );
}

#[test]
fn predicates_receive_the_complete_family_metadata_and_clones_keep_policy() {
    let p = fresh();
    let pages = PagesCodec::new(encode, decode).should_save(|pages| {
        assert_eq!(pages.selected_index, Some(1));
        assert_eq!(pages.items.len(), 2);
        false
    });
    child_pages(
        &p,
        &PagesNavigation::new(),
        Pages::new(vec![1, 2], 1).unwrap(),
        PagesOptions {
            codec: Some(pages.clone()),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let items = ItemsCodec::new(encode, decode).should_save(|items| {
        assert_eq!(
            items.active_items.get(&1),
            Some(&ActiveLifecycleState::Started)
        );
        assert_eq!(items.active_items.get(&2), None);
        false
    });
    let mut initial = Items::new(vec![1, 2]).unwrap();
    initial
        .active_items
        .insert(Rc::new(1), ActiveLifecycleState::Started);
    child_items(
        &p,
        &ItemsNavigation::new(),
        initial,
        ItemsOptions {
            codec: Some(items.clone()),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    let panels: PanelsCodec<u8, u8> = PanelsCodec::without_extra(
        |_, _| panic!("skipped panels encoded"),
        |_| panic!("unused decoder"),
    )
    .should_save(|panels| {
        assert_eq!(panels.mode, ChildPanelsMode::Dual);
        assert_eq!(panels.details.as_deref(), Some(&2));
        false
    });
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
            codec: Some(panels.clone()),
            ..Default::default()
        },
        |_, ctx| ctx,
        |_, ctx| ctx,
    )
    .unwrap();
    let slot = SlotCodec::<u8>::new(
        |_| panic!("empty slot encoded"),
        |_| panic!("unused decoder"),
    )
    .should_save(|slot| {
        assert!(slot.is_none());
        false
    });
    child_slot(
        &p,
        &SlotNavigation::new(),
        None,
        SlotOptions {
            codec: Some(slot.clone()),
            ..Default::default()
        },
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(p.state_store().save().unwrap().is_empty());
}

#[test]
fn absent_and_empty_states_have_distinct_initializer_and_restore_paths() {
    for family in FAMILIES {
        let policy = Policy::default();
        let p = fresh();
        let mut calls = 0;
        build(
            family,
            &p,
            || {
                calls += 1;
                vec![7]
            },
            &policy,
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(policy.restores.get(), 0);
    }
    for family in [Family::Slot, Family::Pages, Family::Items] {
        let policy = Policy::default();
        let p = fresh();
        build(family, &p, Vec::new, &policy).unwrap();
        let saved = p.state_store().save().unwrap();
        let p = root(saved.clone(), InstanceStore::from(InstanceKeeper::new()));
        assert!(
            build(family, &p, || panic!("empty restore initialized"), &policy)
                .unwrap()
                .members
                .is_empty()
        );
        policy.restore.set(false);
        let p = root(saved, InstanceStore::from(InstanceKeeper::new()));
        assert_eq!(build(family, &p, || vec![9], &policy).unwrap().members, [9]);
    }
}
