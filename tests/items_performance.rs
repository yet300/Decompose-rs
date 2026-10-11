use decompose_rs::router::items::*;
use decompose_rs::*;
use std::{cell::Cell, rc::Rc};

struct Counted {
    id: usize,
    comparisons: Rc<Cell<usize>>,
}
impl PartialEq for Counted {
    fn eq(&self, other: &Self) -> bool {
        self.comparisons.set(self.comparisons.get() + 1);
        self.id == other.id
    }
}
impl Eq for Counted {}
fn root() -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::new(lifecycle)
}
fn unchanged_comparisons(n: usize) -> usize {
    let comparisons = Rc::new(Cell::new(0));
    let parent = root();
    let navigation = ItemsNavigation::new();
    let _items = child_items_indexed(
        &parent,
        &navigation,
        Items::new(
            (0..n)
                .map(|id| Counted {
                    id,
                    comparisons: comparisons.clone(),
                })
                .collect(),
        )
        .unwrap(),
        ItemsOptions::default(),
        ConfigurationIndex::equality_only(),
        |_, _| panic!("inactive factory"),
    )
    .unwrap();
    comparisons.set(0);
    let _ = navigation.try_navigate(std::convert::identity).unwrap();
    comparisons.get()
}
#[test]
fn unchanged_inactive_membership_uses_linear_equality_work() {
    for n in [256, 512, 1024] {
        let count = unchanged_comparisons(n);
        println!("unchanged n={n}: {count}");
        assert!(count <= 2 * n, "n={n}: {count} equality comparisons");
    }
}

#[test]
fn default_mutable_equality_fallback_keeps_reproducible_comparison_baseline() {
    for n in [256, 512, 1024] {
        let comparisons = Rc::new(Cell::new(0));
        let parent = root();
        let navigation = ItemsNavigation::new();
        let _items = child_items(
            &parent,
            &navigation,
            Items::new(
                (0..n)
                    .map(|id| Counted {
                        id,
                        comparisons: comparisons.clone(),
                    })
                    .collect(),
            )
            .unwrap(),
            ItemsOptions::default(),
            |_, _| (),
        )
        .unwrap();
        comparisons.set(0);
        let _ = navigation.try_navigate(std::convert::identity).unwrap();
        let count = comparisons.get();
        println!("default fallback n={n}: {count}");
        assert_eq!(count, n * (n - 1) / 2);
    }
}

#[test]
fn default_eq_only_update_revalidates_interior_mutated_identity() {
    struct MutableIdentity(Cell<u32>);
    impl PartialEq for MutableIdentity {
        fn eq(&self, other: &Self) -> bool {
            self.0.get() == other.0.get()
        }
    }
    impl Eq for MutableIdentity {}
    let parent = root();
    let navigation = ItemsNavigation::new();
    let items = child_items(
        &parent,
        &navigation,
        Items::new(vec![
            MutableIdentity(Cell::new(1)),
            MutableIdentity(Cell::new(2)),
        ])
        .unwrap(),
        ItemsOptions::default(),
        |_, _| (),
    )
    .unwrap();
    items.get().items[1].0.set(1);
    assert!(matches!(
        navigation.try_navigate(std::convert::identity),
        Err(decompose_rs::router::NavigationError::DuplicateConfiguration)
    ));
}

#[test]
fn default_map_preserves_first_equal_match_after_interior_mutation() {
    struct MutableIdentity(Cell<u32>);
    impl PartialEq for MutableIdentity {
        fn eq(&self, other: &Self) -> bool {
            self.0.get() == other.0.get()
        }
    }
    impl Eq for MutableIdentity {}
    let first = Rc::new(MutableIdentity(Cell::new(1)));
    let second = Rc::new(MutableIdentity(Cell::new(2)));
    let mut map = ItemMap::new();
    map.insert(first, 10);
    map.insert(second.clone(), 20);
    second.0.set(1);
    assert_eq!(map.get(&second), Some(&10));
    assert_eq!(map.insert(second, 30), Some(10));
    assert_eq!(map.values().copied().collect::<Vec<_>>(), [30, 20]);
}

