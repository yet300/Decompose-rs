use decompose_rs::router::stack::*;
use decompose_rs::*;
use std::{cell::RefCell, rc::Rc};
fn root() -> DefaultComponentContext {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed).unwrap();
    DefaultComponentContext::new(lifecycle)
}
#[test]
fn pop_while_visits_root_and_preserves_nonempty_stack() {
    for initial in [vec![1], vec![1, 2], vec![1, 2, 3]] {
        for panicking_helper in [false, true] {
            let parent = root();
            let nav = StackNavigation::new();
            let stack = child_stack(
                &parent,
                &nav,
                initial.clone(),
                StackOptions::default(),
                |_, ctx| ctx,
            )
            .unwrap();
            let calls = Rc::new(RefCell::new(Vec::new()));
            let output = calls.clone();
            let predicate = move |c: &i32| {
                output.borrow_mut().push(*c);
                true
            };
            if panicking_helper {
                nav.pop_while_or_panic(predicate);
            } else {
                let _ = nav.try_pop_while(predicate).unwrap();
            }
            assert_eq!(
                *calls.borrow(),
                initial.iter().copied().rev().collect::<Vec<_>>()
            );
            assert_eq!(*stack.get().active.configuration, 1);
            assert!(stack.get().back_stack.is_empty());
        }
    }
}
#[test]
fn pop_while_stops_at_first_false() {
    let parent = root();
    let nav = StackNavigation::new();
    let stack = child_stack(
        &parent,
        &nav,
        vec![1, 2, 3],
        StackOptions::default(),
        |_, ctx| ctx,
    )
    .unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let output = calls.clone();
    let _ = nav
        .try_pop_while(move |c| {
            output.borrow_mut().push(*c);
            *c != 2
        })
        .unwrap();
    assert_eq!(*calls.borrow(), [3, 2]);
    assert_eq!(*stack.get().active.configuration, 2);
}
#[test]
fn root_predicate_panic_leaves_committed_stack_and_source_retryable() {
    for panicking_helper in [false, true] {
        let parent = root();
        let nav = StackNavigation::new();
        let stack = child_stack(
            &parent,
            &nav,
            vec![1, 2],
            StackOptions::default(),
            |_, ctx| ctx,
        )
        .unwrap();
        let old = stack.get().active.instance.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let predicate = |c: &i32| {
                assert_ne!(*c, 1, "root panic");
                true
            };
            if panicking_helper {
                nav.pop_while_or_panic(predicate);
            } else {
                let _ = nav.try_pop_while(predicate).unwrap();
            }
        }));
        assert!(result.is_err());
        assert_eq!(*stack.get().active.configuration, 2);
        assert!(Rc::ptr_eq(&old, &stack.get().active.instance));
        let _ = nav.try_pop().unwrap();
        assert_eq!(*stack.get().active.configuration, 1);
    }
}

// Shared semantic fixture usable by a Kotlin harness. These executions are Rust;
// upstream Kotlin behavior was established by source inspection, not a JVM run.
#[test]
fn shared_pop_while_trace_fixture() {
    fn numbers(value: &str) -> Vec<i32> {
        value.split(',').map(|c| c.parse().unwrap()).collect()
    }
    for line in include_str!("fixtures/pop_while.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let parts: Vec<_> = line.split('\t').collect();
        let initial = numbers(parts[0]);
        let predicate = parts[1].to_owned();
        let parent = root();
        let nav = StackNavigation::new();
        let stack = child_stack(&parent, &nav, initial, StackOptions::default(), |_, ctx| {
            ctx
        })
        .unwrap();
        let calls = Rc::new(RefCell::new(Vec::new()));
        let output = calls.clone();
        let panic_expected = predicate.starts_with("panic=");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = nav
                .try_pop_while(move |c| {
                    output.borrow_mut().push(*c);
                    if predicate == format!("panic={c}") {
                        panic!("fixture root panic");
                    }
                    predicate != format!("stop={c}")
                })
                .unwrap();
        }));
        assert_eq!(result.is_err(), panic_expected);
        assert_eq!(*calls.borrow(), numbers(parts[2]));
        assert_eq!(
            stack
                .get()
                .items()
                .map(|c| *c.configuration)
                .collect::<Vec<_>>(),
            numbers(parts[3])
        );
    }
}
