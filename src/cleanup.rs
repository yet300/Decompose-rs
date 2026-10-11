//! One protocol for library-owned cleanup boundaries.
use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
};

type Panic = Box<dyn Any + Send>;

/// Independent actions always finish. An existing unwind remains authoritative;
/// otherwise the first cleanup panic is resumed after the final action.
pub(crate) struct Cleanup {
    unwinding: bool,
    first: Option<Panic>,
}
impl Cleanup {
    pub(crate) fn new() -> Self {
        Self {
            unwinding: std::thread::panicking(),
            first: None,
        }
    }
    pub(crate) fn run(&mut self, action: impl FnOnce()) {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(action)) {
            if !self.unwinding && self.first.is_none() {
                self.first = Some(payload);
            } else {
                discard(payload);
            }
        }
    }
    pub(crate) fn finish(mut self) {
        if let Some(payload) = self.first.take() {
            resume_unwind(payload);
        }
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(payload) = self.first.take() {
            discard(payload);
        }
    }
}

fn discard(payload: Panic) {
    // Panic payloads are arbitrary user values too. A destructor may panic;
    // catch that once, then forget the replacement payload to prevent recursive
    // payload destruction from creating another unwind during cleanup.
    if let Err(replacement) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
        std::mem::forget(replacement);
    }
}
