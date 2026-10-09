use decompose_rs::router::items::*;
use decompose_rs::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn root() -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::new(lifecycle)
}
#[test]
fn all_inactive_creates_nothing_until_lazy_fetch() {
    let parent = root();
    let nav = ItemsNavigation::new();
    let count = Rc::new(Cell::new(0));
    let out = count.clone();
    let lazy = child_items(
        &parent,
        &nav,
        Items::new(vec![0u32, 1]).unwrap(),
        ItemsOptions::default(),
        move |_, ctx| {
            out.set(out.get() + 1);
            ctx
        },
    )
    .unwrap();
    assert_eq!(count.get(), 0);
    assert!(lazy.get().active_items.is_empty());
    let first = lazy.get_or_create(&1).unwrap();
    assert_eq!(count.get(), 1);
    assert_eq!(first.lifecycle().state(), LifecycleState::Created);
    assert!(Rc::ptr_eq(&first, &lazy.get_or_create(&1).unwrap()));
    assert_eq!(
        lazy.get().active_items.get(&1).unwrap().lifecycle_state,
        ActiveLifecycleState::Created
    );
}
#[test]
fn three_requested_targets_are_independent_and_parent_bounded() {
    let life = LifecycleRegistry::new();
    let parent = DefaultComponentContext::new(life.clone());
    let nav = ItemsNavigation::new();
    let mut initial = Items::new(vec![0u32, 1, 2, 3]).unwrap();
    for (c, s) in [
        (0, ActiveLifecycleState::Created),
        (1, ActiveLifecycleState::Started),
        (2, ActiveLifecycleState::Resumed),
    ] {
        initial.active_items.insert(Rc::new(c), s);
    }
    let lazy = child_items(&parent, &nav, initial, ItemsOptions::default(), |_, ctx| {
        ctx
    })
    .unwrap();
    let snapshot = lazy.get();
    assert_eq!(snapshot.active_items.len(), 3);
    for child in snapshot.active_items.values() {
        assert_eq!(
            child.child.instance.lifecycle().state(),
            LifecycleState::Initialized
        );
    }
    life.move_to(LifecycleState::Started).unwrap();
    assert_eq!(
        snapshot
            .active_items
            .get(&0)
            .unwrap()
            .child
            .instance
            .lifecycle()
            .state(),
        LifecycleState::Created
    );
    assert_eq!(
        snapshot
            .active_items
            .get(&1)
            .unwrap()
            .child
            .instance
            .lifecycle()
            .state(),
        LifecycleState::Started
    );
    assert_eq!(
        snapshot
            .active_items
            .get(&2)
            .unwrap()
            .child
            .instance
            .lifecycle()
            .state(),
        LifecycleState::Started
    );
    assert_eq!(
        snapshot.active_items.get(&2).unwrap().lifecycle_state,
        ActiveLifecycleState::Resumed
    );
    life.move_to(LifecycleState::Resumed).unwrap();
    assert_eq!(
        snapshot
            .active_items
            .get(&2)
            .unwrap()
            .child
            .instance
            .lifecycle()
            .state(),
        LifecycleState::Resumed
    );
}
#[test]
fn map_supports_eq_only_non_clone_keys_and_replaces_equal_entries() {
    #[derive(PartialEq, Eq)]
    struct Config(u32);
    let first = Rc::new(Config(1));
    let equal = Rc::new(Config(1));
    let mut map = ItemMap::new();
    assert_eq!(map.insert(first, 2), None);
    assert_eq!(map.insert(equal.clone(), 3), Some(2));
    assert_eq!(map.len(), 1);
    assert!(Rc::ptr_eq(map.keys().next().unwrap(), &equal));
    assert_eq!(map.get(&Config(1)), Some(&3));
    let cloned = map.clone();
    assert_eq!(cloned.values().copied().collect::<Vec<_>>(), [3]);
    map.retain(|c, v| c.0 == 1 && *v == 3);
    assert_eq!(map.remove(&Config(1)), Some(3));
    assert!(map.is_empty());
    map = ItemMap::from_owned([(Config(2), 4)]);
    assert!(map.contains_key(&Config(2)));
    map.clear();
    assert!(map.is_empty());
}