#[test]
fn default_matching_preserves_earliest_equal_child_after_interior_mutation() {
    struct MutableIdentity(Cell<u32>);
    impl PartialEq for MutableIdentity {
        fn eq(&self, other: &Self) -> bool {
            self.0.get() == other.0.get()
        }
    }
    impl Eq for MutableIdentity {}
    let parent = root();
    let navigation = ItemsNavigation::new();
    let mut initial = Items::new(vec![
        MutableIdentity(Cell::new(1)),
        MutableIdentity(Cell::new(2)),
    ])
    .unwrap();
    for configuration in &initial.items {
        initial
            .active_items
            .insert(configuration.clone(), ActiveLifecycleState::Created);
    }
    let items = child_items(
        &parent,
        &navigation,
        initial,
        ItemsOptions::default(),
        |configuration, _| configuration.0.get(),
    )
    .unwrap();
    let snapshot = items.get();
    let earliest = snapshot
        .active_items
        .values()
        .next()
        .unwrap()
        .child
        .instance
        .clone();
    snapshot.items[1].0.set(1);
    navigation.set_items_or_panic(|_| {
        vec![
            Rc::new(MutableIdentity(Cell::new(3))),
            Rc::new(MutableIdentity(Cell::new(1))),
        ]
    });
    assert!(Rc::ptr_eq(
        &earliest,
        &items
            .get()
            .active_items
            .get(&MutableIdentity(Cell::new(1)))
            .unwrap()
            .child
            .instance
    ));
}

#[test]
fn indexed_changed_active_membership_reaches_engine_matching_and_snapshot_maps() {
    for n in [256, 512, 1024] {
        let comparisons = Rc::new(Cell::new(0));
        let configurations = || {
            (0..n)
                .map(|id| Counted {
                    id,
                    comparisons: comparisons.clone(),
                })
                .collect::<Vec<_>>()
        };
        let parent = root();
        let nav = ItemsNavigation::new();
        let index = ConfigurationIndex::new(|c: &Counted| c.id as u64);
        let mut initial = Items {
            items: configurations().into_iter().map(Rc::new).collect(),
            active_items: ItemMap::with_index(index.clone()),
        };
        for configuration in &initial.items {
            initial
                .active_items
                .insert(configuration.clone(), ActiveLifecycleState::Created);
        }
        let lazy = child_items_indexed(
            &parent,
            &nav,
            initial,
            ItemsOptions::default(),
            index.clone(),
            |_, ctx| ctx,
        )
        .unwrap();
        let first = lazy
            .get()
            .active_items
            .values()
            .next()
            .unwrap()
            .child
            .instance
            .clone();
        let changed = configurations()
            .into_iter()
            .rev()
            .map(Rc::new)
            .collect::<Vec<_>>();
        comparisons.set(0);
        let _ = nav.try_set_items(move |_| changed.clone()).unwrap();
        let count = comparisons.get();
        println!("indexed active reorder n={n}: {count}");
        assert!(count <= 8 * n, "n={n}: {count} comparisons");
        assert!(Rc::ptr_eq(
            &first,
            &lazy
                .get()
                .active_items
                .get(&Counted { id: 0, comparisons })
                .unwrap()
                .child
                .instance
        ));
        assert_eq!(lazy.get().items[0].id, n - 1);
    }
}

