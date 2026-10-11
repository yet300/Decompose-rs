//! Opaque context services. No borrow guard escapes an operation.
use super::{SharedInstances, SharedKeeper};
use crate::{InstanceKeeper, InstanceKeeperError, StateKeeper, StateKeeperError, StateSnapshot};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Failure while accessing a shared state keeper.
#[derive(Debug, thiserror::Error)]
pub enum StateAccessError {
    /// Another operation or legacy raw handle currently holds an incompatible borrow.
    #[error("state keeper is borrowed by another operation")]
    Borrowed,
    /// State registration or encoding/decoding failed.
    #[error(transparent)]
    Keeper(#[from] StateKeeperError),
}

/// Failure while accessing retained instances.
#[derive(Debug, thiserror::Error)]
pub enum InstanceAccessError {
    /// Another operation or legacy raw handle currently holds an incompatible borrow.
    #[error("instance keeper is borrowed by another operation")]
    Borrowed,
    /// Retention, duplicate key or downcast failure.
    #[error(transparent)]
    Keeper(#[from] InstanceKeeperError),
}

/// Shared state service exposing owned results rather than `RefCell` guards.
/// Save providers are synchronous; incompatible reentrant store operations
/// return [`StateAccessError::Borrowed`]. Provider panics still propagate.
#[derive(Debug, Clone)]
pub struct StateStore {
    pub(crate) inner: SharedKeeper,
}
impl Default for StateStore {
    fn default() -> Self {
        Self::from(StateKeeper::new())
    }
}
impl From<StateKeeper> for StateStore {
    fn from(keeper: StateKeeper) -> Self {
        Self {
            inner: Rc::new(RefCell::new(keeper)),
        }
    }
}
impl StateStore {
    /// Whether two handles share the same provider scope.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
    /// Registers a provider owned by the context until unregistration/teardown.
    pub fn register(
        &self,
        key: &str,
        provider: impl Fn() -> Vec<u8> + 'static,
    ) -> Result<(), StateAccessError> {
        // Pre-check under a short borrow so a rejected provider drops after
        // this call returns, outside any keeper borrow. Its Drop may reenter.
        {
            let keeper = self
                .inner
                .try_borrow()
                .map_err(|_| StateAccessError::Borrowed)?;
            if keeper.is_registered(key) {
                return Err(StateKeeperError::DuplicateKey(key.into()).into());
            }
        }
        self.inner
            .try_borrow_mut()
            .map_err(|_| StateAccessError::Borrowed)?
            .register(key, provider)?;
        Ok(())
    }
    /// Registers a fallible byte provider; encoding failures surface at save time.
    pub fn register_fallible(
        &self,
        key: &str,
        provider: impl Fn() -> Result<Vec<u8>, String> + 'static,
    ) -> Result<(), StateAccessError> {
        {
            let keeper = self
                .inner
                .try_borrow()
                .map_err(|_| StateAccessError::Borrowed)?;
            if keeper.is_registered(key) {
                return Err(StateKeeperError::DuplicateKey(key.into()).into());
            }
        }
        // StateKeeper has no fallible raw-byte hook; wrap via a value provider
        // carrying unit payload and the caller encoder.
        self.inner
            .try_borrow_mut()
            .map_err(|_| StateAccessError::Borrowed)?
            .register_value(key, || (), move |()| provider())
            .map_err(StateAccessError::Keeper)?;
        Ok(())
    }
    pub fn register_optional(
        &self,
        key: &str,
        provider: impl Fn() -> Option<Vec<u8>> + 'static,
    ) -> Result<(), StateAccessError> {
        {
            let keeper = self
                .inner
                .try_borrow()
                .map_err(|_| StateAccessError::Borrowed)?;
            if keeper.is_registered(key) {
                return Err(StateKeeperError::DuplicateKey(key.into()).into());
            }
        }
        self.inner
            .try_borrow_mut()
            .map_err(|_| StateAccessError::Borrowed)?
            .register_optional(key, provider)?;
        Ok(())
    }
    /// Unregisters a provider, preserving any unconsumed restored state.
    /// A removed provider drops after the keeper borrow is released, so its
    /// Drop may reenter this store.
    /// This guarantee applies to registrations made through Decompose's adapter.
    /// Pre-existing imported Essenty providers retain primitive drop behavior.
    pub fn unregister(&self, key: &str) -> Result<bool, StateAccessError> {
        let removed = {
            let mut guard = self
                .inner
                .try_borrow_mut()
                .map_err(|_| StateAccessError::Borrowed)?;
            guard.take_registration(key)
        };
        let existed = removed.existed;
        drop(removed);
        Ok(existed)
    }
    /// Saves the same durable bytes as the underlying keeper.
    pub fn save(&self) -> Result<BTreeMap<String, Vec<u8>>, StateAccessError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| StateAccessError::Borrowed)?
            .save()?)
    }
    /// Saves durable bytes and owned memory-only navigation snapshots.
    pub fn save_snapshot(&self) -> Result<StateSnapshot, StateAccessError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| StateAccessError::Borrowed)?
            .save_snapshot()?)
    }
    /// Consumes one restored entry as owned bytes.
    pub fn consume_bytes(&self, key: &str) -> Result<Option<Vec<u8>>, StateAccessError> {
        Ok(self
            .inner
            .try_borrow_mut()
            .map_err(|_| StateAccessError::Borrowed)?
            .consume_bytes(key))
    }
    /// Decodes an owned restored entry outside internal borrows. The decoder may
    /// borrow stack data or reenter this store. Consumption is one-shot, including
    /// decode failure, as on the legacy keeper API.
    pub fn consume_value<T>(
        &self,
        key: &str,
        decode: impl FnOnce(&[u8]) -> Result<T, String>,
    ) -> Result<Option<T>, StateAccessError> {
        self.consume_bytes(key)?
            .map(|bytes| {
                decode(&bytes).map_err(|reason| {
                    StateAccessError::Keeper(StateKeeperError::Decode {
                        key: key.into(),
                        reason,
                    })
                })
            })
            .transpose()
    }
    /// Whether a provider is registered under this key.
    pub fn is_registered(&self, key: &str) -> Result<bool, StateAccessError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| StateAccessError::Borrowed)?
            .is_registered(key))
    }
    /// Number of registered providers.
    pub fn provider_count(&self) -> Result<usize, StateAccessError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| StateAccessError::Borrowed)?
            .provider_count())
    }
    /// Whether restored input remains available.
    pub fn has_consumable(&self, key: &str) -> Result<bool, StateAccessError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| StateAccessError::Borrowed)?
            .has_consumable(key))
    }
    /// Number of restored entries still available.
    pub fn pending_count(&self) -> Result<usize, StateAccessError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| StateAccessError::Borrowed)?
            .pending_count())
    }
    /// Enables owned memory persistence for a router before constructing it.
    pub fn enable_memory_navigation(&self, key: &str) -> Result<(), StateAccessError> {
        self.inner
            .try_borrow_mut()
            .map_err(|_| StateAccessError::Borrowed)?
            .enable_memory_navigation(key)?;
        Ok(())
    }
}