fn active(items: Vec<u32>, targets: &[(u32, ActiveLifecycleState)]) -> Items<u32> {
    let mut s = Items::new(items).unwrap();
    s.active_items = ItemMap::from_owned(targets.iter().copied());
    s
}
fn codec() -> ItemsCodec<u32> {
    ItemsCodec::new(
        |items: &[Rc<u32>]| Ok(items.iter().flat_map(|c| c.to_le_bytes()).collect()),
        |bytes| {
            if bytes.len() % 4 != 0 {
                return Err("bad configurations".into());
            }
            Ok(bytes
                .chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .collect())
        },
    )
}
fn persistent() -> ItemsOptions<u32> {
    ItemsOptions {
        codec: Some(codec()),
        ..Default::default()
    }
}
fn restored(
    saved: std::collections::BTreeMap<String, Vec<u8>>,
    instances: Option<SharedInstances>,
) -> DefaultComponentContext {
    let life = LifecycleRegistry::new();
    life.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::with_all(
        life,
        Rc::new(RefCell::new(StateKeeper::with_restored(saved))),
        instances.unwrap_or_else(|| Rc::new(RefCell::new(InstanceKeeper::new()))),
        SharedBack::new(),
    )
}
#[test]
fn dpi1_frozen_fixture_restores_and_saves_exact_bytes() {
    const FIXTURE: &[u8] = &[
        68, 80, 73, 49, 1, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0, 3, 7, 0, 0, 0,
        99, 104, 105, 108, 100, 47, 48, 8, 0, 0, 0, 68, 67, 83, 49, 0, 0, 0, 0,
    ];
    assert_eq!(FIXTURE.len(), 48);
    let parent = restored(
        std::collections::BTreeMap::from([("DefaultChildItems".into(), FIXTURE.to_vec())]),
        None,
    );
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(*lazy.get().items[0], 7);
    let child = lazy.get().active_items.get(&7).unwrap().clone();
    assert_eq!(child.child.key, "child/0");
    assert_eq!(child.lifecycle_state, ActiveLifecycleState::Resumed);
    assert_eq!(
        parent.state_keeper().borrow().save().unwrap()["DefaultChildItems"],
        FIXTURE
    );
}
#[test]
fn persistent_live_and_inactive_state_survives_repeated_saves() {
    let parent = root();
    let nav = ItemsNavigation::new();
    let lazy = child_items(
        &parent,
        &nav,
        active(vec![0, 1], &[(0, ActiveLifecycleState::Resumed)]),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let old = lazy.get_or_create(&0).unwrap();
    old.state_keeper()
        .borrow_mut()
        .register("payload", || vec![4, 5])
        .unwrap();
    let key = lazy.get().active_items.get(&0).unwrap().child.key.clone();
    lazy.set_active_items(|_| ItemMap::new());
    assert!(old.lifecycle().state().is_destroyed());
    let saved = parent.state_keeper().borrow().save().unwrap();
    assert_eq!(saved, parent.state_keeper().borrow().save().unwrap());
    let parent = restored(saved, None);
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![99], &[(99, ActiveLifecycleState::Started)]),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(
        lazy.get().items.iter().map(|c| **c).collect::<Vec<_>>(),
        [0, 1]
    );
    assert!(lazy.get().active_items.is_empty());
    let fresh = lazy.get_or_create(&0).unwrap();
    assert_eq!(
        fresh.state_keeper().borrow_mut().consume_bytes("payload"),
        Some(vec![4, 5])
    );
    assert_eq!(lazy.get().active_items.get(&0).unwrap().child.key, key);
}
#[test]
fn persistent_empty_is_authoritative_over_nonempty_initial() {
    let parent = root();
    child_items(
        &parent,
        &ItemsNavigation::<u32>::new(),
        Items::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    let parent = restored(saved, None);
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![9], &[(9, ActiveLifecycleState::Resumed)]),
        persistent(),
        |_, _| panic!("empty factory"),
    )
    .unwrap();
    assert!(lazy.get().items.is_empty());
}
#[test]
fn reorders_and_status_changes_preserve_live_identity_and_canonical_handles() {
    let parent = root();
    let nav = ItemsNavigation::new();
    let lazy = child_items(
        &parent,
        &nav,
        active(vec![0, 1], &[(0, ActiveLifecycleState::Resumed)]),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let old = lazy.get_or_create(&0).unwrap();
    let key = lazy.get().active_items.get(&0).unwrap().child.key.clone();
    lazy.navigate(|mut s| {
        s.items = vec![Rc::new(1), Rc::new(0)];
        s.active_items = ItemMap::from_owned([
            (0, ActiveLifecycleState::Started),
            (9, ActiveLifecycleState::Resumed),
        ]);
        s
    });
    let snapshot = lazy.get();
    let child = snapshot.active_items.get(&0).unwrap();
    assert!(Rc::ptr_eq(&old, &child.child.instance));
    assert_eq!(child.child.key, key);
    assert_eq!(old.lifecycle().state(), LifecycleState::Started);
    assert_eq!(snapshot.active_items.len(), 1);
    assert!(Rc::ptr_eq(
        &snapshot.items[1],
        snapshot.active_items.keys().next().unwrap()
    ));
    assert!(Rc::ptr_eq(&snapshot.items[1], &child.child.configuration));
}
#[test]
fn deactivate_recreates_keeper_and_component_under_same_key_then_removal_discards_state() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0], &[(0, ActiveLifecycleState::Started)]),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let old = lazy.get_or_create(&0).unwrap();
    let key = lazy.get().active_items.get(&0).unwrap().child.key.clone();
    old.state_keeper()
        .borrow_mut()
        .register("value", || vec![8])
        .unwrap();
    lazy.set_active_items(|_| ItemMap::new());
    assert!(old.instance_keeper().borrow().is_destroyed());
    let fresh = lazy.get_or_create(&0).unwrap();
    assert!(!Rc::ptr_eq(&old, &fresh));
    assert!(!Rc::ptr_eq(
        &old.instance_keeper(),
        &fresh.instance_keeper()
    ));
    assert_eq!(
        fresh.state_keeper().borrow_mut().consume_bytes("value"),
        Some(vec![8])
    );
    assert_eq!(lazy.get().active_items.get(&0).unwrap().child.key, key);
    fresh
        .state_keeper()
        .borrow_mut()
        .register("value", || vec![9])
        .unwrap();
    lazy.set_items(|_| vec![]);
    lazy.set_items(|_| vec![Rc::new(0)]);
    let third = lazy.get_or_create(&0).unwrap();
    assert_ne!(lazy.get().active_items.get(&0).unwrap().child.key, key);
    assert_eq!(
        third.state_keeper().borrow_mut().consume_bytes("value"),
        None
    );
}
#[test]
fn duplicates_are_rejected_before_factories_initially_and_during_navigation() {
    assert!(matches!(
        Items::new(vec![0u32, 0]),
        Err(NavigationError::DuplicateConfiguration)
    ));
    let parent = root();
    let calls = Rc::new(Cell::new(0));
    let out = calls.clone();
    let invalid = Items {
        items: vec![Rc::new(0u32), Rc::new(0)],
        active_items: ItemMap::from_owned([(0, ActiveLifecycleState::Resumed)]),
    };
    assert!(matches!(
        child_items(
            &parent,
            &ItemsNavigation::new(),
            invalid,
            ItemsOptions::default(),
            move |_, ctx| {
                out.set(out.get() + 1);
                ctx
            }
        ),
        Err(NavigationError::DuplicateConfiguration)
    ));
    assert_eq!(calls.get(), 0);
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![0u32]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let error = Rc::new(RefCell::new(None));
    let out = error.clone();
    lazy.set_items_with(
        |mut v| {
            v.push(Rc::new(0));
            v
        },
        move |r| *out.borrow_mut() = r.err(),
    );
    assert_eq!(
        *error.borrow(),
        Some(NavigationError::DuplicateConfiguration)
    );
    assert_eq!(lazy.get().items.len(), 1);
}
#[test]
fn initial_active_map_is_pruned_and_canonicalized() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(
            vec![0],
            &[
                (0, ActiveLifecycleState::Created),
                (1, ActiveLifecycleState::Resumed),
            ],
        ),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let snapshot = lazy.get();
    assert_eq!(snapshot.active_items.len(), 1);
    assert!(Rc::ptr_eq(
        &snapshot.items[0],
        snapshot.active_items.keys().next().unwrap()
    ));
}
fn via_trait<N: ItemsNavigator<u32>>(n: &N) {
    n.set_active_items(|mut m| {
        m.insert(Rc::new(0), ActiveLifecycleState::Resumed);
        m
    });
}
#[test]
fn shared_completions_use_each_receivers_input_local_helpers_are_isolated() {
    let first = root();
    let second = root();
    let nav = ItemsNavigation::new();
    let a = child_items(
        &first,
        &nav,
        Items::new(vec![0u32]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let b = child_items(
        &second,
        &nav,
        Items::new(vec![0u32, 1]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    via_trait(&nav);
    assert_eq!(a.get().active_items.len(), 1);
    assert_eq!(b.get().active_items.len(), 1);
    let seen = Rc::new(RefCell::new(vec![]));
    let out = seen.clone();
    nav.set_items_with(
        |mut v| {
            v.push(Rc::new(2));
            v
        },
        move |r| {
            let r = r.unwrap().clone();
            out.borrow_mut()
                .push((r.old_items.items.len(), r.new_items.items.len()));
        },
    );
    assert_eq!(*seen.borrow(), [(1, 2), (2, 3)]);
    a.set_active_items(|_| ItemMap::new());
    assert!(a.get().active_items.is_empty());
    assert_eq!(b.get().active_items.len(), 1);
    via_trait(&a);
    assert_eq!(a.get().active_items.len(), 1);
}
#[test]
fn noop_publishes_and_completes_after_observer() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::<u32>::new(),
        Items::default(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    let out = log.clone();
    let _sub = lazy.subscribe(move |_| out.borrow_mut().push("observer"));
    log.borrow_mut().clear();
    let out = log.clone();
    lazy.navigate_with(
        |s| s,
        move |r| {
            assert!(r.is_ok());
            out.borrow_mut().push("completion")
        },
    );
    assert_eq!(*log.borrow(), ["observer", "completion"]);
}
#[test]
fn dropping_source_does_not_disable_local_navigation_and_dead_fetch_is_destroyed() {
    let parent = root();
    let nav = ItemsNavigation::new();
    let lazy = child_items(
        &parent,
        &nav,
        Items::new(vec![0u32]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    drop(nav);
    let active = lazy.get_or_create(&0).unwrap();
    assert_eq!(active.lifecycle().state(), LifecycleState::Created);
    assert!(matches!(
        lazy.get_or_create(&9),
        Err(NavigationError::UnknownConfiguration)
    ));
    let snapshot = lazy.get();
    parent.lifecycle().destroy().unwrap();
    assert!(matches!(
        lazy.get_or_create(&0),
        Err(NavigationError::Destroyed)
    ));
    assert!(
        snapshot
            .active_items
            .get(&0)
            .unwrap()
            .child
            .instance
            .lifecycle()
            .state()
            .is_destroyed()
    );
    let error = Rc::new(RefCell::new(None));
    let out = error.clone();
    lazy.navigate_with(|s| s, move |r| *out.borrow_mut() = r.err());
    assert_eq!(*error.borrow(), Some(NavigationError::Destroyed));
}
#[test]
fn inactive_recursive_fetch_is_busy_without_activation_active_fetch_is_allowed() {
    let parent = root();
    let holder = Rc::new(RefCell::new(
        None::<LazyChildItems<u32, DefaultComponentContext>>,
    ));
    let access = holder.clone();
    let seen = Rc::new(RefCell::new(vec![]));
    let out = seen.clone();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0, 1, 2], &[(0, ActiveLifecycleState::Resumed)]),
        ItemsOptions::default(),
        move |c, ctx| {
            if *c == 1 {
                let binding = access.borrow();
                let lazy = binding.as_ref().unwrap();
                assert_eq!(
                    lazy.get_or_create(&0).unwrap().lifecycle().state(),
                    LifecycleState::Resumed
                );
                out.borrow_mut().push(lazy.get_or_create(&2).err());
            }
            ctx
        },
    )
    .unwrap();
    *holder.borrow_mut() = Some(lazy.clone());
    lazy.get_or_create(&1).unwrap();
    assert_eq!(
        *seen.borrow(),
        [Some(NavigationError::NavigationInProgress)]
    );
    assert!(!lazy.get().active_items.contains_key(&2));
    holder.borrow_mut().take();
}
#[test]
fn lazy_fetch_reports_unavailable_after_queued_factory_self_removal_or_deactivation() {
    for remove in [false, true] {
        let parent = root();
        let nav = ItemsNavigation::new();
        let source = nav.clone();
        let lazy = child_items(
            &parent,
            &nav,
            Items::new(vec![0u32]).unwrap(),
            ItemsOptions::default(),
            move |_, ctx| {
                if remove {
                    source.set_items(|_| vec![]);
                } else {
                    source.set_active_items(|_| ItemMap::new());
                }
                ctx
            },
        )
        .unwrap();
        assert!(matches!(
            lazy.get_or_create(&0),
            Err(NavigationError::ChildUnavailable)
        ));
        assert!(lazy.get().active_items.is_empty());
    }
}

const ONE_FRAME: &[u8] = &[
    68, 80, 73, 49, 1, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0, 3, 7, 0, 0, 0, 99,
    104, 105, 108, 100, 47, 48, 8, 0, 0, 0, 68, 67, 83, 49, 0, 0, 0, 0,
];
fn reject_before_codec(bytes: Vec<u8>) {
    let original = std::collections::BTreeMap::from([("DefaultChildItems".into(), bytes)]);
    let parent = restored(original.clone(), None);
    let calls = Rc::new(Cell::new(0));
    let out = calls.clone();
    let options = ItemsOptions {
        codec: Some(ItemsCodec::new(
            |_| Ok(vec![]),
            move |_| {
                out.set(out.get() + 1);
                Ok(vec![7u32])
            },
        )),
        ..Default::default()
    };
    assert!(
        child_items(
            &parent,
            &ItemsNavigation::new(),
            Items::default(),
            options,
            |_, _| panic!("invalid frame factory")
        )
        .is_err()
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(parent.state_keeper().borrow().save().unwrap(), original);
    assert_eq!(parent.back_handler().handler_count(), 0);
}
#[test]
fn every_truncated_dpi1_prefix_rejects_before_codec_and_preserves_input() {
    for length in 0..ONE_FRAME.len() {
        reject_before_codec(ONE_FRAME[..length].to_vec());
    }
}
#[test]
fn malformed_dpi1_magic_status_lengths_count_counter_utf8_nested_and_trailing_precede_codec() {
    let mut variants = vec![];
    for (index, value) in [(0, 0), (24, 4), (29, 255), (40, 0)] {
        let mut b = ONE_FRAME.to_vec();
        b[index] = value;
        variants.push(b);
    }
    for range in [12..16, 20..24, 25..29, 36..40, 44..48] {
        let mut b = ONE_FRAME.to_vec();
        b[range].copy_from_slice(&u32::MAX.to_le_bytes());
        variants.push(b);
    }
    let mut b = ONE_FRAME.to_vec();
    b[4..12].copy_from_slice(&0u64.to_le_bytes());
    variants.push(b);
    let mut b = ONE_FRAME.to_vec();
    b.push(0);
    variants.push(b);
    // Noncanonical numeric key, including an equal parsed counter with leading zero.
    let mut b = ONE_FRAME.to_vec();
    b[25..29].copy_from_slice(&8u32.to_le_bytes());
    b.insert(35, b'0');
    variants.push(b);
    let mut b = ONE_FRAME.to_vec();
    b[35] = b'1';
    variants.push(b);
    // Two children with equal identities pass framing lengths but must reject uniqueness.
    let mut b = ONE_FRAME.to_vec();
    b[20..24].copy_from_slice(&2u32.to_le_bytes());
    b.extend_from_slice(&ONE_FRAME[24..]);
    variants.push(b);
    // Valid outer length containing a DCS1 trailing byte.
    let mut b = ONE_FRAME.to_vec();
    b[36..40].copy_from_slice(&9u32.to_le_bytes());
    b.push(0);
    variants.push(b);
    for bytes in variants {
        reject_before_codec(bytes);
    }
}
#[test]
fn all_four_saved_target_bytes_restore_literal_states() {
    for (byte, expected) in [
        (0, None),
        (1, Some(ActiveLifecycleState::Created)),
        (2, Some(ActiveLifecycleState::Started)),
        (3, Some(ActiveLifecycleState::Resumed)),
    ] {
        let mut b = ONE_FRAME.to_vec();
        b[24] = byte;
        let parent = restored(
            std::collections::BTreeMap::from([("DefaultChildItems".into(), b.clone())]),
            None,
        );
        let lazy = child_items(
            &parent,
            &ItemsNavigation::new(),
            Items::default(),
            persistent(),
            |_, ctx| ctx,
        )
        .unwrap();
        assert_eq!(
            lazy.get().active_items.get(&7).map(|c| c.lifecycle_state),
            expected
        );
        assert_eq!(
            parent.state_keeper().borrow().save().unwrap()["DefaultChildItems"],
            b
        );
    }
}
#[test]
fn decode_error_count_mismatch_and_decoded_duplicates_reject_before_factory_with_retry() {
    for kind in 0..3 {
        let bytes = if kind == 2 {
            let mut b = ONE_FRAME.to_vec();
            b[4..12].copy_from_slice(&2u64.to_le_bytes());
            b[20..24].copy_from_slice(&2u32.to_le_bytes());
            let mut second = ONE_FRAME[24..].to_vec();
            second[11] = b'1';
            b.extend(second);
            b
        } else {
            ONE_FRAME.to_vec()
        };
        let saved = std::collections::BTreeMap::from([("DefaultChildItems".into(), bytes)]);
        let parent = restored(saved.clone(), None);
        let options = ItemsOptions {
            codec: Some(ItemsCodec::new(
                |_| Ok(vec![]),
                move |_| match kind {
                    0 => Err("codec error".into()),
                    1 => Ok(vec![]),
                    _ => Ok(vec![7u32, 7]),
                },
            )),
            ..Default::default()
        };
        assert!(
            child_items(
                &parent,
                &ItemsNavigation::new(),
                Items::default(),
                options,
                |_, _| panic!("decoder failure factory")
            )
            .is_err()
        );
        assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
        if kind != 2 {
            child_items(
                &parent,
                &ItemsNavigation::new(),
                Items::default(),
                persistent(),
                |_, ctx| ctx,
            )
            .unwrap();
        }
    }
}
#[test]
fn encoding_failure_surfaces_without_replacing_live_membership() {
    let parent = root();
    let options = ItemsOptions {
        codec: Some(ItemsCodec::new(
            |_: &[Rc<u32>]| Err("encoder exploded".into()),
            |_| Ok(vec![]),
        )),
        ..Default::default()
    };
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0], &[(0, ActiveLifecycleState::Started)]),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(
        parent
            .state_keeper()
            .borrow()
            .save()
            .unwrap_err()
            .to_string()
            .contains("encoder exploded")
    );
    assert_eq!(
        lazy.get()
            .active_items
            .get(&0)
            .unwrap()
            .child
            .instance
            .lifecycle()
            .state(),
        LifecycleState::Started
    );
}
#[test]
fn key_reservation_precedes_codec_and_factory_for_mixed_router_construction() {
    use decompose_rs::router::slot::*;
    let saved =
        std::collections::BTreeMap::from([("DefaultChildItems".into(), ONE_FRAME.to_vec())]);
    let parent = restored(saved, None);
    let context = parent.clone();
    let calls = Rc::new(Cell::new(0));
    let out = calls.clone();
    let options = ItemsOptions {
        codec: Some(ItemsCodec::new(
            |_| Ok(vec![]),
            move |_| {
                assert!(matches!(
                    child_slot(
                        &context,
                        &SlotNavigation::new(),
                        Some(1u32),
                        SlotOptions {
                            key: "DefaultChildItems".into(),
                            ..Default::default()
                        },
                        |_, _| panic!("reservation factory")
                    ),
                    Err(NavigationError::Context(
                        DecomposeError::DuplicateKey { .. }
                    ))
                ));
                out.set(out.get() + 1);
                Ok(vec![7])
            },
        )),
        ..Default::default()
    };
    child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::default(),
        options,
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(calls.get(), 1);
    let parent = root();
    let context = parent.clone();
    child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0], &[(0, ActiveLifecycleState::Created)]),
        ItemsOptions::default(),
        move |_, ctx| {
            assert!(matches!(
                child_slot(
                    &context,
                    &SlotNavigation::new(),
                    Some(1u32),
                    SlotOptions {
                        key: "DefaultChildItems".into(),
                        ..Default::default()
                    },
                    |_, _| panic!("reservation factory")
                ),
                Err(NavigationError::Context(
                    DecomposeError::DuplicateKey { .. }
                ))
            ));
            ctx
        },
    )
    .unwrap();
}
#[test]
fn keeper_type_conflict_rolls_back_original_input() {
    let saved =
        std::collections::BTreeMap::from([("DefaultChildItems".into(), ONE_FRAME.to_vec())]);
    let parent = restored(saved.clone(), None);
    parent
        .instance_keeper()
        .borrow_mut()
        .get_or_create("DefaultChildItems", || Rc::new(9u32))
        .unwrap();
    assert!(matches!(
        child_items(
            &parent,
            &ItemsNavigation::new(),
            Items::default(),
            persistent(),
            |_, _| panic!("type conflict factory")
        ),
        Err(NavigationError::Context(_))
    ));
    assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
}
#[test]
fn transient_saves_skip_child_providers_and_reset_retained_instances() {
    let instances = Rc::new(RefCell::new(InstanceKeeper::new()));
    let parent = restored(Default::default(), Some(instances.clone()));
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0], &[(0, ActiveLifecycleState::Resumed)]),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let old = lazy.get_or_create(&0).unwrap();
    old.instance_keeper()
        .borrow_mut()
        .get_or_create("service", || Rc::new(7u32))
        .unwrap();
    old.state_keeper()
        .borrow_mut()
        .register("forbidden", || panic!("transient saved child"))
        .unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    assert!(saved.is_empty());
    parent.lifecycle().destroy().unwrap();
    let parent = restored(saved, Some(instances));
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![0u32]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(lazy.get().active_items.is_empty());
    assert!(old.instance_keeper().borrow().is_destroyed());
    assert!(
        !lazy
            .get_or_create(&0)
            .unwrap()
            .instance_keeper()
            .borrow()
            .contains("service")
    );
}
#[test]
fn external_retention_survives_parent_recreation_only_for_live_restored_entries() {
    let instances = Rc::new(RefCell::new(InstanceKeeper::new()));
    let parent = restored(Default::default(), Some(instances.clone()));
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(
            vec![0, 1],
            &[
                (0, ActiveLifecycleState::Resumed),
                (1, ActiveLifecycleState::Created),
            ],
        ),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let old = lazy.get_or_create(&0).unwrap();
    let service = old
        .instance_keeper()
        .borrow_mut()
        .get_or_create("service", || Rc::new(7u32))
        .unwrap();
    lazy.set_active_items(|mut m| {
        m.remove(&1);
        m
    });
    let saved = parent.state_keeper().borrow().save().unwrap();
    parent.lifecycle().destroy().unwrap();
    assert!(!old.instance_keeper().borrow().is_destroyed());
    let parent = restored(saved, Some(instances));
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let fresh = lazy.get_or_create(&0).unwrap();
    let retained = fresh
        .instance_keeper()
        .borrow_mut()
        .get_or_create("service", || Rc::new(99u32))
        .unwrap();
    assert!(Rc::ptr_eq(&service, &retained));
    assert!(!Rc::ptr_eq(&old, &fresh));
    assert!(old.lifecycle().state().is_destroyed());
}
#[test]
fn coherent_save_freezes_targets_membership_keys_and_counter_before_provider_navigation() {
    let parent = root();
    let nav = ItemsNavigation::new();
    let lazy = child_items(
        &parent,
        &nav,
        active(vec![0, 1], &[(0, ActiveLifecycleState::Started)]),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let source = nav.clone();
    let once = Rc::new(Cell::new(true));
    let flag = once.clone();
    lazy.get_or_create(&0)
        .unwrap()
        .state_keeper()
        .borrow_mut()
        .register("navigate", move || {
            if flag.replace(false) {
                source.set_items(|_| vec![]);
            }
            vec![3]
        })
        .unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    assert!(lazy.get().items.is_empty());
    let parent = restored(saved, None);
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert_eq!(
        lazy.get().items.iter().map(|c| **c).collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(
        lazy.get().active_items.get(&0).unwrap().lifecycle_state,
        ActiveLifecycleState::Started
    );
    assert_eq!(
        lazy.get().active_items.get(&0).unwrap().child.key,
        "child/0"
    );
    lazy.set_items(|mut v| {
        v.push(Rc::new(2));
        v
    });
    lazy.get_or_create(&2).unwrap();
    assert_eq!(
        lazy.get().active_items.get(&2).unwrap().child.key,
        "child/2"
    );
}
#[test]
fn exhausted_counter_returns_error_without_factory_and_preserves_empty_state() {
    let mut frame = b"DPI1".to_vec();
    frame.extend(u64::MAX.to_le_bytes());
    frame.extend(0u32.to_le_bytes());
    frame.extend(0u32.to_le_bytes());
    let parent = restored(
        std::collections::BTreeMap::from([("DefaultChildItems".into(), frame)]),
        None,
    );
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::default(),
        persistent(),
        |_, _| panic!("counter exhausted factory"),
    )
    .unwrap();
    let error = Rc::new(RefCell::new(None));
    let out = error.clone();
    lazy.set_items_with(|_| vec![Rc::new(0)], move |r| *out.borrow_mut() = r.err());
    assert!(matches!(*error.borrow(), Some(NavigationError::State(_))));
    assert!(lazy.get().items.is_empty());
}
#[test]
fn map_retain_mutates_values_and_preserves_insertion_order() {
    let mut map = ItemMap::from_owned([(1, 2), (2, 3), (3, 4)]);
    map.retain(|k, v| {
        *v += 1;
        *k != 2
    });
    assert_eq!(
        map.iter().map(|(c, v)| (**c, *v)).collect::<Vec<_>>(),
        [(1, 3), (3, 5)]
    );
    let map: ItemMap<_, _> = [(Rc::new(1), 3), (Rc::new(1), 4), (Rc::new(2), 5)]
        .into_iter()
        .collect();
    assert_eq!(map.len(), 2);
    assert_eq!(map.get(&1), Some(&4));
}

#[test]
fn back_is_gated_by_requested_and_parent_states_and_never_changes_membership() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(
            vec![0, 1, 2, 3],
            &[
                (0, ActiveLifecycleState::Created),
                (1, ActiveLifecycleState::Started),
                (2, ActiveLifecycleState::Resumed),
            ],
        ),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    for c in [0, 1, 2] {
        let out = log.clone();
        lazy.get_or_create(&c)
            .unwrap()
            .back_handler()
            .register(0, true, move |_, _| out.borrow_mut().push(c))
            .unwrap();
    }
    assert!(parent.back_handler().back());
    assert_eq!(*log.borrow(), [2]);
    lazy.set_active_items(|mut m| {
        m.remove(&2);
        m
    });
    assert!(parent.back_handler().back());
    assert_eq!(*log.borrow(), [2, 1]);
    parent.lifecycle().move_to(LifecycleState::Created).unwrap();
    assert!(!parent.back_handler().back());
    parent.lifecycle().move_to(LifecycleState::Started).unwrap();
    assert!(parent.back_handler().back());
    assert_eq!(*log.borrow(), [2, 1, 1]);
    lazy.set_active_items(|mut m| {
        m.insert(Rc::new(1), ActiveLifecycleState::Created);
        m
    });
    assert!(!parent.back_handler().back());
    assert_eq!(lazy.get().items.len(), 4);
}
#[test]
fn back_priority_lifo_and_predictive_cancellation_preserve_membership() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(
            vec![0, 1],
            &[
                (0, ActiveLifecycleState::Started),
                (1, ActiveLifecycleState::Resumed),
            ],
        ),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let log = Rc::new(RefCell::new(vec![]));
    for c in [0, 1] {
        let out = log.clone();
        lazy.get_or_create(&c)
            .unwrap()
            .back_handler()
            .register(0, true, move |event, _| {
                out.borrow_mut().push((c, event.phase))
            })
            .unwrap();
    }
    assert!(parent.back_handler().predictive_start());
    parent.back_handler().predictive_progress(0.5).unwrap();
    lazy.set_active_items(|mut m| {
        m.remove(&1);
        m
    });
    assert_eq!(
        *log.borrow(),
        [
            (1, BackPhase::Started),
            (1, BackPhase::Progressed),
            (1, BackPhase::Cancelled)
        ]
    );
    assert_eq!(lazy.get().items.len(), 2);
    assert!(parent.back_handler().back());
    assert_eq!(log.borrow().last(), Some(&(0, BackPhase::Invoked)));
    let out = log.clone();
    let handle = parent
        .back_handler()
        .register(2, true, move |event, _| {
            out.borrow_mut().push((9, event.phase))
        })
        .unwrap();
    assert!(parent.back_handler().back());
    assert_eq!(log.borrow().last(), Some(&(9, BackPhase::Invoked)));
    parent.back_handler().unregister(handle.id());
    lazy.set_active_items(|_| ItemMap::new());
    assert!(!parent.back_handler().back());
}
#[test]
fn local_back_navigation_is_deferred_and_inactive_fetch_is_busy_without_side_effect() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0, 1], &[(0, ActiveLifecycleState::Started)]),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let source = lazy.clone();
    let done = Rc::new(Cell::new(false));
    let output = done.clone();
    lazy.get_or_create(&0)
        .unwrap()
        .back_handler()
        .register(0, true, move |_, _| {
            assert!(matches!(
                source.get_or_create(&1),
                Err(NavigationError::NavigationInProgress)
            ));
            assert!(!source.get().active_items.contains_key(&1));
            let out = output.clone();
            source.set_active_items_with(
                |mut m| {
                    m.insert(Rc::new(1), ActiveLifecycleState::Resumed);
                    m
                },
                move |r| {
                    assert!(r.is_ok());
                    out.set(true)
                },
            );
            assert!(!output.get());
            assert!(!source.get().active_items.contains_key(&1));
        })
        .unwrap();
    assert!(parent.back_handler().back());
    assert!(done.get());
    assert_eq!(
        lazy.get().active_items.get(&1).unwrap().lifecycle_state,
        ActiveLifecycleState::Resumed
    );
}
#[test]
fn panicking_back_callback_discards_deferred_local_work_and_allows_retry() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0, 1], &[(0, ActiveLifecycleState::Started)]),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let source = lazy.clone();
    let handle = lazy
        .get_or_create(&0)
        .unwrap()
        .back_handler()
        .register(0, true, move |_, _| {
            source.set_items(|_| vec![]);
            panic!("Back panic")
        })
        .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| parent
            .back_handler()
            .back()))
        .is_err()
    );
    assert_eq!(lazy.get().items.len(), 2);
    lazy.get_or_create(&0)
        .unwrap()
        .back_handler()
        .unregister(handle.id());
    lazy.get_or_create(&1).unwrap();
    assert_eq!(lazy.get().active_items.len(), 2);
}
#[test]
fn local_factory_lifecycle_observer_completion_navigation_is_fifo() {
    let parent = root();
    let holder = Rc::new(RefCell::new(
        None::<LazyChildItems<u32, DefaultComponentContext>>,
    ));
    let access = holder.clone();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![0u32, 1, 2, 3, 4]).unwrap(),
        ItemsOptions::default(),
        move |c, ctx| {
            if *c == 0 {
                access.borrow().as_ref().unwrap().set_active_items(|mut m| {
                    m.insert(Rc::new(1), ActiveLifecycleState::Created);
                    m
                });
                let access = access.clone();
                let _ = ctx
                    .lifecycle()
                    .do_on_resume(move || {
                        access.borrow().as_ref().unwrap().set_active_items(|mut m| {
                            m.insert(Rc::new(2), ActiveLifecycleState::Created);
                            m
                        })
                    })
                    .detach();
            }
            ctx
        },
    )
    .unwrap();
    *holder.borrow_mut() = Some(lazy.clone());
    let log = Rc::new(RefCell::new(vec![]));
    let out = log.clone();
    let source = lazy.clone();
    let once = Rc::new(Cell::new(true));
    let flag = once.clone();
    let token = lazy.subscribe(move |s| {
        out.borrow_mut().push(s.active_items.len());
        if s.active_items.contains_key(&0) && flag.replace(false) {
            source.set_active_items(|mut m| {
                m.insert(Rc::new(3), ActiveLifecycleState::Created);
                m
            });
        }
    });
    log.borrow_mut().clear();
    let source = lazy.clone();
    lazy.set_active_items_with(
        |mut m| {
            m.insert(Rc::new(0), ActiveLifecycleState::Resumed);
            m
        },
        move |r| {
            assert_eq!(r.unwrap().new_items.active_items.len(), 1);
            source.set_active_items(|mut m| {
                m.insert(Rc::new(4), ActiveLifecycleState::Created);
                m
            });
        },
    );
    assert_eq!(*log.borrow(), [1, 2, 3, 4, 5]);
    holder.borrow_mut().take();
    drop(token);
}
#[test]
fn lazy_fetch_is_unavailable_after_observer_or_lifecycle_self_deactivation() {
    for lifecycle in [false, true] {
        let parent = root();
        let nav = ItemsNavigation::new();
        let source = nav.clone();
        let lazy = child_items(
            &parent,
            &nav,
            Items::new(vec![0u32]).unwrap(),
            ItemsOptions::default(),
            move |_, ctx| {
                if lifecycle {
                    let source = source.clone();
                    let _ = ctx
                        .lifecycle()
                        .do_on_create(move || source.set_active_items(|_| ItemMap::new()))
                        .detach();
                }
                ctx
            },
        )
        .unwrap();
        let source = nav.clone();
        let _token = lazy.subscribe(move |s| {
            if !lifecycle && s.active_items.contains_key(&0) {
                source.set_active_items(|_| ItemMap::new());
            }
        });
        assert!(matches!(
            lazy.get_or_create(&0),
            Err(NavigationError::ChildUnavailable)
        ));
    }
}
#[test]
fn local_observer_and_completion_panics_discard_nested_work_and_recover() {
    for observer in [false, true] {
        let parent = root();
        let lazy = child_items(
            &parent,
            &ItemsNavigation::new(),
            Items::new(vec![0u32, 1]).unwrap(),
            ItemsOptions::default(),
            |_, ctx| ctx,
        )
        .unwrap();
        let flag = Rc::new(Cell::new(true));
        let once = flag.clone();
        let source = lazy.clone();
        let token = lazy.subscribe(move |s| {
            if observer && s.active_items.contains_key(&0) && once.replace(false) {
                source.set_items(|_| vec![]);
                panic!("observer panic");
            }
        });
        let source = lazy.clone();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lazy.set_active_items_with(
                |mut m| {
                    m.insert(Rc::new(0), ActiveLifecycleState::Resumed);
                    m
                },
                move |_| {
                    if !observer {
                        source.set_items(|_| vec![]);
                        panic!("completion panic");
                    }
                }
            )))
            .is_err()
        );
        assert_eq!(lazy.get().items.len(), 2);
        assert_eq!(lazy.get().active_items.len(), 1);
        lazy.get_or_create(&1).unwrap();
        assert_eq!(lazy.get().active_items.len(), 2);
        drop(token);
    }
}
#[test]
fn local_transformer_panic_drops_queued_work_and_releases_processing_gate() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![0u32]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let source = lazy.clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lazy.navigate(move |_| {
            source.set_items(|_| vec![]);
            panic!("transformer panic")
        })))
        .is_err()
    );
    assert_eq!(lazy.get().items.len(), 1);
    lazy.get_or_create(&0).unwrap();
    assert_eq!(lazy.get().active_items.len(), 1);
}
#[test]
fn local_factory_panic_releases_prepared_scopes_and_allows_lazy_retry() {
    let parent = root();
    let flag = Rc::new(Cell::new(true));
    let once = flag.clone();
    let capture = Rc::new(RefCell::new(vec![]));
    let out = capture.clone();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![0u32, 1]).unwrap(),
        ItemsOptions::default(),
        move |c, ctx| {
            out.borrow_mut()
                .push((ctx.lifecycle(), ctx.instance_keeper()));
            if *c == 1 && once.replace(false) {
                panic!("factory panic")
            }
            ctx
        },
    )
    .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lazy.set_active_items(
            |_| ItemMap::from_owned([
                (0, ActiveLifecycleState::Created),
                (1, ActiveLifecycleState::Resumed)
            ])
        )))
        .is_err()
    );
    for (life, keeper) in capture.borrow().iter() {
        assert!(life.state().is_destroyed());
        assert!(keeper.borrow().is_destroyed());
    }
    assert!(lazy.get().active_items.is_empty());
    assert_eq!(
        lazy.get_or_create(&1).unwrap().lifecycle().state(),
        LifecycleState::Created
    );
}
#[test]
fn initial_factory_panic_restores_original_input_and_allows_constructor_retry() {
    let saved =
        std::collections::BTreeMap::from([("DefaultChildItems".into(), ONE_FRAME.to_vec())]);
    let parent = restored(saved.clone(), None);
    let captured = Rc::new(RefCell::new(None));
    let out = captured.clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| child_items(
            &parent,
            &ItemsNavigation::new(),
            Items::default(),
            persistent(),
            move |_, ctx| {
                *out.borrow_mut() = Some((ctx.lifecycle(), ctx.instance_keeper()));
                panic!("initial factory")
            }
        )))
        .is_err()
    );
    let binding = captured.borrow();
    let (life, keeper) = binding.as_ref().unwrap();
    assert!(life.state().is_destroyed());
    assert!(keeper.borrow().is_destroyed());
    assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
    assert_eq!(parent.back_handler().handler_count(), 0);
    child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
}
#[test]
fn failed_initial_queued_navigation_rolls_back_original_input() {
    let saved =
        std::collections::BTreeMap::from([("DefaultChildItems".into(), ONE_FRAME.to_vec())]);
    let parent = restored(saved.clone(), None);
    let nav = ItemsNavigation::new();
    let source = nav.clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| child_items(
            &parent,
            &nav,
            Items::default(),
            persistent(),
            move |_, ctx| {
                source.set_items(|v| vec![v[0].clone(), v[0].clone()]);
                ctx
            }
        )))
        .is_err()
    );
    assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
    assert_eq!(parent.back_handler().handler_count(), 0);
    child_items(&parent, &nav, Items::default(), persistent(), |_, ctx| ctx).unwrap();
}
#[test]
fn parent_destroy_during_factory_or_initial_resume_is_typed_and_retryable_input() {
    for resume in [false, true] {
        let saved =
            std::collections::BTreeMap::from([("DefaultChildItems".into(), ONE_FRAME.to_vec())]);
        let parent = restored(saved.clone(), None);
        let life = parent.lifecycle();
        assert!(matches!(
            child_items(
                &parent,
                &ItemsNavigation::new(),
                Items::default(),
                persistent(),
                move |_, ctx| {
                    if resume {
                        let life = life.clone();
                        let _ = ctx
                            .lifecycle()
                            .do_on_resume(move || life.destroy().unwrap())
                            .detach();
                    } else {
                        life.destroy().unwrap();
                    }
                    ctx
                }
            ),
            Err(NavigationError::Destroyed)
        ));
        assert_eq!(parent.state_keeper().borrow().save().unwrap(), saved);
        assert_eq!(parent.back_handler().handler_count(), 0);
    }
}
#[test]
fn destroyed_parent_rejects_construction_without_factory() {
    let parent = root();
    parent.lifecycle().destroy().unwrap();
    assert!(matches!(
        child_items(
            &parent,
            &ItemsNavigation::new(),
            Items::new(vec![0u32]).unwrap(),
            ItemsOptions::default(),
            |_, _| panic!("destroyed factory")
        ),
        Err(NavigationError::Destroyed)
    ));
}
#[test]
fn forward_factories_reverse_destruction_and_demotion_before_promotion() {
    let parent = root();
    let log = Rc::new(RefCell::new(vec![]));
    let out = log.clone();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![0u32, 1, 2]).unwrap(),
        ItemsOptions::default(),
        move |c, ctx| {
            let c = *c;
            out.borrow_mut().push((c, "factory"));
            let out = out.clone();
            let _ = ctx
                .lifecycle()
                .subscribe(move |s| match s {
                    LifecycleState::Resumed => out.borrow_mut().push((c, "resume")),
                    LifecycleState::Created => out.borrow_mut().push((c, "created")),
                    LifecycleState::Destroyed => out.borrow_mut().push((c, "destroy")),
                    _ => {}
                })
                .detach();
            ctx
        },
    )
    .unwrap();
    lazy.set_active_items(|_| {
        ItemMap::from_owned([
            (2, ActiveLifecycleState::Created),
            (0, ActiveLifecycleState::Resumed),
            (1, ActiveLifecycleState::Created),
        ])
    });
    assert_eq!(
        log.borrow()
            .iter()
            .filter(|(_, s)| *s == "factory")
            .copied()
            .collect::<Vec<_>>(),
        [(0, "factory"), (1, "factory"), (2, "factory")]
    );
    log.borrow_mut().clear();
    lazy.set_active_items(|mut m| {
        m.insert(Rc::new(0), ActiveLifecycleState::Created);
        m.insert(Rc::new(2), ActiveLifecycleState::Resumed);
        m
    });
    assert!(
        log.borrow()
            .iter()
            .position(|e| *e == (0, "created"))
            .unwrap()
            < log
                .borrow()
                .iter()
                .position(|e| *e == (2, "resume"))
                .unwrap()
    );
    log.borrow_mut().clear();
    lazy.set_items(|_| vec![]);
    assert_eq!(
        log.borrow()
            .iter()
            .filter(|(_, s)| *s == "destroy")
            .copied()
            .collect::<Vec<_>>(),
        [(2, "destroy"), (1, "destroy"), (0, "destroy")]
    );
}
#[test]
fn lifecycle_panic_never_reuses_ended_live_scope_and_local_retry_recovers() {
    for outgoing in [false, true] {
        let parent = root();
        let flag = Rc::new(Cell::new(true));
        let once = flag.clone();
        let lazy = child_items(
            &parent,
            &ItemsNavigation::new(),
            active(vec![0, 1], &[(0, ActiveLifecycleState::Resumed)]),
            ItemsOptions::default(),
            move |c, ctx| {
                let once = once.clone();
                if *c == 0 && outgoing {
                    let _ = ctx
                        .lifecycle()
                        .do_on_destroy(move || {
                            if once.replace(false) {
                                panic!("destroy panic")
                            }
                        })
                        .detach();
                } else if *c == 1 && !outgoing {
                    let _ = ctx
                        .lifecycle()
                        .do_on_resume(move || {
                            if once.replace(false) {
                                panic!("resume panic")
                            }
                        })
                        .detach();
                }
                ctx
            },
        )
        .unwrap();
        let old = lazy.get_or_create(&0).unwrap();
        let key = lazy.get().active_items.get(&0).unwrap().child.key.clone();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lazy
                .set_active_items(|_| ItemMap::from_owned([(1, ActiveLifecycleState::Resumed)]))))
            .is_err()
        );
        assert!(old.lifecycle().state().is_destroyed());
        lazy.set_active_items(|_| ItemMap::from_owned([(0, ActiveLifecycleState::Resumed)]));
        let fresh = lazy.get_or_create(&0).unwrap();
        assert!(!Rc::ptr_eq(&old, &fresh));
        assert_ne!(lazy.get().active_items.get(&0).unwrap().child.key, key);
        assert_eq!(fresh.lifecycle().state(), LifecycleState::Resumed);
    }
}
#[test]
fn dropping_views_keeps_parent_owned_engine_without_navigation_control_cycle() {
    let parent = root();
    let nav = ItemsNavigation::new();
    let capture = Rc::new(());
    let weak = Rc::downgrade(&capture);
    let lazy = child_items(
        &parent,
        &nav,
        Items::new(vec![0u32]).unwrap(),
        ItemsOptions::default(),
        move |_, ctx| {
            let _ = &capture;
            ctx
        },
    )
    .unwrap();
    drop(lazy);
    nav.set_active_items(|_| ItemMap::from_owned([(0, ActiveLifecycleState::Resumed)]));
    assert!(weak.upgrade().is_some());
    parent.lifecycle().destroy().unwrap();
    assert!(weak.upgrade().is_none());
}
#[test]
fn every_navigation_helper_on_trait_source_and_lazy_reports_normalized_old_new_state() {
    fn exercise<N: ItemsNavigator<u32>>(nav: &N, log: Rc<RefCell<Vec<(usize, usize)>>>) {
        let out = log.clone();
        nav.navigate_with(
            |s| s,
            move |r| {
                let r = r.unwrap();
                out.borrow_mut()
                    .push((r.old_items.items.len(), r.new_items.items.len()))
            },
        );
        let out = log.clone();
        nav.set_items_with(
            |mut v| {
                v.push(Rc::new(1));
                v
            },
            move |r| {
                let r = r.unwrap();
                out.borrow_mut()
                    .push((r.old_items.items.len(), r.new_items.items.len()))
            },
        );
        let out = log.clone();
        nav.set_active_items_with(
            |_| {
                ItemMap::from_owned([
                    (1, ActiveLifecycleState::Created),
                    (99, ActiveLifecycleState::Started),
                ])
            },
            move |r| {
                let r = r.unwrap();
                out.borrow_mut().push((
                    r.old_items.active_items.len(),
                    r.new_items.active_items.len(),
                ))
            },
        );
        nav.navigate(|s| s);
        nav.set_items(|v| v);
        nav.set_active_items(|m| m);
    }
    for local in [false, true] {
        let parent = root();
        let nav = ItemsNavigation::new();
        let lazy = child_items(
            &parent,
            &nav,
            Items::new(vec![0u32]).unwrap(),
            ItemsOptions::default(),
            |_, ctx| ctx,
        )
        .unwrap();
        let log = Rc::new(RefCell::new(vec![]));
        if local {
            exercise(&lazy, log.clone())
        } else {
            exercise(&nav, log.clone())
        };
        assert_eq!(*log.borrow(), [(1, 1), (1, 2), (0, 1)]);
    }
}
#[test]
fn shared_source_without_receivers_discards_transformers_and_completions() {
    let nav = ItemsNavigation::<u32>::new();
    nav.navigate_with(
        |_| panic!("no receiver transform"),
        |_| panic!("no receiver completion"),
    );
}
#[test]
fn convenience_errors_panic_and_explicit_completion_recovers() {
    let parent = root();
    let nav = ItemsNavigation::new();
    let lazy = child_items(
        &parent,
        &nav,
        Items::new(vec![0u32]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || lazy.set_items(|v| vec![v[0].clone(), v[0].clone()])
        ))
        .is_err()
    );
    lazy.get_or_create(&0).unwrap();
    parent.lifecycle().destroy().unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lazy.navigate(|s| s))).is_err()
    );
    nav.navigate_with(
        |_| panic!("destroyed disconnected source"),
        |_| panic!("destroyed disconnected completion"),
    );
}

