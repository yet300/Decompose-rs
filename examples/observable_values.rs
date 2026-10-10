use decompose_rs::value::{MutableValue, ObserveLifecycleMode};
use decompose_rs::{LifecycleRegistry, LifecycleState};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

struct Counter(i32);

fn main() {
    let source = MutableValue::new(Counter(3));
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let mapped = source.value().map(move |counter| {
        count.set(count.get() + 1);
        format!("counter={}", counter.0)
    });
    let first = mapped.get();
    assert!(Rc::ptr_eq(&first, &mapped.get()));
    assert_eq!(calls.get(), 1);

    let previous = source.get();
    let next = Rc::new(Counter(5));
    assert!(source.compare_and_set(&previous, next.clone()));
    assert!(Rc::ptr_eq(&source.get(), &next));
    assert!(!source.compare_and_set(&previous, Rc::new(Counter(99))));
    assert_eq!(mapped.get().as_str(), "counter=5");
    assert_eq!(calls.get(), 2);

    let attempts = Cell::new(0);
    let reentrant = source.clone();
    let installed = source.update_and_get(|old| {
        attempts.set(attempts.get() + 1);
        if attempts.get() == 1 {
            reentrant.set(Counter(12));
        }
        Rc::new(Counter(old.0 + 1))
    });
    assert_eq!(attempts.get(), 2);
    assert_eq!(installed.0, 13);
    assert!(Rc::ptr_eq(&installed, &source.get()));

    let lifecycle = LifecycleRegistry::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let output = seen.clone();
    let binding = source.value().subscribe_with_lifecycle(
        &lifecycle,
        ObserveLifecycleMode::StartStop,
        move |counter| output.borrow_mut().push(counter.0),
    );
    assert!(seen.borrow().is_empty());
    lifecycle.move_to(LifecycleState::Started).unwrap();
    source.set(Counter(14));
    assert_eq!(*seen.borrow(), vec![13, 14]);
    lifecycle.move_to(LifecycleState::Created).unwrap();
    source.set(Counter(15));
    assert_eq!(*seen.borrow(), vec![13, 14]);
    lifecycle.move_to(LifecycleState::Started).unwrap();
    assert_eq!(*seen.borrow(), vec![13, 14, 15]);
    drop(binding);
    source.set(Counter(16));
    assert_eq!(*seen.borrow(), vec![13, 14, 15]);
    lifecycle.destroy().unwrap();

    println!(
        "identity cache: {} mapper calls before observation",
        calls.get()
    );
    println!(
        "reentrant update: {} attempts, installed {}",
        attempts.get(),
        installed.0
    );
    println!("lifecycle observations: {:?}", seen.borrow());
}
