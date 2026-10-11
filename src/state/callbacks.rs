//! Independently released callbacks; map field destruction cannot double panic.
use crate::cleanup::Cleanup;
use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
};

pub(super) struct Callbacks<T>(BTreeMap<String, T>);
impl<T> Default for Callbacks<T> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}
impl<T> Deref for Callbacks<T> {
    type Target = BTreeMap<String, T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T> DerefMut for Callbacks<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl<T> Drop for Callbacks<T> {
    fn drop(&mut self) {
        let mut cleanup = Cleanup::new();
        while let Some((_, callback)) = self.0.pop_first() {
            cleanup.run(|| drop(callback));
        }
        cleanup.finish();
    }
}