#[test]
fn empty_initial_and_nonclone_public_types_are_cloneable_without_factories() {
    #[derive(PartialEq, Eq)]
    struct Config(u32);
    struct Component(DefaultComponentContext);
    let parent = root();
    let nav = ItemsNavigation::<Config>::default();
    let _ = nav.clone();
    let empty = Items::<Config>::default();
    let _ = empty.clone();
    let _ = ChildItems::<Config, Component>::default().clone();
    let lazy = child_items(&parent, &nav, empty, ItemsOptions::default(), |_, _| {
        panic!("empty factory")
    })
    .unwrap();
    assert!(lazy.get().items.is_empty());
    drop(lazy);
    let parent = root();
    let initial = Items::new(vec![Config(0)]).unwrap();
    let lazy = child_items(&parent, &nav, initial, ItemsOptions::default(), |_, ctx| {
        Component(ctx)
    })
    .unwrap();
    let _ = lazy.clone();
    let _ = lazy.value().clone();
    let component = lazy.get_or_create(&Config(0)).unwrap();
    let snapshot = lazy.get();
    let _ = snapshot.as_ref().clone();
    let _ = snapshot.active_items.get(&Config(0)).unwrap().clone();
    let result = Rc::new(RefCell::new(None));
    let out = result.clone();
    lazy.navigate_with(|s| s, move |r| *out.borrow_mut() = Some(r.unwrap().clone()));
    assert_eq!(result.borrow().as_ref().unwrap().new_items.items[0].0, 0);
    assert_eq!(component.0.lifecycle().state(), LifecycleState::Created);
    let codec = ItemsCodec::<Config>::new(|_| Ok(vec![]), |_| Ok(vec![]));
    let _ = codec.clone();
}
#[test]
fn nonclone_custom_context_service_survives_nested_items_and_mixed_routers() {
    use decompose_rs::router::{slot::*, stack::*};
    struct Custom {
        base: DefaultComponentContext,
        service: Rc<u32>,
    }
    impl ComponentContext for Custom {
        fn lifecycle(&self) -> LifecycleRegistry {
            self.base.lifecycle()
        }
        fn state_keeper(&self) -> SharedKeeper {
            self.base.state_keeper()
        }
        fn instance_keeper(&self) -> SharedInstances {
            self.base.instance_keeper()
        }
        fn back_handler(&self) -> SharedBack {
            self.base.back_handler()
        }
        fn context_factory(&self) -> ContextFactory<Self> {
            let service = self.service.clone();
            Rc::new(move |parts| Self {
                base: DefaultComponentContext::from_parts(parts),
                service: service.clone(),
            })
        }
    }
    #[derive(PartialEq, Eq)]
    struct Config(u32);
    let service = Rc::new(42);
    let parent = Custom {
        base: root(),
        service: service.clone(),
    };
    let items = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![Config(1)]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let child = items.get_or_create(&Config(1)).unwrap();
    let slot = child_slot(
        child.as_ref(),
        &SlotNavigation::new(),
        Some(Config(2)),
        SlotOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let nested = child_items(
        slot.get().child.as_ref().unwrap().instance.as_ref(),
        &ItemsNavigation::new(),
        Items::new(vec![Config(3)]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let third = nested.get_or_create(&Config(3)).unwrap();
    let stack = child_stack(
        third.as_ref(),
        &StackNavigation::new(),
        vec![Config(4)],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(Rc::ptr_eq(&service, &stack.get().active.instance.service));
    assert_eq!(
        stack.get().active.instance.lifecycle().state(),
        LifecycleState::Created
    );
    parent.lifecycle().destroy().unwrap();
    assert!(
        stack
            .get()
            .active
            .instance
            .lifecycle()
            .state()
            .is_destroyed()
    );
}
#[test]
fn nested_mixed_routers_restore_after_items_deactivation_and_process_death() {
    use decompose_rs::router::{slot::*, stack::*};
    struct Nested {
        ctx: DefaultComponentContext,
        nav: SlotNavigation<u32>,
        slot: decompose_rs::value::Value<ChildSlot<u32, DefaultComponentContext>>,
        stack: decompose_rs::value::Value<ChildStack<u32, DefaultComponentContext>>,
        items: LazyChildItems<u32, DefaultComponentContext>,
    }
    fn build(_: &u32, ctx: DefaultComponentContext) -> Nested {
        let nav = SlotNavigation::new();
        let slot = child_slot(
            &ctx,
            &nav,
            Some(10u32),
            SlotOptions {
                codec: Some(SlotCodec::new(
                    |c: &u32| Ok(c.to_le_bytes().to_vec()),
                    |b| Ok(u32::from_le_bytes(b.try_into().map_err(|_| "bad slot")?)),
                )),
                ..Default::default()
            },
            |_, ctx| ctx,
        )
        .unwrap();
        let stack = child_stack(
            &ctx,
            &StackNavigation::new(),
            vec![20u32, 21],
            StackOptions {
                codec: Some(StackCodec::new(
                    |v: &[Rc<u32>]| Ok(v.iter().flat_map(|c| c.to_le_bytes()).collect()),
                    |b| {
                        Ok(b.chunks_exact(4)
                            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                            .collect())
                    },
                )),
                ..Default::default()
            },
            |_, ctx| ctx,
        )
        .unwrap();
        let items = child_items(
            &ctx,
            &ItemsNavigation::new(),
            active(vec![30], &[(30, ActiveLifecycleState::Started)]),
            persistent(),
            |_, ctx| ctx,
        )
        .unwrap();
        Nested {
            ctx,
            nav,
            slot,
            stack,
            items,
        }
    }
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0], &[(0, ActiveLifecycleState::Resumed)]),
        persistent(),
        build,
    )
    .unwrap();
    let old = lazy.get_or_create(&0).unwrap();
    old.nav.activate(99);
    old.items
        .get_or_create(&30)
        .unwrap()
        .state_keeper()
        .borrow_mut()
        .register("value", || vec![55])
        .unwrap();
    let key = lazy.get().active_items.get(&0).unwrap().child.key.clone();
    lazy.set_active_items(|_| ItemMap::new());
    assert!(old.ctx.lifecycle().state().is_destroyed());
    let saved = parent.state_keeper().borrow().save().unwrap();
    parent.lifecycle().destroy().unwrap();
    let parent = restored(saved, None);
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::default(),
        persistent(),
        build,
    )
    .unwrap();
    let fresh = lazy.get_or_create(&0).unwrap();
    assert_eq!(lazy.get().active_items.get(&0).unwrap().child.key, key);
    assert_eq!(*fresh.slot.get().child.as_ref().unwrap().configuration, 99);
    assert_eq!(*fresh.stack.get().active.configuration, 21);
    assert_eq!(
        fresh
            .items
            .get_or_create(&30)
            .unwrap()
            .state_keeper()
            .borrow_mut()
            .consume_bytes("value"),
        Some(vec![55])
    );
    assert_eq!(
        fresh
            .items
            .get()
            .active_items
            .get(&30)
            .unwrap()
            .lifecycle_state,
        ActiveLifecycleState::Started
    );
}
#[test]
fn active_fetch_preserves_started_and_resumed_requested_states_without_publication() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(
            vec![0, 1],
            &[
                (0, ActiveLifecycleState::Started),
                (1, ActiveLifecycleState::Resumed),
            ],
        ),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let before = lazy.get();
    for (c, s) in [(0, LifecycleState::Started), (1, LifecycleState::Resumed)] {
        let fetched = lazy.get_or_create(&c).unwrap();
        assert!(Rc::ptr_eq(
            &fetched,
            &before.active_items.get(&c).unwrap().child.instance
        ));
        assert_eq!(fetched.lifecycle().state(), s);
    }
    assert!(Rc::ptr_eq(&before, &lazy.get()));
}
#[test]
fn busy_observer_and_completion_fetches_queue_no_activation() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![0u32, 1]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let source = lazy.clone();
    let _token = lazy.subscribe(move |s| {
        if s.active_items.contains_key(&0) {
            assert!(matches!(
                source.get_or_create(&1),
                Err(NavigationError::NavigationInProgress)
            ));
            assert!(source.get_or_create(&0).is_ok());
        }
    });
    let source = lazy.clone();
    lazy.set_active_items_with(
        |_| ItemMap::from_owned([(0, ActiveLifecycleState::Created)]),
        move |_| {
            assert!(matches!(
                source.get_or_create(&1),
                Err(NavigationError::NavigationInProgress)
            ));
            assert!(source.get_or_create(&0).is_ok());
        },
    );
    assert!(!lazy.get().active_items.contains_key(&1));
}
#[test]
fn inactive_lazy_failure_does_not_leave_activation_in_queue() {
    let parent = root();
    let fail = Rc::new(Cell::new(true));
    let flag = fail.clone();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![0u32]).unwrap(),
        ItemsOptions::default(),
        move |_, ctx| {
            if flag.replace(false) {
                panic!("lazy factory failure")
            }
            ctx
        },
    )
    .unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lazy.get_or_create(&0))).is_err()
    );
    assert!(lazy.get().active_items.is_empty());
    lazy.navigate(|s| s);
    assert!(lazy.get().active_items.is_empty());
    assert_eq!(
        lazy.get_or_create(&0).unwrap().lifecycle().state(),
        LifecycleState::Created
    );
}

