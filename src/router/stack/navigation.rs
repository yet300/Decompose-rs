use super::NavigationError;
use crate::value::Subscription;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

type Transformer<C> = dyn Fn(Vec<Rc<C>>) -> Vec<Rc<C>>;

/// Configuration snapshots before and after a successful navigation event.
pub struct NavigationResult<C> {
    /// The previous configurations in bottom-to-top order.
    pub old_stack: Vec<Rc<C>>,
    /// The committed configurations in bottom-to-top order.
    pub new_stack: Vec<Rc<C>>,
}
impl<C> Clone for NavigationResult<C> {
    fn clone(&self) -> Self {
        Self {
            old_stack: self.old_stack.clone(),
            new_stack: self.new_stack.clone(),
        }
    }
}
pub(crate) struct Event<C> {
    pub(crate) transformer: Rc<Transformer<C>>,
    pub(crate) completion: Rc<dyn Fn(Result<NavigationResult<C>, NavigationError>)>,
}
impl<C> Clone for Event<C> {
    fn clone(&self) -> Self {
        Self {
            transformer: self.transformer.clone(),
            completion: self.completion.clone(),
        }
    }
}
struct Observer<C> {
    callback: Box<dyn Fn(Event<C>)>,
    active: Cell<bool>,
}
struct Source<C> {
    observers: RefCell<Vec<Rc<Observer<C>>>>,
    pending: RefCell<VecDeque<Event<C>>>,
    emitting: Cell<bool>,
}
/// A shared, single-threaded source of stack navigation events.
///
/// Each connected stack applies the transformer to its own current snapshot
/// and reports its own result. Events with no connected stacks are discarded.
/// Nested events are delivered in FIFO order after the current broadcast.
pub struct StackNavigation<C> {
    source: Rc<Source<C>>,
}
impl<C> Clone for StackNavigation<C> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
        }
    }
}
impl<C: 'static> Default for StackNavigation<C> {
    fn default() -> Self {
        Self::new()
    }
}
impl<C: 'static> StackNavigation<C> {
    /// Creates an empty navigation source, ready to connect to child stacks.
    pub fn new() -> Self {
        Self {
            source: Rc::new(Source {
                observers: RefCell::new(Vec::new()),
                pending: RefCell::new(VecDeque::new()),
                emitting: Cell::new(false),
            }),
        }
    }
    /// Transforms the configurations of every connected stack.
    ///
    /// Panics if a connected stack rejects the resulting configurations.
    /// Use [`Self::navigate_with`] to receive a typed error instead.
    pub fn navigate(&self, f: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        self.navigate_with(f, |result| {
            if let Err(error) = result {
                panic!("stack navigation failed: {error:?}");
            }
        });
    }
    /// Transforms each connected stack and receives its committed result.
    ///
    /// Completion runs once per connected stack, after that stack publishes
    /// its updated children. A panic interrupts the current broadcast, drops
    /// queued events, and propagates; the source remains usable afterward.
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.source.pending.borrow_mut().push_back(Event {
            transformer: Rc::new(transformer),
            completion: Rc::new(completion),
        });
        if self.source.emitting.replace(true) {
            return;
        }
        struct Guard<'a, C>(&'a Source<C>);
        impl<C> Drop for Guard<'_, C> {
            fn drop(&mut self) {
                let abandoned = std::mem::take(&mut *self.0.pending.borrow_mut());
                self.0.emitting.set(false);
                drop(abandoned);
            }
        }
        let _guard = Guard(&self.source);
        loop {
            let event = self.source.pending.borrow_mut().pop_front();
            let Some(event) = event else {
                break;
            };
            let observers = self.source.observers.borrow().clone();
            for observer in observers {
                if observer.active.get() {
                    (observer.callback)(event.clone());
                }
            }
        }
    }
    pub(crate) fn subscribe(&self, callback: impl Fn(Event<C>) + 'static) -> Subscription {
        let observer = Rc::new(Observer {
            callback: Box::new(callback),
            active: Cell::new(true),
        });
        self.source.observers.borrow_mut().push(observer.clone());
        let source = Rc::downgrade(&self.source);
        let observer = Rc::downgrade(&observer);
        Subscription::from_cancel(move || {
            let Some(observer) = observer.upgrade() else {
                return;
            };
            observer.active.set(false);
            if let Some(source) = source.upgrade() {
                let removed = {
                    let mut observers = source.observers.borrow_mut();
                    observers
                        .iter()
                        .position(|entry| Rc::ptr_eq(entry, &observer))
                        .map(|index| observers.remove(index))
                };
                drop(removed);
            }
        })
    }
    /// Appends a new top configuration.
    pub fn push(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            stack.push(configuration.clone());
            stack
        });
    }
    /// Removes the top child when the stack has more than one child.
    pub fn pop(&self) {
        self.navigate(|mut stack| {
            if stack.len() > 1 {
                stack.pop();
            }
            stack
        });
    }
    /// Removes matching top children, stopping before the root.
    pub fn pop_while(&self, predicate: impl Fn(&C) -> bool + 'static) {
        self.navigate(move |mut stack| {
            while stack.len() > 1 && predicate(stack.last().unwrap()) {
                stack.pop();
            }
            stack
        });
    }
    /// Makes the zero-based child at `index` active. Out-of-range indices
    /// clamp to the current top child.
    pub fn pop_to(&self, index: usize) {
        self.navigate(move |mut stack| {
            if !stack.is_empty() {
                let index = index.min(stack.len() - 1);
                stack.truncate(index + 1);
            }
            stack
        });
    }
    /// Removes every child above the root.
    pub fn pop_to_first(&self) {
        self.pop_to(0);
    }
    /// Replaces the current top configuration.
    pub fn replace_current(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            stack.pop();
            stack.push(configuration.clone());
            stack
        });
    }
    /// Replaces the entire stack. Connected stacks reject an empty result.
    pub fn replace_all(&self, configurations: Vec<C>) {
        let configurations: Vec<_> = configurations.into_iter().map(Rc::new).collect();
        self.navigate(move |_| configurations.clone());
    }
    /// Removes configurations whose selected kind matches the new
    /// configuration, then appends the new configuration.
    pub fn bring_to_front_by<K: Eq + 'static>(
        &self,
        configuration: C,
        selector: impl Fn(&C) -> K + 'static,
    ) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            let kind = selector(&configuration);
            stack.retain(|entry| selector(entry) != kind);
            stack.push(configuration.clone());
            stack
        });
    }
}
impl<C: Eq + 'static> StackNavigation<C> {
    /// Appends unless the current top configuration is equal.
    pub fn push_new(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            if stack.last().is_none_or(|top| top != &configuration) {
                stack.push(configuration.clone());
            }
            stack
        });
    }
    /// Removes the last equal configuration, if present, then appends it.
    pub fn push_to_front(&self, configuration: C) {
        let configuration = Rc::new(configuration);
        self.navigate(move |mut stack| {
            if let Some(index) = stack.iter().rposition(|entry| entry == &configuration) {
                stack.remove(index);
            }
            stack.push(configuration.clone());
            stack
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    type Connected = (StackNavigation<u8>, Rc<RefCell<Vec<Rc<u8>>>>, Subscription);
    fn connected(initial: Vec<u8>) -> Connected {
        let navigation = StackNavigation::new();
        let stack = Rc::new(RefCell::new(
            initial.into_iter().map(Rc::new).collect::<Vec<_>>(),
        ));
        let target = stack.clone();
        let token = navigation.subscribe(move |event| {
            let old_stack = target.borrow().clone();
            let new_stack = (event.transformer)(old_stack.clone());
            *target.borrow_mut() = new_stack.clone();
            (event.completion)(Ok(NavigationResult {
                old_stack,
                new_stack,
            }));
        });
        (navigation, stack, token)
    }
    fn contents(stack: &RefCell<Vec<Rc<u8>>>) -> Vec<u8> {
        stack.borrow().iter().map(|c| **c).collect()
    }

    #[test]
    fn push_pop_and_clamped_pop_to_preserve_the_root() {
        let (navigation, stack, _token) = connected(vec![1]);
        navigation.pop();
        assert_eq!(contents(&stack), [1]);
        navigation.push(2);
        navigation.push(3);
        assert_eq!(contents(&stack), [1, 2, 3]);
        navigation.pop_to(usize::MAX);
        assert_eq!(contents(&stack), [1, 2, 3]);
        navigation.pop();
        assert_eq!(contents(&stack), [1, 2]);
        navigation.pop_to(0);
        assert_eq!(contents(&stack), [1]);
        navigation.push(4);
        navigation.pop_to_first();
        assert_eq!(contents(&stack), [1]);
    }
    #[test]
    fn pop_while_stops_at_first_nonmatching_child_and_never_removes_root() {
        let (navigation, stack, _token) = connected(vec![1, 2, 3, 4]);
        navigation.pop_while(|c| *c > 2);
        assert_eq!(contents(&stack), [1, 2]);
        navigation.pop_while(|_| true);
        assert_eq!(contents(&stack), [1]);
    }
    #[test]
    fn replace_operations_and_kind_selection_apply_the_expected_order() {
        let (navigation, stack, _token) = connected(vec![1, 2, 3]);
        navigation.replace_current(4);
        assert_eq!(contents(&stack), [1, 2, 4]);
        navigation.replace_all(vec![1, 2, 3, 4]);
        assert_eq!(contents(&stack), [1, 2, 3, 4]);
        navigation.bring_to_front_by(5, |c| *c % 2);
        assert_eq!(contents(&stack), [2, 4, 5]);
        navigation.replace_all(vec![]);
        assert!(contents(&stack).is_empty());
    }
    #[test]
    fn push_new_checks_only_top_and_push_to_front_removes_last_match() {
        let (navigation, stack, _token) = connected(vec![1, 2, 1, 3]);
        let original = stack.borrow()[0].clone();
        navigation.push_new(3);
        assert_eq!(contents(&stack), [1, 2, 1, 3]);
        navigation.push_to_front(1);
        assert_eq!(contents(&stack), [1, 2, 3, 1]);
        assert!(Rc::ptr_eq(&original, &stack.borrow()[0]));
        navigation.push_new(2);
        assert_eq!(contents(&stack), [1, 2, 3, 1, 2]);
    }

    #[test]
    fn nested_events_finish_each_broadcast_before_next_event() {
        let navigation = StackNavigation::<u8>::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        let nested = navigation.clone();
        let _first = navigation.subscribe(move |event| {
            let value = *(event.transformer)(vec![])[0];
            output.borrow_mut().push((1, value));
            if value == 1 {
                nested.navigate(|_| vec![Rc::new(2)]);
            }
        });
        let output = seen.clone();
        let _second = navigation.subscribe(move |event| {
            output
                .borrow_mut()
                .push((2, *(event.transformer)(vec![])[0]));
        });
        navigation.navigate(|_| vec![Rc::new(1)]);
        assert_eq!(*seen.borrow(), [(1, 1), (2, 1), (1, 2), (2, 2)]);
    }

    #[test]
    fn panic_discards_queued_events_and_releases_dispatch_flag() {
        let navigation = StackNavigation::<u8>::new();
        let nested = navigation.clone();
        let _first = navigation.subscribe(move |event| {
            if *(event.transformer)(vec![])[0] == 1 {
                nested.navigate(|_| vec![Rc::new(2)]);
                panic!("observer panic");
            }
        });
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        let _second = navigation
            .subscribe(move |event| output.borrow_mut().push(*(event.transformer)(vec![])[0]));
        assert!(
            catch_unwind(AssertUnwindSafe(
                || navigation.navigate(|_| vec![Rc::new(1)])
            ))
            .is_err()
        );
        navigation.navigate(|_| vec![Rc::new(3)]);
        assert_eq!(*seen.borrow(), [3]);
    }

    #[test]
    fn cancel_skips_an_observer_already_snapshotted() {
        let navigation = StackNavigation::<u8>::new();
        let pending = Rc::new(RefCell::new(None::<Subscription>));
        let cancel = pending.clone();
        let _first = navigation.subscribe(move |_| cancel.borrow().as_ref().unwrap().cancel());
        let count = Rc::new(Cell::new(0));
        let output = count.clone();
        *pending.borrow_mut() = Some(navigation.subscribe(move |_| output.set(output.get() + 1)));
        navigation.navigate(|x| x);
        assert_eq!(count.get(), 0);
    }

    #[test]
    fn multiple_subscribers_receive_independent_input_and_completion() {
        struct NoClone(u8);
        let navigation = StackNavigation::<NoClone>::default();
        let _clone = navigation.clone();
        let _first = navigation.subscribe(|event| {
            let old_stack = vec![Rc::new(NoClone(1))];
            let new_stack = (event.transformer)(old_stack.clone());
            (event.completion)(Ok(NavigationResult {
                old_stack,
                new_stack,
            }));
        });
        let _second = navigation.subscribe(|event| {
            let old_stack = vec![Rc::new(NoClone(2))];
            let new_stack = (event.transformer)(old_stack.clone());
            (event.completion)(Ok(NavigationResult {
                old_stack,
                new_stack,
            }));
        });
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        navigation.navigate_with(
            |mut stack| {
                stack.push(Rc::new(NoClone(3)));
                stack
            },
            move |result| {
                let result = result.unwrap();
                output
                    .borrow_mut()
                    .push((result.old_stack[0].0, result.new_stack[1].0));
            },
        );
        assert_eq!(*seen.borrow(), [(1, 3), (2, 3)]);
    }
    #[test]
    fn subscriber_added_in_callback_receives_only_later_events() {
        let navigation = StackNavigation::<u8>::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let output = seen.clone();
        let late = Rc::new(RefCell::new(None));
        let token_slot = late.clone();
        let source = navigation.clone();
        let _first = navigation.subscribe(move |event| {
            if *(event.transformer)(vec![])[0] == 1 {
                let output = output.clone();
                *token_slot.borrow_mut() = Some(source.subscribe(move |event| {
                    output.borrow_mut().push(*(event.transformer)(vec![])[0])
                }));
                source.push(2);
            }
        });
        navigation.push(1);
        assert_eq!(*seen.borrow(), [2]);
        navigation.push(3);
        assert_eq!(*seen.borrow(), [2, 3]);
    }
    #[test]
    fn token_does_not_retain_source_and_dropping_token_releases_observer() {
        let navigation = StackNavigation::<u8>::new();
        let capture = Rc::new(());
        let weak = Rc::downgrade(&capture);
        let token = navigation.subscribe(move |_| {
            let _ = &capture;
        });
        drop(token);
        assert!(weak.upgrade().is_none());
        let capture = Rc::new(());
        let weak = Rc::downgrade(&capture);
        let token = navigation.subscribe(move |_| {
            let _ = &capture;
        });
        drop(navigation);
        assert!(weak.upgrade().is_none());
        token.cancel();
    }
    #[test]
    fn default_completion_panics_on_failure_but_explicit_completion_receives_it() {
        let navigation = StackNavigation::<u8>::new();
        let _token =
            navigation.subscribe(|event| (event.completion)(Err(NavigationError::EmptyStack)));
        assert!(catch_unwind(AssertUnwindSafe(|| navigation.navigate(|_| vec![]))).is_err());
        let error = Rc::new(RefCell::new(None));
        let output = error.clone();
        navigation.navigate_with(
            |_| vec![],
            move |result| *output.borrow_mut() = result.err(),
        );
        assert_eq!(*error.borrow(), Some(NavigationError::EmptyStack));
    }
}
