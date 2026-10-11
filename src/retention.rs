use crate::{InstanceStore, SharedInstances, instances::InstanceScope};
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
            instances: Rc::new(RefCell::new(InstanceScope::new())),
        }
    }
}
impl Drop for RetainedScope {
    fn drop(&mut self) {
        let mut cleanup = crate::cleanup::Cleanup::new();
        cleanup.run(|| InstanceStore::destroy_shared(&self.instances));
        cleanup.finish();
    }
}
