use super::{Derived, MutableValue, Subscription, Value, ValueSource};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

struct Cache<T, R> {
    input: Rc<T>,
    output: Rc<R>,
}

struct Mapped<T, R> {
    upstream: Value<T>,
    mapper: Box<dyn Fn(&T) -> R>,
    cache: RefCell<Cache<T, R>>,
    revision: Cell<u64>,
    computing: RefCell<Vec<Rc<T>>>,
    output: MutableValue<R>,
    upstream_subscription: RefCell<Option<Subscription>>,
    connecting: Cell<bool>,
}

pub(super) fn create<T: 'static, R: 'static>(
    upstream: Value<T>,
    mapper: impl Fn(&T) -> R + 'static,
) -> Value<R> {
    let input = upstream.get();
    let output = Rc::new(mapper(&input));
    Value {
        source: ValueSource::Derived(Rc::new(Mapped {
            upstream,
            mapper: Box::new(mapper),
            output: MutableValue::from_rc(output.clone()),
            cache: RefCell::new(Cache { input, output }),
            revision: Cell::new(0),
            computing: RefCell::new(Vec::new()),
            upstream_subscription: RefCell::new(None),
            connecting: Cell::new(false),
        })),
    }
}

impl<T: 'static, R: 'static> Mapped<T, R> {
    fn map_snapshot(&self, input: Rc<T>, revision: u64) -> Rc<R> {
        {
            let cache = self.cache.borrow();
            if Rc::ptr_eq(&input, &cache.input) {
                return cache.output.clone();
            }
        }
        {
            let mut computing = self.computing.borrow_mut();
            assert!(
                !computing.iter().any(|other| Rc::ptr_eq(other, &input)),
                "recursive mapped Value::get for the same uncached input"
            );
            computing.push(input.clone());
        }
        struct ComputeGuard<'a, T> {
            computing: &'a RefCell<Vec<Rc<T>>>,
            input: Rc<T>,
        }
        impl<T> Drop for ComputeGuard<'_, T> {
            fn drop(&mut self) {
                let removed = {
                    let mut computing = self.computing.borrow_mut();
                    let index = computing
                        .iter()
                        .position(|entry| Rc::ptr_eq(entry, &self.input))
                        .unwrap();
                    computing.remove(index)
                };
                drop(removed);
            }
        }
        let _guard = ComputeGuard {
            computing: &self.computing,
            input: input.clone(),
        };
        let output = Rc::new((self.mapper)(&input));
        // The caller starts this guard before upstream.get(), which can itself
        // run mappers and reentrantly install a newer pair in a mapped chain.
        if self.revision.get() == revision {
            let previous = self.cache.replace(Cache {
                input,
                output: output.clone(),
            });
            self.revision.set(revision.wrapping_add(1));
            drop(previous);
        }
        output
    }

    fn disconnect_if_idle(&self) {
        if self.output.source.observers.borrow().is_empty() {
            let subscription = self.upstream_subscription.borrow_mut().take();
            drop(subscription);
        }
    }

    fn connect(self: &Rc<Self>) {
        if self.upstream_subscription.borrow().is_some() || self.connecting.get() {
            return;
        }
        self.connecting.set(true);
        struct ConnectGuard<'a>(&'a Cell<bool>);
        impl Drop for ConnectGuard<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let _guard = ConnectGuard(&self.connecting);
        let weak = Rc::downgrade(self);
        let subscription = self.upstream.subscribe(move |input| {
            if let Some(mapped) = weak.upgrade() {
                let revision = mapped.revision.get();
                // A prior observer in the upstream's captured emission round
                // may already have installed and exposed a newer snapshot.
                // Discard that stale delivery before it can replace the cache.
                if !Rc::ptr_eq(&input, &mapped.upstream.get()) {
                    return;
                }
                let output = mapped.map_snapshot(input, revision);
                // Do not publish an older computation over a newer reentrant cache.
                if mapped.revision.get() == revision
                    || Rc::ptr_eq(&output, &mapped.cache.borrow().output)
                {
                    mapped.output.set_rc(output);
                }
            }
        });
        *self.upstream_subscription.borrow_mut() = Some(subscription);
    }
}

impl<T: 'static, R: 'static> Derived<R> for Mapped<T, R> {
    fn get(&self) -> Rc<R> {
        let revision = self.revision.get();
        let output = self.map_snapshot(self.upstream.get(), revision);
        let is_current = Rc::ptr_eq(&output, &self.cache.borrow().output);
        if is_current && !Rc::ptr_eq(&output, &self.output.get()) {
            self.output.set_rc(output.clone());
        }
        output
    }

    fn subscribe(self: Rc<Self>, callback: Box<dyn Fn(Rc<R>)>) -> Subscription {
        let weak = Rc::downgrade(&self);
        // Roll back a newly connected upstream if downstream replay panics.
        let cleanup = Subscription::from_cancel(move || {
            if let Some(mapped) = weak.upgrade() {
                mapped.disconnect_if_idle();
            }
        });
        self.connect();
        // A mapper may reentrantly subscribe after replacing its upstream.
        // Refresh before replay rather than exposing the output being superseded.
        self.get();
        let token = self.output.value().subscribe(callback);
        let slot = Rc::new(RefCell::new(Some(token)));
        let cancel_slot = slot.clone();
        let weak = Rc::downgrade(&self);
        let result = Subscription::with_detach(
            move || {
                // Removing the downstream callback can run a panicking capture
                // destructor. Disconnect idle upstream observation on unwind too.
                let _cleanup = Subscription::from_cancel(move || {
                    if let Some(mapped) = weak.upgrade() {
                        mapped.disconnect_if_idle();
                    }
                });
                let token = cancel_slot.borrow_mut().take();
                drop(token);
            },
            move || {
                let token = slot.borrow_mut().take();
                if let Some(token) = token {
                    token.detach();
                }
            },
        );
        cleanup.detach();
        result
    }
}
