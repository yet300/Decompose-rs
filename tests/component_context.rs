use decompose_rs::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

fn resumed() -> LifecycleRegistry {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    lifecycle
}
fn root(saved: BTreeMap<String, Vec<u8>>, instances: SharedInstances) -> DefaultComponentContext {
    DefaultComponentContext::with_all(
        resumed(),
        Rc::new(RefCell::new(StateKeeper::with_restored(saved))),
        instances,
        SharedBack::new(),
    )
}

#[test]
fn restored_frame_survives_the_first_conflicted_save() {
    let original = DefaultComponentContext::new(resumed());
    let child = original.child_context("child").unwrap();
    child
        .state_keeper()
        .borrow_mut()
        .register("data", || vec![42])
        .unwrap();
    let saved = original.state_keeper().borrow().save().unwrap();
    let restored = root(saved.clone(), Rc::new(RefCell::new(InstanceKeeper::new())));
    let child = restored.child_context("child").unwrap();
    let state = child.state_keeper();
    let guard = state.borrow_mut();
    assert_eq!(restored.state_keeper().borrow().save().unwrap(), saved);
    drop(guard);
    assert_eq!(state.borrow_mut().consume_bytes("data"), Some(vec![42]));
}

struct FallibleContext {
    delegate: DefaultComponentContext,
    fail: Rc<Cell<bool>>,
}
impl ComponentContext for FallibleContext {
    fn lifecycle(&self) -> LifecycleRegistry {
        self.delegate.lifecycle()
    }
    fn state_keeper(&self) -> SharedKeeper {
        self.delegate.state_keeper()
    }
    fn instance_keeper(&self) -> SharedInstances {
        self.delegate.instance_keeper()
    }
    fn back_handler(&self) -> SharedBack {
        self.delegate.back_handler()
    }
    fn context_factory(&self) -> ContextFactory<Self> {
        let fail = self.fail.clone();
        Rc::new(move |parts| {
            assert!(!fail.get(), "intentional construction failure");
            Self {
                delegate: DefaultComponentContext::from_parts(parts),
                fail: fail.clone(),
            }
        })
    }
}

#[test]
fn failed_construction_keeps_restored_state_retryable() {
    let original = DefaultComponentContext::new(resumed());
    original
        .child_context("child")
        .unwrap()
        .state_keeper()
        .borrow_mut()
        .register("data", || vec![7])
        .unwrap();
    let saved = original.state_keeper().borrow().save().unwrap();
    let fail = Rc::new(Cell::new(true));
    let restored = FallibleContext {
        delegate: root(saved.clone(), Rc::new(RefCell::new(InstanceKeeper::new()))),
        fail: fail.clone(),
    };
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        restored.child_context("child")
    }));
    assert!(outcome.is_err());
    assert_eq!(restored.state_keeper().borrow().save().unwrap(), saved);
    fail.set(false);
    let child = restored.child_context("child").unwrap();
    assert_eq!(
        child.state_keeper().borrow_mut().consume_bytes("data"),
        Some(vec![7])
    );
}

#[test]
fn fixed_child_retention_survives_destroying_old_component_lifecycle() {
    let retained = Rc::new(RefCell::new(InstanceKeeper::new()));
    let original = root(BTreeMap::new(), retained.clone());
    let child = original.child_context("child").unwrap();
    let scope = child.instance_keeper();
    let value = scope
        .borrow_mut()
        .get_or_create("service", || 17u32)
        .unwrap();
    let saved = original.state_keeper().borrow().save().unwrap();
    original.lifecycle().destroy().unwrap();
    let recreated = root(saved, retained.clone());
    let child = recreated.child_context("child").unwrap();
    assert!(Rc::ptr_eq(&scope, &child.instance_keeper()));
    assert!(!scope.borrow().is_destroyed());
    assert!(Rc::ptr_eq(
        &value,
        &child
            .instance_keeper()
            .borrow_mut()
            .get_or_create("service", || 99u32)
            .unwrap()
    ));
    retained.borrow_mut().destroy_all();
    assert!(scope.borrow().is_destroyed());
}

#[test]
fn explicit_child_lifecycle_also_retains_instances_across_parent_recreation() {
    let retained = Rc::new(RefCell::new(InstanceKeeper::new()));
    let original = root(BTreeMap::new(), retained.clone());
    let local = resumed();
    let child = original
        .child_context_with_lifecycle("child", &local)
        .unwrap();
    let scope = child.instance_keeper();
    let saved = original.state_keeper().borrow().save().unwrap();
    original.lifecycle().destroy().unwrap();
    assert!(!scope.borrow().is_destroyed());
    let recreated = root(saved, retained);
    let child = recreated
        .child_context_with_lifecycle("child", &resumed())
        .unwrap();
    assert!(Rc::ptr_eq(&scope, &child.instance_keeper()));
}
