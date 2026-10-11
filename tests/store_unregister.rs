use decompose_rs::{StateKeeper, StateStore};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

struct OnDrop(Option<Box<dyn FnOnce()>>);
impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.take().unwrap()();
    }
}
fn register_drop(store: &StateStore, key: &str, action: impl FnOnce() + 'static) {
    let capture = OnDrop(Some(Box::new(action)));
    store
        .register(key, move || {
            let _ = &capture;
            vec![1]
        })
        .unwrap();
}

#[test]
fn removed_provider_reads_actual_remaining_registration_and_restored_state() {
    let store = StateStore::from(StateKeeper::with_restored(BTreeMap::from([(
        "pending".into(),
        vec![7],
    )])));
    store.register("unrelated", || vec![9]).unwrap();
    let peer = store.clone();
    register_drop(&store, "removed", move || {
        assert!(!peer.is_registered("removed").unwrap());
        assert!(peer.is_registered("unrelated").unwrap());
        assert_eq!(peer.provider_count().unwrap(), 1);
        assert!(peer.has_consumable("pending").unwrap());
    });
    assert!(store.unregister("removed").unwrap());
    assert!(!store.unregister("removed").unwrap());
}

#[test]
fn destructor_registration_including_same_key_is_preserved() {
    let store = StateStore::default();
    let peer = store.clone();
    register_drop(&store, "removed", move || {
        peer.register("replacement", || vec![2]).unwrap();
        peer.register("removed", || vec![3]).unwrap();
    });
    assert!(store.unregister("removed").unwrap());
    assert_eq!(
        store.save().unwrap(),
        BTreeMap::from([("removed".into(), vec![3]), ("replacement".into(), vec![2])])
    );
}

#[test]
fn destructor_unregisters_another_provider_without_resurrection() {
    let store = StateStore::default();
    store.register("other", || vec![2]).unwrap();
    let peer = store.clone();
    register_drop(&store, "removed", move || {
        assert!(peer.unregister("other").unwrap());
    });
    store.unregister("removed").unwrap();
    assert_eq!(store.provider_count().unwrap(), 0);
}

#[test]
fn destructor_consumes_pending_input_once() {
    let store = StateStore::from(StateKeeper::with_restored(BTreeMap::from([(
        "pending".into(),
        vec![7],
    )])));
    let peer = store.clone();
    let consumed = Rc::new(RefCell::new(None));
    let output = consumed.clone();
    register_drop(&store, "removed", move || {
        *output.borrow_mut() = peer.consume_bytes("pending").unwrap();
    });
    store.unregister("removed").unwrap();
    assert_eq!(*consumed.borrow(), Some(vec![7]));
    assert_eq!(store.consume_bytes("pending").unwrap(), None);
}

#[test]
fn destructor_panic_preserves_other_providers_input_and_successful_mutations() {
    let store = StateStore::from(StateKeeper::with_restored(BTreeMap::from([(
        "pending".into(),
        vec![7],
    )])));
    store.register("unrelated", || vec![9]).unwrap();
    let peer = store.clone();
    register_drop(&store, "removed", move || {
        peer.register("replacement", || vec![2]).unwrap();
        panic!("provider cleanup");
    });
    assert!(catch_unwind(AssertUnwindSafe(|| store.unregister("removed"))).is_err());
    assert!(!store.is_registered("removed").unwrap());
    assert_eq!(
        store.save().unwrap(),
        BTreeMap::from([
            ("pending".into(), vec![7]),
            ("replacement".into(), vec![2]),
            ("unrelated".into(), vec![9])
        ])
    );
    assert_eq!(store.consume_bytes("pending").unwrap(), Some(vec![7]));
}

#[test]
fn optional_and_fallible_capture_release_has_the_same_contract() {
    let store = StateStore::default();
    for optional in [false, true] {
        let peer = store.clone();
        let capture = OnDrop(Some(Box::new(move || {
            peer.register("replacement", || vec![4]).unwrap();
        })));
        if optional {
            store
                .register_optional("key", move || {
                    let _ = &capture;
                    None
                })
                .unwrap();
        } else {
            store
                .register_fallible("key", move || {
                    let _ = &capture;
                    Ok(vec![3])
                })
                .unwrap();
        }
        store.unregister("key").unwrap();
        assert!(store.is_registered("replacement").unwrap());
        store.unregister("replacement").unwrap();
    }
}

#[test]
fn providers_registered_before_sharing_use_detachable_ownership() {
    let mut keeper = StateKeeper::new();
    let output = Rc::new(RefCell::new(None::<StateStore>));
    let peer = output.clone();
    let capture = OnDrop(Some(Box::new(move || {
        peer.borrow()
            .as_ref()
            .unwrap()
            .register("replacement", || vec![8])
            .unwrap();
    })));
    keeper
        .register_value(
            "key",
            move || {
                let _ = &capture;
                3_u8
            },
            |value| Ok(vec![*value]),
        )
        .unwrap();
    let store = StateStore::from(keeper);
    *output.borrow_mut() = Some(store.clone());
    assert!(store.unregister("key").unwrap());
    assert_eq!(store.save().unwrap()["replacement"], [8]);
}

#[test]
fn ordinary_registration_duplicate_and_optional_pending_semantics_remain() {
    let store = StateStore::from(StateKeeper::with_restored(BTreeMap::from([(
        "key".into(),
        vec![7],
    )])));
    store.register_optional("key", || None).unwrap();
    assert!(store.register("key", || vec![9]).is_err());
    assert_eq!(store.save().unwrap()["key"], [7]);
    assert!(store.unregister("key").unwrap());
    assert_eq!(store.consume_bytes("key").unwrap(), Some(vec![7]));
}

#[test]
fn provider_map_finishes_independent_capture_release_after_panics() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut keeper = StateKeeper::new();
    for key in ["a", "b", "c"] {
        let log = seen.clone();
        let capture = OnDrop(Some(Box::new(move || {
            log.borrow_mut().push(key);
            if key != "c" {
                panic!("{key} cleanup");
            }
        })));
        keeper
            .register(key, move || {
                let _ = &capture;
                vec![]
            })
            .unwrap();
    }
    let panic = catch_unwind(AssertUnwindSafe(|| drop(keeper))).unwrap_err();
    assert_eq!(panic.downcast_ref::<String>().unwrap(), "a cleanup");
    assert_eq!(*seen.borrow(), ["a", "b", "c"]);
}
