//! Ownership retained beside Essenty's primitive registry so releases can run
//! user destructors independently, outside every shared-store borrow.
use crate::{InstanceKeeper, InstanceKeeperError, cleanup::Cleanup};
use std::{any::Any, collections::BTreeMap, rc::Rc};

#[derive(Default)]
pub(crate) struct InstanceScope {
    keeper: InstanceKeeper,
    owned: BTreeMap<String, Rc<dyn Any>>,
    pub(crate) lifecycle_owned: bool,
}
impl std::fmt::Debug for InstanceScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.keeper.fmt(f)
    }
}
impl From<InstanceKeeper> for InstanceScope {
    fn from(keeper: InstanceKeeper) -> Self {
        Self {
            keeper,
            owned: BTreeMap::new(),
            lifecycle_owned: false,
        }
    }
}
impl InstanceScope {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn terminal() -> Self {
        let mut scope = Self::new();
        scope.keeper.destroy_all();
        scope
    }
    pub(crate) fn len(&self) -> usize {
        self.keeper.len()
    }
    pub(crate) fn is_destroyed(&self) -> bool {
        self.keeper.is_destroyed()
    }
    pub(crate) fn contains(&self, key: &str) -> bool {
        self.keeper.contains(key)
    }
    pub(crate) fn get<T: 'static>(&self, key: &str) -> Result<Option<Rc<T>>, InstanceKeeperError> {
        self.keeper.get(key)
    }
    pub(crate) fn put<T: 'static>(
        &mut self,
        key: &str,
        value: T,
    ) -> Result<Rc<T>, InstanceKeeperError> {
        let value = self.keeper.put(key, value)?;
        self.owned.insert(key.to_owned(), value.clone());
        Ok(value)
    }
    pub(crate) fn get_or_create<T: 'static>(
        &mut self,
        key: &str,
        create: impl FnOnce() -> T,
    ) -> Result<Rc<T>, InstanceKeeperError> {
        let value = self.keeper.get_or_create(key, create)?;
        self.owned
            .entry(key.to_owned())
            .or_insert_with(|| value.clone());
        Ok(value)
    }
    pub(crate) fn remove<T: 'static>(
        &mut self,
        key: &str,
    ) -> Result<Option<Rc<T>>, InstanceKeeperError> {
        let value = self.keeper.remove(key)?;
        self.owned.remove(key);
        Ok(value)
    }
    pub(crate) fn detach(&mut self, key: &str) -> Option<Rc<dyn Any>> {
        // The extra strong reference prevents arbitrary Drop under this borrow.
        let owned = self.owned.remove(key);
        if owned.is_some() {
            self.keeper.destroy(key);
        }
        owned
    }
}
impl Drop for InstanceScope {
    fn drop(&mut self) {
        let mut cleanup = Cleanup::new();
        let keeper = std::mem::take(&mut self.keeper);
        cleanup.run(|| drop(keeper));
        let owned = std::mem::take(&mut self.owned);
        for (_, value) in owned {
            cleanup.run(|| drop(value));
        }
        cleanup.finish();
    }
}