#[test]
fn removal_of_inactive_member_discards_tombstone_key_and_saved_state() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![0u32]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let old = lazy.get_or_create(&0).unwrap();
    let key = lazy.get().active_items.get(&0).unwrap().child.key.clone();
    old.state_keeper()
        .borrow_mut()
        .register("saved", || vec![7])
        .unwrap();
    lazy.set_active_items(|_| ItemMap::new());
    lazy.set_items(|_| vec![]);
    lazy.set_items(|_| vec![Rc::new(0)]);
    let fresh = lazy.get_or_create(&0).unwrap();
    assert_ne!(lazy.get().active_items.get(&0).unwrap().child.key, key);
    assert_eq!(
        fresh.state_keeper().borrow_mut().consume_bytes("saved"),
        None
    );
}
#[test]
fn restoration_of_earlier_membership_discards_newer_retained_scope() {
    let instances = Rc::new(RefCell::new(InstanceKeeper::new()));
    let parent = restored(Default::default(), Some(instances.clone()));
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0], &[(0, ActiveLifecycleState::Started)]),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    let saved = parent.state_keeper().borrow().save().unwrap();
    lazy.set_items(|mut v| {
        v.push(Rc::new(1));
        v
    });
    let newer = lazy.get_or_create(&1).unwrap().instance_keeper();
    parent.lifecycle().destroy().unwrap();
    let parent = restored(saved, Some(instances));
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::default(),
        persistent(),
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(newer.borrow().is_destroyed());
    assert_eq!(lazy.get().items.len(), 1);
    lazy.set_items(|mut v| {
        v.push(Rc::new(2));
        v
    });
    assert!(!Rc::ptr_eq(
        &newer,
        &lazy.get_or_create(&2).unwrap().instance_keeper()
    ));
}
#[test]
fn lifecycle_deactivation_cannot_reenable_created_or_inactive_back_bridge() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0], &[(0, ActiveLifecycleState::Resumed)]),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let instance = lazy.get_or_create(&0).unwrap();
    let back = instance.back_handler();
    let handle = back
        .register(0, true, |_, _| panic!("demoted child Back"))
        .unwrap();
    let callback_back = back.clone();
    let _ = instance
        .lifecycle()
        .do_on_stop(move || callback_back.set_enabled(handle.id(), true))
        .detach();
    lazy.set_active_items(|_| ItemMap::from_owned([(0, ActiveLifecycleState::Created)]));
    assert!(!parent.back_handler().back());
    lazy.set_active_items(|_| ItemMap::new());
    back.set_enabled(handle.id(), true);
    assert!(!parent.back_handler().back());
}
#[test]
fn local_lazy_value_and_subscribe_share_identical_snapshot_source() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![0u32]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let value = lazy.value();
    let clone = lazy.clone();
    assert!(Rc::ptr_eq(&value.get(), &lazy.get()));
    let delivered = Rc::new(RefCell::new(None));
    let output = delivered.clone();
    let _token = clone.subscribe(move |s| *output.borrow_mut() = Some(s));
    lazy.get_or_create(&0).unwrap();
    assert!(Rc::ptr_eq(
        delivered.borrow().as_ref().unwrap(),
        &value.get()
    ));
    assert!(Rc::ptr_eq(&clone.get(), &lazy.get()));
}