#[test]
fn indexed_collision_duplicates_and_map_replacement_use_equality() {
    let index = ConfigurationIndex::new(|_: &u32| 0);
    let mut map = ItemMap::with_index(index.clone());
    assert_eq!(map.insert_owned(1, 10), None);
    assert_eq!(map.insert_owned(2, 20), None);
    assert_eq!(map.insert_owned(1, 30), Some(10));
    assert_eq!(map.get(&2), Some(&20));
    assert_eq!(map.remove(&1), Some(30));
    assert_eq!(map.get(&2), Some(&20));
    map.retain(|_, v| {
        *v += 1;
        true
    });
    assert_eq!(map.get(&2), Some(&21));
    let parent = root();
    let nav = ItemsNavigation::new();
    let lazy = child_items_indexed(
        &parent,
        &nav,
        Items::new(vec![1, 2]).unwrap(),
        ItemsOptions::default(),
        index,
        |_, ctx| ctx,
    )
    .unwrap();
    let result = nav.try_set_items(|_| vec![Rc::new(1), Rc::new(1)]);
    assert!(matches!(
        result,
        Err(decompose_rs::router::NavigationError::DuplicateConfiguration)
    ));
    assert_eq!(
        lazy.get().items.iter().map(|c| **c).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(
        lazy.get_or_create(&2).unwrap().lifecycle().state(),
        LifecycleState::Created
    );
}

#[test]
fn unchanged_active_eq_only_membership_uses_linear_equality_work() {
    for n in [256, 512, 1024] {
        let comparisons = Rc::new(Cell::new(0));
        let parent = root();
        let navigation = ItemsNavigation::new();
        let mut initial = Items::new(
            (0..n)
                .map(|id| Counted {
                    id,
                    comparisons: comparisons.clone(),
                })
                .collect(),
        )
        .unwrap();
        for configuration in &initial.items {
            initial
                .active_items
                .insert(configuration.clone(), ActiveLifecycleState::Created);
        }
        let lazy = child_items_indexed(
            &parent,
            &navigation,
            initial,
            ItemsOptions::default(),
            ConfigurationIndex::equality_only(),
            |_, ctx| ctx,
        )
        .unwrap();
        comparisons.set(0);
        let _ = navigation.try_navigate(std::convert::identity).unwrap();
        let count = comparisons.get();
        println!("unchanged Eq-only active n={n}: {count}");
        assert!(count <= 5 * n, "n={n}: {count} comparisons");
        assert_eq!(lazy.get().active_items.len(), n);
    }
}

#[test]
fn equal_new_allocations_take_fast_path_without_changing_active_identity() {
    let n = 256;
    let comparisons = Rc::new(Cell::new(0));
    let parent = root();
    let nav = ItemsNavigation::new();
    let lazy = child_items_indexed(
        &parent,
        &nav,
        Items::new(
            (0..n)
                .map(|id| Counted {
                    id,
                    comparisons: comparisons.clone(),
                })
                .collect(),
        )
        .unwrap(),
        ItemsOptions::default(),
        ConfigurationIndex::equality_only(),
        |_, ctx| ctx,
    )
    .unwrap();
    let original = lazy
        .get_or_create(&Counted {
            id: 0,
            comparisons: comparisons.clone(),
        })
        .unwrap();
    let new_items = (0..n)
        .map(|id| {
            Rc::new(Counted {
                id,
                comparisons: comparisons.clone(),
            })
        })
        .collect::<Vec<_>>();
    comparisons.set(0);
    let _ = nav.try_set_items(move |_| new_items.clone()).unwrap();
    assert!(comparisons.get() <= 6 * n);
    assert!(Rc::ptr_eq(
        &original,
        &lazy.get_or_create(&Counted { id: 0, comparisons }).unwrap()
    ));
}

#[test]
fn indexed_constructor_checks_duplicates_and_uses_linear_work() {
    let comparisons = Rc::new(Cell::new(0));
    let index = ConfigurationIndex::new(|c: &Counted| c.id as u64);
    let items = Items::new_indexed(
        (0..1024)
            .map(|id| Counted {
                id,
                comparisons: comparisons.clone(),
            })
            .collect(),
        index,
    )
    .unwrap();
    assert_eq!(items.items.len(), 1024);
    assert!(comparisons.get() <= 1024);
    assert!(matches!(
        Items::new_indexed(vec![1, 2, 1], ConfigurationIndex::new(|_: &u32| 0)),
        Err(decompose_rs::router::NavigationError::DuplicateConfiguration)
    ));
}

#[test]
fn indexed_saved_tombstones_restore_lazy_state_and_lifecycle() {
    fn options() -> ItemsOptions<u32> {
        ItemsOptions {
            codec: Some(ItemsCodec::new(
                |items: &[Rc<u32>]| Ok(items.iter().flat_map(|c| c.to_le_bytes()).collect()),
                |bytes| {
                    Ok(bytes
                        .chunks_exact(4)
                        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                        .collect())
                },
            )),
            ..Default::default()
        }
    }
    let parent = root();
    let nav = ItemsNavigation::new();
    let index = ConfigurationIndex::<u32>::hashed();
    let lazy = child_items_indexed(
        &parent,
        &nav,
        Items::new_indexed(vec![1, 2], index.clone()).unwrap(),
        options(),
        index.clone(),
        |_, ctx| ctx,
    )
    .unwrap();
    let first = lazy.get_or_create(&1).unwrap();
    first
        .state_store()
        .register("payload", || vec![9, 8])
        .unwrap();
    let key = lazy.get().active_items.get(&1).unwrap().child.key.clone();
    let _ = nav.try_set_active_items(|_| ItemMap::new()).unwrap();
    assert_eq!(first.lifecycle().state(), LifecycleState::Destroyed);
    let saved = parent.state_store().save().unwrap();
    let life = LifecycleRegistry::new();
    life.move_to(LifecycleState::Resumed).unwrap();
    let restored = DefaultComponentContext::with_stores(
        life,
        StateStore::from(StateKeeper::with_restored(saved)),
        InstanceStore::from(InstanceKeeper::new()),
        SharedBack::new(),
    );
    let nav = ItemsNavigation::new();
    let lazy = child_items_with_initial_indexed(
        &restored,
        &nav,
        || panic!("saved state skips initializer"),
        options(),
        index,
        |_, ctx| ctx,
    )
    .unwrap();
    assert!(lazy.get().active_items.is_empty());
    assert_eq!(
        lazy.get().items.iter().map(|c| **c).collect::<Vec<_>>(),
        [1, 2]
    );
    let child = lazy.get_or_create(&1).unwrap();
    assert_eq!(
        child.state_store().consume_bytes("payload").unwrap(),
        Some(vec![9, 8])
    );
    assert_eq!(lazy.get().active_items.get(&1).unwrap().child.key, key);
    assert_eq!(child.lifecycle().state(), LifecycleState::Created);
    let _ = nav
        .try_set_items(|items| items.into_iter().filter(|c| **c != 1).collect())
        .unwrap();
    assert_eq!(child.lifecycle().state(), LifecycleState::Destroyed);
}

#[test]
fn indexed_map_retains_correct_lookups_after_user_predicate_or_fingerprint_panic() {
    let panic_hash = Rc::new(Cell::new(false));
    let flag = panic_hash.clone();
    let mut map = ItemMap::with_index(ConfigurationIndex::new(move |key: &u32| {
        assert!(!flag.get(), "fingerprint panic");
        u64::from(*key)
    }));
    for key in 1..=3 {
        map.insert_owned(key, key * 10);
    }
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            map.retain(|key, _| {
                assert_ne!(*key, 2, "predicate panic");
                false
            });
        }))
        .is_err()
    );
    assert_eq!(map.get(&1), None);
    assert_eq!(map.get(&2), Some(&20));
    assert_eq!(map.get(&3), Some(&30));
    panic_hash.set(true);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| map.retain(|_, _| true))).is_err()
    );
    panic_hash.set(false);
    assert_eq!(map.get(&2), Some(&20));
    assert_eq!(map.get(&3), Some(&30));
    assert_eq!(map.remove(&2), Some(20));
    assert_eq!(map.get(&3), Some(&30));
}
