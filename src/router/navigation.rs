use super::NavigationError;
use crate::{router::children::NavigationSource, value::Subscription};
use std::rc::Rc;
type ReceiverResults<R> = Rc<std::cell::RefCell<Option<Vec<Result<R, NavigationError>>>>>;

/// Completed receiver results, or accepted FIFO work whose completion is deferred.
/// A broadcast can update several routers; it is not an all-or-nothing transaction.
#[must_use = "inspect navigation errors and retain any pending receipt"]
pub enum NavigationOutcome<R> {
    /// All completion callbacks have been released. No receivers yields an empty vector.
    Completed(Vec<R>),
    /// Work has been accepted exactly once. Do not resubmit it; inspect the receipt
    /// after the enclosing dispatch/notification finishes.
    Pending(NavigationReceipt<R>),
}

/// Owned completion observation for deferred synchronous navigation.
/// Dropping this receipt does not cancel an accepted event. No runtime is needed.
/// A custom receiver retaining an event also retains its completion capability,
/// so the receipt remains pending until that capability is released.
#[must_use = "dropping a receipt discards eventual results, not the accepted event"]
pub struct NavigationReceipt<R> {
    results: ReceiverResults<R>,
    pending: std::rc::Weak<()>,
}

impl<R> NavigationReceipt<R> {
    /// Whether a receiver/event still owns a completion callback.
    pub fn is_pending(&self) -> bool {
        self.pending.strong_count() != 0
    }

    /// Takes results once all callbacks are released. Returns `None` while
    /// pending or after results have been taken. Returns the first receiver error;
    /// successful receivers have already committed and are not rolled back.
    pub fn take_results(&self) -> Option<Result<Vec<R>, NavigationError>> {
        if self.is_pending() {
            return None;
        }
        self.results
            .borrow_mut()
            .take()
            .map(|results| results.into_iter().collect())
    }
}

impl<R> std::fmt::Debug for NavigationReceipt<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavigationReceipt")
            .field("pending", &self.is_pending())
            .finish_non_exhaustive()
    }
}

impl<R> std::fmt::Debug for NavigationOutcome<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Completed(results) => f.debug_tuple("Completed").field(&results.len()).finish(),
            Self::Pending(receipt) => f.debug_tuple("Pending").field(receipt).finish(),
        }
    }
}

pub(crate) fn collect_results<R: 'static>(
    dispatch: impl FnOnce(Box<dyn Fn(Result<R, NavigationError>)>),
) -> Result<NavigationOutcome<R>, NavigationError> {
    let results = Rc::new(std::cell::RefCell::new(Some(Vec::new())));
    let output = results.clone();
    let capability = Rc::new(());
    let receipt = NavigationReceipt {
        results,
        pending: Rc::downgrade(&capability),
    };
    dispatch(Box::new(move |result| {
        let _keep_alive = &capability;
        if let Some(results) = output.borrow_mut().as_mut() {
            results.push(result);
        }
    }));
    match receipt.take_results() {
        Some(result) => result.map(NavigationOutcome::Completed),
        None => Ok(NavigationOutcome::Pending(receipt)),
    }
}

/// A shared state transformation and per-receiver completion callback.
///
/// Cloning shares the callbacks without cloning state or result payloads.
/// Each receiver transforms its own state and completes after publication.
pub struct NavigationEvent<S, R> {
    transformer: Rc<dyn Fn(S) -> S>,
    completion: Rc<dyn Fn(Result<R, NavigationError>)>,
}
impl<S, R> Clone for NavigationEvent<S, R> {
    fn clone(&self) -> Self {
        Self {
            transformer: self.transformer.clone(),
            completion: self.completion.clone(),
        }
    }
}
impl<S, R> NavigationEvent<S, R> {
    /// Creates an event that edits owned receiver state through `&mut S`.
    /// Each receiver owns its input; no payload Clone or shared mutable borrow
    /// is needed. The edit is retained with the event for possible FIFO delivery.
    pub fn modify(edit: impl Fn(&mut S) + 'static) -> Self {
        Self::new(move |mut state| {
            edit(&mut state);
            state
        })
    }
    /// Creates an event with no completion action, including on errors.
    pub fn new(transformer: impl Fn(S) -> S + 'static) -> Self {
        Self::with_completion(transformer, |_| {})
    }
    /// Creates an event with a completion invoked separately by each receiver.
    pub fn with_completion(
        transformer: impl Fn(S) -> S + 'static,
        completion: impl Fn(Result<R, NavigationError>) + 'static,
    ) -> Self {
        Self {
            transformer: Rc::new(transformer),
            completion: Rc::new(completion),
        }
    }
    /// Applies the shared transformer to a receiver's state.
    pub fn transform(&self, state: S) -> S {
        (self.transformer)(state)
    }
    /// Reports one receiver's committed result or transition error.
    pub fn complete(&self, result: Result<R, NavigationError>) {
        (self.completion)(result)
    }
}

/// Configuration snapshots before and after a successful navigation event.
pub struct NavigationResult<C> {
    /// The previous configurations in bottom-to-top order.
    pub old_stack: C,
    /// The committed configurations in bottom-to-top order.
    pub new_stack: C,
}
impl<C: Clone> Clone for NavigationResult<C> {
    fn clone(&self) -> Self {
        Self {
            old_stack: self.old_stack.clone(),
            new_stack: self.new_stack.clone(),
        }
    }
}
pub(crate) struct Event<C> {
    pub(crate) transformer: Rc<dyn Fn(C) -> C>,
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

// Public sources use family state and result types. The engine's internal state
// adapter owns only callbacks and events; it never retains the source reference.
pub(crate) fn subscribe<S: 'static, R: 'static, I: 'static>(
    source: &(impl NavigationSource<NavigationEvent<S, R>> + ?Sized),
    receiver: Rc<dyn Fn(Event<I>)>,
    to_public: impl Fn(I) -> S + 'static,
    to_internal: impl Fn(S) -> I + 'static,
    to_result: impl Fn(NavigationResult<I>) -> R + 'static,
) -> Subscription {
    let to_public = Rc::new(to_public);
    let to_internal = Rc::new(to_internal);
    let to_result = Rc::new(to_result);
    source.subscribe(Rc::new(move |event| {
        let transform = event.clone();
        let to_public = to_public.clone();
        let to_internal = to_internal.clone();
        let to_result = to_result.clone();
        receiver(Event {
            transformer: Rc::new(move |state| to_internal(transform.transform(to_public(state)))),
            completion: Rc::new(move |result| event.complete(result.map(|r| to_result(r)))),
        });
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::stack::StackNavigation;
    use std::cell::{Cell, RefCell};
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