#[test]
fn reactivated_equal_configuration_uses_current_payload_with_stable_key_and_saved_state() {
    struct Config {
        id: u32,
        payload: &'static str,
    }
    impl PartialEq for Config {
        fn eq(&self, other: &Self) -> bool {
            self.id == other.id
        }
    }
    impl Eq for Config {}
    for initially_active in [false, true] {
        let parent = root();
        let mut initial = Items::new(vec![Config {
            id: 0,
            payload: "old",
        }])
        .unwrap();
        if initially_active {
            initial
                .active_items
                .insert(initial.items[0].clone(), ActiveLifecycleState::Started);
        }
        let seen = Rc::new(RefCell::new(vec![]));
        let out = seen.clone();
        let lazy = child_items(
            &parent,
            &ItemsNavigation::new(),
            initial,
            ItemsOptions::default(),
            move |c, ctx| {
                out.borrow_mut().push(c.payload);
                ctx
            },
        )
        .unwrap();
        let old_key = if initially_active {
            let old = lazy
                .get_or_create(&Config {
                    id: 0,
                    payload: "probe",
                })
                .unwrap();
            old.state_keeper()
                .borrow_mut()
                .register("saved", || vec![7])
                .unwrap();
            let key = lazy
                .get()
                .active_items
                .get(&Config {
                    id: 0,
                    payload: "probe",
                })
                .unwrap()
                .child
                .key
                .clone();
            lazy.set_active_items(|_| ItemMap::new());
            Some(key)
        } else {
            None
        };
        lazy.set_items(|_| {
            vec![Rc::new(Config {
                id: 0,
                payload: "new",
            })]
        });
        let fresh = lazy
            .get_or_create(&Config {
                id: 0,
                payload: "probe",
            })
            .unwrap();
        assert_eq!(seen.borrow().last(), Some(&"new"));
        assert_eq!(
            lazy.get()
                .active_items
                .get(&Config {
                    id: 0,
                    payload: "probe"
                })
                .unwrap()
                .child
                .configuration
                .payload,
            "new"
        );
        if let Some(key) = old_key {
            assert_eq!(
                lazy.get()
                    .active_items
                    .get(&Config {
                        id: 0,
                        payload: "probe"
                    })
                    .unwrap()
                    .child
                    .key,
                key
            );
            assert_eq!(
                fresh.state_keeper().borrow_mut().consume_bytes("saved"),
                Some(vec![7])
            );
        } else {
            assert_eq!(seen.borrow().as_slice(), ["new"]);
        }
    }
}