/// Shared instance service. Factories and released payloads run/drop outside
/// internal borrows. Stored types are `'static` because Essenty uses `Any`.
#[derive(Debug, Clone)]
pub struct InstanceStore {
    pub(crate) inner: SharedInstances,
}
impl Default for InstanceStore {
    fn default() -> Self {
        Self::from(InstanceKeeper::new())
    }
}
impl From<InstanceKeeper> for InstanceStore {
    fn from(keeper: InstanceKeeper) -> Self {
        Self {
            inner: Rc::new(RefCell::new(keeper.into())),
        }
    }
}
impl InstanceStore {
    /// Whether two handles share the same retention scope.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
    /// Whether a value is retained under this key.
    pub fn contains(&self, key: &str) -> Result<bool, InstanceAccessError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| InstanceAccessError::Borrowed)?
            .contains(key))
    }
    /// Gets a shared instance without cloning its payload.
    pub fn get<T: 'static>(&self, key: &str) -> Result<Option<Rc<T>>, InstanceAccessError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| InstanceAccessError::Borrowed)?
            .get(key)?)
    }
    /// Gets an instance, cloning its payload when present.
    pub fn get_cloned<T: Clone + 'static>(
        &self,
        key: &str,
    ) -> Result<Option<T>, InstanceAccessError> {
        Ok(self.get::<T>(key)?.map(|value| (*value).clone()))
    }
    /// Retains a value and returns shared ownership. Rejected values drop after
    /// releasing internal borrows, even when their destructor reenters the store.
    pub fn put<T: 'static>(&self, key: &str, value: T) -> Result<Rc<T>, InstanceAccessError> {
        let mut keeper = self
            .inner
            .try_borrow_mut()
            .map_err(|_| InstanceAccessError::Borrowed)?;
        if keeper.is_destroyed() {
            return Err(InstanceKeeperError::Destroyed.into());
        }
        if keeper.contains(key) {
            return Err(InstanceKeeperError::DuplicateKey(key.into()).into());
        }
        Ok(keeper.put(key, value)?)
    }
    /// Runs a borrowed FnOnce factory outside keeper borrows. If it reentrantly
    /// installs the same key, the installed instance wins (or a type error is
    /// returned); the unused candidate drops outside the keeper borrow.
    pub fn get_or_create<T: 'static>(
        &self,
        key: &str,
        create: impl FnOnce() -> T,
    ) -> Result<Rc<T>, InstanceAccessError> {
        {
            let keeper = self
                .inner
                .try_borrow()
                .map_err(|_| InstanceAccessError::Borrowed)?;
            if keeper.is_destroyed() {
                return Err(InstanceKeeperError::Destroyed.into());
            }
            if let Some(value) = keeper.get(key)? {
                return Ok(value);
            }
        }
        let candidate = create();
        {
            let keeper = self
                .inner
                .try_borrow()
                .map_err(|_| InstanceAccessError::Borrowed)?;
            if keeper.is_destroyed() {
                return Err(InstanceKeeperError::Destroyed.into());
            }
            if let Some(value) = keeper.get(key)? {
                return Ok(value);
            }
        }
        self.put(key, candidate)
    }
    /// Removes a typed instance, leaving a mismatched value retained.
    pub fn remove<T: 'static>(&self, key: &str) -> Result<Option<Rc<T>>, InstanceAccessError> {
        Ok(self
            .inner
            .try_borrow_mut()
            .map_err(|_| InstanceAccessError::Borrowed)?
            .remove(key)?)
    }
    /// Number of retained keys.
    pub fn len(&self) -> Result<usize, InstanceAccessError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| InstanceAccessError::Borrowed)?
            .len())
    }
    /// Whether no keys are retained.
    pub fn is_empty(&self) -> Result<bool, InstanceAccessError> {
        Ok(self.len()? == 0)
    }
    /// Whether the scope has ended.
    pub fn is_destroyed(&self) -> Result<bool, InstanceAccessError> {
        Ok(self
            .inner
            .try_borrow()
            .map_err(|_| InstanceAccessError::Borrowed)?
            .is_destroyed())
    }
    /// Releases retained values outside the borrow, preserving terminal state.
    pub fn clear(&self) -> Result<(), InstanceAccessError> {
        self.release(false)
    }
    /// Ends the scope and releases retained values outside the borrow.
    pub fn destroy_all(&self) -> Result<(), InstanceAccessError> {
        self.release(true)
    }
    fn release(&self, destroy: bool) -> Result<(), InstanceAccessError> {
        let previous = {
            let mut keeper = self
                .inner
                .try_borrow_mut()
                .map_err(|_| InstanceAccessError::Borrowed)?;
            let mut empty = if destroy || keeper.is_destroyed() {
                crate::instances::InstanceScope::terminal()
            } else {
                crate::instances::InstanceScope::new()
            };
            empty.lifecycle_owned = keeper.lifecycle_owned;
            std::mem::replace(&mut *keeper, empty)
        };
        let mut cleanup = crate::cleanup::Cleanup::new();
        cleanup.run(|| drop(previous));
        cleanup.finish();
        Ok(())
    }
    pub(crate) fn destroy_shared(instances: &SharedInstances) {
        Self {
            inner: instances.clone(),
        }
        .destroy_all()
        .expect("internal instance borrow during cleanup");
    }
    pub(crate) fn release_key_shared(instances: &SharedInstances, key: &str) {
        let released = instances.borrow_mut().detach(key);
        let mut cleanup = crate::cleanup::Cleanup::new();
        cleanup.run(|| drop(released));
        cleanup.finish();
    }
}
