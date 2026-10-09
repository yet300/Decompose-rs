use crate::{InstanceKeeper, SharedInstances};
use std::{cell::RefCell, rc::Rc};

/// Owned only by its parent keeper. Public child handles reference the nested
/// keeper rather than this owner, so releasing the parent entry ends the
/// nested scope even when components retain shared handles to it.
pub(crate) struct RetainedScope {
    pub(crate) instances: SharedInstances,
}
impl RetainedScope {
    pub(crate) fn new() -> Self {
        Self {
            instances: Rc::new(RefCell::new(InstanceKeeper::new())),
        }
    }
}
impl Drop for RetainedScope {
    fn drop(&mut self) {
        if let Ok(mut instances) = self.instances.try_borrow_mut() {
            instances.destroy_all();
        }
    }
}