#[test]
fn equal_live_replacement_preserves_instance_then_uses_new_payload_on_recreation() {
    struct Config {
        id: u32,
        payload: &'static str,
    }
    impl PartialEq for Config {
        fn eq(&self, o: &Self) -> bool {
            self.id == o.id
        }
    }
    impl Eq for Config {}
    let parent = root();
    let mut initial = Items::new(vec![Config {
        id: 0,
        payload: "old",
    }])
    .unwrap();
    initial
        .active_items
        .insert(initial.items[0].clone(), ActiveLifecycleState::Resumed);
    let calls = Rc::new(RefCell::new(vec![]));
    let out = calls.clone();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        initial,
        ItemsOptions::default(),
        move |c, ctx| {
            out.borrow_mut().push(c.payload);
            ctx
        },
    )
    .unwrap();
    let probe = Config {
        id: 0,
        payload: "lookup",
    };
    let old = lazy.get_or_create(&probe).unwrap();
    let key = lazy
        .get()
        .active_items
        .get(&probe)
        .unwrap()
        .child
        .key
        .clone();
    lazy.set_items(|_| {
        vec![Rc::new(Config {
            id: 0,
            payload: "new",
        })]
    });
    assert!(Rc::ptr_eq(&old, &lazy.get_or_create(&probe).unwrap()));
    assert_eq!(calls.borrow().as_slice(), ["old"]);
    assert_eq!(
        lazy.get()
            .active_items
            .get(&probe)
            .unwrap()
            .child
            .configuration
            .payload,
        "new"
    );
    lazy.set_active_items(|_| ItemMap::new());
    assert!(!Rc::ptr_eq(&old, &lazy.get_or_create(&probe).unwrap()));
    assert_eq!(calls.borrow().as_slice(), ["old", "new"]);
    assert_eq!(lazy.get().active_items.get(&probe).unwrap().child.key, key);
}
#[test]
fn no_op_navigation_never_restarts_live_lifecycle() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0], &[(0, ActiveLifecycleState::Resumed)]),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let life = lazy.get_or_create(&0).unwrap().lifecycle();
    let states = Rc::new(RefCell::new(vec![]));
    let out = states.clone();
    let _token = life.subscribe(move |s| out.borrow_mut().push(s));
    states.borrow_mut().clear();
    lazy.navigate(|s| s);
    lazy.set_items(|v| v);
    lazy.set_active_items(|m| m);
    assert!(states.borrow().is_empty());
    assert_eq!(life.state(), LifecycleState::Resumed);
}

