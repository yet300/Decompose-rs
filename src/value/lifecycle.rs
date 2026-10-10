use super::{Subscription, Value};
use essenty_lifecycle::{LifecycleRegistry, LifecycleState};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// The lifecycle interval during which a value observer receives snapshots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObserveLifecycleMode {
    /// Observe from creation until terminal destruction.
    CreateDestroy,
    /// Observe while started or resumed; closing on stop.
    #[default]
    StartStop,
    /// Observe only while resumed; closing on pause.
    ResumePause,
}

impl ObserveLifecycleMode {
    fn includes(self, state: LifecycleState) -> bool {
        state != LifecycleState::Destroyed
            && state
                >= match self {
                    Self::CreateDestroy => LifecycleState::Created,
                    Self::StartStop => LifecycleState::Started,
                    Self::ResumePause => LifecycleState::Resumed,
                }
    }
}

type Callback<T> = Rc<dyn Fn(Rc<T>)>;

struct Binding<T> {
    value: RefCell<Option<Value<T>>>,
    observer: RefCell<Option<Callback<T>>>,
    lifecycle: Weak<LifecycleRegistry>,
    detached_lifecycle: RefCell<Option<Rc<LifecycleRegistry>>>,
    mode: ObserveLifecycleMode,
    active: Cell<bool>,
    open: Cell<bool>,
    generation: Cell<u64>,
    initializing: Cell<bool>,
    source_subscription: RefCell<Option<Subscription>>,
    lifecycle_subscription: RefCell<Option<essenty_lifecycle::Subscription>>,
}

impl<T: 'static> Binding<T> {
    fn close(&self) {
        self.open.set(false);
        self.generation.set(self.generation.get().wrapping_add(1));
        let source = self.source_subscription.borrow_mut().take();
        drop(source);
    }

    fn cancel(&self) {
        if !self.active.replace(false) {
            return;
        }
        self.close();
        let listener = self.lifecycle_subscription.borrow_mut().take();
        let observer = self.observer.borrow_mut().take();
        let value = self.value.borrow_mut().take();
        let lifecycle = self.detached_lifecycle.borrow_mut().take();
        // Never drop user-owned captures under a binding or registry borrow.
        drop(listener);
        drop(observer);
        drop(value);
        drop(lifecycle);
    }

    fn reconcile(self: &Rc<Self>) {
        if self.initializing.get() || !self.active.get() {
            return;
        }
        let Some(lifecycle) = self.lifecycle.upgrade() else {
            self.cancel();
            return;
        };
        let state = lifecycle.state();
        if state == LifecycleState::Destroyed {
            self.cancel();
            return;
        }
        if !self.mode.includes(state) {
            self.close();
            return;
        }
        if self.open.replace(true) {
            return;
        }
        let generation = self.generation.get();
        struct OpenGuard<'a, T: 'static> {
            binding: &'a Binding<T>,
            generation: u64,
            completed: bool,
        }
        impl<T: 'static> Drop for OpenGuard<'_, T> {
            fn drop(&mut self) {
                if !self.completed && self.binding.generation.get() == self.generation {
                    self.binding.close();
                }
            }
        }
        let mut guard = OpenGuard {
            binding: self,
            generation,
            completed: false,
        };
        let value = self.value.borrow().as_ref().unwrap().clone();
        let weak = Rc::downgrade(self);
        let token = value.subscribe(move |snapshot| {
            let Some(binding) = weak.upgrade() else {
                return;
            };
            if binding.generation.get() != generation {
                return;
            }
            // Essenty snapshots callbacks; a prior callback may already have
            // changed the registry while this callback awaits delivery.
            let Some(lifecycle) = binding.lifecycle.upgrade() else {
                binding.cancel();
                return;
            };
            let state = lifecycle.state();
            if state == LifecycleState::Destroyed {
                binding.cancel();
                return;
            }
            if !binding.active.get() || !binding.open.get() || !binding.mode.includes(state) {
                binding.close();
                return;
            }
            let observer = binding.observer.borrow().clone();
            if let Some(observer) = observer {
                observer(snapshot);
            }
        });
        // Replay may have closed or reopened the gate before its token exists.
        if self.active.get() && self.open.get() && self.generation.get() == generation {
            let previous = self.source_subscription.replace(Some(token));
            drop(previous);
        } else {
            drop(token);
        }
        guard.completed = true;
    }
}

pub(super) fn subscribe<T: 'static>(
    value: Value<T>,
    lifecycle: &LifecycleRegistry,
    mode: ObserveLifecycleMode,
    observer: impl Fn(Rc<T>) + 'static,
) -> Subscription {
    if lifecycle.state() == LifecycleState::Destroyed {
        drop(observer);
        return Subscription::from_cancel(|| {});
    }
    let witness = Rc::new(lifecycle.clone());
    let binding = Rc::new(Binding {
        value: RefCell::new(Some(value)),
        observer: RefCell::new(Some(Rc::new(observer))),
        lifecycle: Rc::downgrade(&witness),
        detached_lifecycle: RefCell::new(None),
        mode,
        active: Cell::new(true),
        open: Cell::new(false),
        generation: Cell::new(0),
        initializing: Cell::new(true),
        source_subscription: RefCell::new(None),
        lifecycle_subscription: RefCell::new(None),
    });
    let cancellation_binding = binding.clone();
    let cancellation_witness = witness.clone();
    let detached_binding = binding.clone();
    let token = Subscription::with_detach(
        move || {
            let _witness = cancellation_witness;
            cancellation_binding.cancel();
        },
        move || {
            if detached_binding.active.get() {
                *detached_binding.detached_lifecycle.borrow_mut() = Some(witness);
            }
        },
    );
    let listener_binding = binding.clone();
    // Essenty replay happens before its token is returned. Suppress user code
    // until that token is safely owned, then read the actual current state.
    let listener = lifecycle.subscribe(move |_| listener_binding.reconcile());
    *binding.lifecycle_subscription.borrow_mut() = Some(listener);
    binding.initializing.set(false);
    binding.reconcile();
    token
}
