use crate::value::Subscription;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

/// An object-safe source of shared navigation events.
pub trait NavigationSource<E> {
    /// Registers an event receiver. Custom sources may emit synchronously here.
    fn subscribe(&self, observer: Rc<dyn Fn(Rc<E>)>) -> Subscription;
}
struct Observer<E> {
    callback: Rc<dyn Fn(Rc<E>)>,
    active: Cell<bool>,
}
struct PendingEvent<E> {
    event: Rc<E>,
    on_abandon: Option<Box<dyn FnOnce()>>,
    from_diagnostic: bool,
}
struct Source<E> {
    observers: RefCell<Vec<Rc<Observer<E>>>>,
    pending: RefCell<VecDeque<PendingEvent<E>>>,
    emitting: Cell<bool>,
}
/// Synchronous event broadcasting, with registration order and nested FIFO delivery.
///
/// Events are shared without cloning their payload and never replayed. Events
/// emitted without receivers are discarded. Panics abandon queued events,
/// propagate to the caller, and leave the source usable for later events.
pub struct SimpleNavigation<E> {
    source: Rc<Source<E>>,
}
impl<E> Clone for SimpleNavigation<E> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
        }
    }
}
impl<E> Default for SimpleNavigation<E> {
    fn default() -> Self {
        Self::new()
    }
}
impl<E> SimpleNavigation<E> {
    /// Creates an empty source.
    pub fn new() -> Self {
        Self {
            source: Rc::new(Source {
                observers: RefCell::new(Vec::new()),
                pending: RefCell::new(VecDeque::new()),
                emitting: Cell::new(false),
            }),
        }
    }
    /// Emits an owned event to connected receivers.
    pub fn navigate(&self, event: E) {
        self.navigate_rc(Rc::new(event));
    }
    /// Emits an existing event, preserving its identity at every receiver.
    pub fn navigate_rc(&self, event: Rc<E>) {
        self.push_pending(event, None);
    }
    /// Emits an event whose queued-behind delivery reports failure if a
    /// preceding observer panics. The handler runs only for events still
    /// queued when the broadcast unwinds; delivered events drop it uncalled.
    /// User handlers must not panic; panics are caught and ignored to avoid
    /// aborting the active unwind.
    pub fn navigate_rc_abandonable(&self, event: Rc<E>, on_abandon: impl FnOnce() + 'static) {
        self.push_pending(event, Some(Box::new(on_abandon) as Box<dyn FnOnce()>));
    }
    fn push_pending(&self, event: Rc<E>, on_abandon: Option<Box<dyn FnOnce()>>) {
        if self.source.observers.borrow().is_empty() {
            return;
        }
        self.source.pending.borrow_mut().push_back(PendingEvent {
            event,
            on_abandon,
            from_diagnostic: crate::settings::is_reporting(),
        });
        if self.source.emitting.replace(true) {
            return;
        }
        struct Reset<'a, E>(&'a Source<E>);
        impl<E> Drop for Reset<'_, E> {
            fn drop(&mut self) {
                let abandoned = std::mem::take(&mut *self.0.pending.borrow_mut());
                self.0.emitting.set(false);
                // Only queued-behind events remain here; the in-flight event
                // was already popped. Report them as abandoned so pending
                // receipts observe Err instead of an empty Ok. Never abort an
                // active unwind from a user abandon handler.
                if std::thread::panicking() {
                    for mut pending in abandoned {
                        if let Some(on_abandon) = pending.on_abandon.take() {
                            let _ =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(on_abandon));
                        }
                        drop(pending.event);
                    }
                } else {
                    drop(abandoned);
                }
            }
        }
        let _reset = Reset(&self.source);
        loop {
            let event = self.source.pending.borrow_mut().pop_front();
            let Some(event) = event else { break };
            let observers = self.source.observers.borrow().clone();
            let deliver = || {
                for observer in observers {
                    if observer.active.get() {
                        (observer.callback)(event.event.clone());
                    }
                }
            };
            if event.from_diagnostic {
                crate::settings::without_diagnostics(deliver);
            } else {
                deliver();
            }
        }
    }
}
impl<E: 'static> NavigationSource<E> for SimpleNavigation<E> {
    fn subscribe(&self, callback: Rc<dyn Fn(Rc<E>)>) -> Subscription {
        let observer = Rc::new(Observer {
            callback,
            active: Cell::new(true),
        });
        self.source.observers.borrow_mut().push(observer.clone());
        let source = Rc::downgrade(&self.source);
        let observer = Rc::downgrade(&observer);
        Subscription::from_cancel(move || {
            if let Some(observer) = observer.upgrade() {
                observer.active.set(false);
                if let Some(source) = source.upgrade() {
                    let removed = {
                        let mut observers = source.observers.borrow_mut();
                        observers
                            .iter()
                            .position(|o| Rc::ptr_eq(o, &observer))
                            .map(|i| observers.remove(i))
                    };
                    drop(removed);
                }
            }
        })
    }
}
/// Navigation snapshots before and after one successful transition.
pub struct NavigationResult<N> {
    /// The previous committed navigation state.
    pub old_state: N,
    /// The newly committed navigation state.
    pub new_state: N,
}
impl<N: Clone> Clone for NavigationResult<N> {
    fn clone(&self) -> Self {
        Self {
            old_state: self.old_state.clone(),
            new_state: self.new_state.clone(),
        }
    }
}