#[test]
fn lazy_fetch_rejects_ended_old_scope_immediately_after_caught_lifecycle_panic() {
    for outgoing in [false, true] {
        let parent = root();
        let once = Rc::new(Cell::new(true));
        let flag = once.clone();
        let lazy = child_items(
            &parent,
            &ItemsNavigation::new(),
            active(vec![0, 1], &[(0, ActiveLifecycleState::Resumed)]),
            ItemsOptions::default(),
            move |c, ctx| {
                let flag = flag.clone();
                if *c == 0 && outgoing {
                    let _ = ctx
                        .lifecycle()
                        .do_on_destroy(move || {
                            if flag.replace(false) {
                                panic!("outgoing destruction panic");
                            }
                        })
                        .detach();
                } else if *c == 1 && !outgoing {
                    let _ = ctx
                        .lifecycle()
                        .do_on_resume(move || {
                            if flag.replace(false) {
                                panic!("incoming activation panic");
                            }
                        })
                        .detach();
                }
                ctx
            },
        )
        .unwrap();
        let old = lazy.get_or_create(&0).unwrap();
        let snapshot = lazy.get();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lazy
                .set_active_items(|_| ItemMap::from_owned([(1, ActiveLifecycleState::Resumed)]))))
            .is_err()
        );
        assert!(old.lifecycle().state().is_destroyed());
        assert!(matches!(
            lazy.get_or_create(&0),
            Err(NavigationError::ChildUnavailable)
        ));
        assert!(Rc::ptr_eq(&snapshot, &lazy.get()));
        lazy.navigate(|s| s);
        let repaired = lazy.get_or_create(&0).unwrap();
        assert!(!Rc::ptr_eq(&old, &repaired));
        assert_eq!(repaired.lifecycle().state(), LifecycleState::Resumed);
    }
}
#[test]
fn lazy_fetch_rejects_manually_destroyed_child_lifecycle_without_resurrecting() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0], &[(0, ActiveLifecycleState::Started)]),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let old = lazy.get_or_create(&0).unwrap();
    let snapshot = lazy.get();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        old.lifecycle().destroy().unwrap()
    }));
    assert!(old.lifecycle().state().is_destroyed());
    assert!(matches!(
        lazy.get_or_create(&0),
        Err(NavigationError::ChildUnavailable)
    ));
    assert!(Rc::ptr_eq(&snapshot, &lazy.get()));
    parent.lifecycle().destroy().unwrap();
    assert!(matches!(
        lazy.get_or_create(&0),
        Err(NavigationError::Destroyed)
    ));
}
#[test]
fn lazy_fetch_rejects_destroyed_or_exclusively_borrowed_child_keeper() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        active(vec![0], &[(0, ActiveLifecycleState::Resumed)]),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let old = lazy.get_or_create(&0).unwrap();
    let snapshot = lazy.get();
    let keeper = old.instance_keeper();
    let borrowed = keeper.borrow_mut();
    assert!(matches!(
        lazy.get_or_create(&0),
        Err(NavigationError::ChildUnavailable)
    ));
    assert!(Rc::ptr_eq(&snapshot, &lazy.get()));
    drop(borrowed);
    assert!(Rc::ptr_eq(&old, &lazy.get_or_create(&0).unwrap()));
    keeper.borrow_mut().destroy_all();
    assert!(matches!(
        lazy.get_or_create(&0),
        Err(NavigationError::ChildUnavailable)
    ));
    assert!(Rc::ptr_eq(&snapshot, &lazy.get()));
}

#[test]
fn lazy_fetch_rechecks_child_liveness_after_synchronous_activation_callbacks() {
    let parent = root();
    let lazy = child_items(
        &parent,
        &ItemsNavigation::new(),
        Items::new(vec![0u32]).unwrap(),
        ItemsOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let token = lazy.subscribe(move |snapshot| {
        if let Some(child) = snapshot.active_items.get(&0) {
            let life = child.child.instance.lifecycle();
            let _ =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| life.destroy().unwrap()));
        }
    });
    assert!(matches!(
        lazy.get_or_create(&0),
        Err(NavigationError::ChildUnavailable)
    ));
    assert!(
        lazy.get()
            .active_items
            .get(&0)
            .unwrap()
            .child
            .instance
            .lifecycle()
            .state()
            .is_destroyed()
    );
    let snapshot = lazy.get();
    assert!(matches!(
        lazy.get_or_create(&0),
        Err(NavigationError::ChildUnavailable)
    ));
    assert!(Rc::ptr_eq(&snapshot, &lazy.get()));
    drop(token);
}
