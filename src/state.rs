//! Essenty state storage with reversible Decompose child-state consumption.
use essenty_state_keeper::{StateKeeper as EssentyStateKeeper, StateKeeperError};
mod callbacks;
mod memory;
use callbacks::Callbacks;
pub(crate) use memory::MemoryValue;
pub(crate) use memory::SavedEntry;
use memory::SnapshotProvider;
pub use memory::StateSnapshot;
type FallibleProvider = Box<dyn Fn() -> Result<Option<Vec<u8>>, StateKeeperError>>;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::{Deref, DerefMut},
};

/// State keeper used by component contexts.
///
/// Delegates provider registration and codecs to Essenty. Decompose keeps
/// consumed child frames here during failed construction, so retries and
/// saves cannot lose previously restored state. Convert an existing Essenty
/// keeper with `StateKeeper::from(keeper)`.
///
/// Providers registered through this adapter can be detached before shared-store
/// borrows end. Providers already hidden in an imported [`EssentyStateKeeper`]
/// retain that primitive's destructor behavior; it has no provider-detach API.
#[derive(Default)]
pub struct StateKeeper {
    inner: EssentyStateKeeper,
    returned: BTreeMap<String, Vec<u8>>,
    fallible: Callbacks<FallibleProvider>,
    memory: BTreeMap<String, MemoryValue>,
    snapshot_providers: Callbacks<SnapshotProvider>,
    memory_navigation: BTreeSet<String>,
}

/// Ownership removed from a registration; dropping it never holds a store borrow.
pub(crate) struct RemovedRegistration {
    pub(crate) existed: bool,
    provider: Option<FallibleProvider>,
    snapshot: Option<SnapshotProvider>,
}
impl Drop for RemovedRegistration {
    fn drop(&mut self) {
        let mut cleanup = crate::cleanup::Cleanup::new();
        cleanup.run(|| drop(self.provider.take()));
        cleanup.run(|| drop(self.snapshot.take()));
        cleanup.finish();
    }
}
impl StateKeeper {
    /// Creates an empty keeper.
    pub fn new() -> Self {
        Self::default()
    }
    /// Registers a byte provider. Essenty holds the key reservation; this
    /// adapter owns captures so shared stores can detach them before dropping.
    pub fn register(
        &mut self,
        key: &str,
        provider: impl Fn() -> Vec<u8> + 'static,
    ) -> Result<(), StateKeeperError> {
        self.register_fallible(key, move || Ok(Some(provider())))
    }
    /// Registers a provider that may preserve pending input by returning None.
    pub fn register_optional(
        &mut self,
        key: &str,
        provider: impl Fn() -> Option<Vec<u8>> + 'static,
    ) -> Result<(), StateKeeperError> {
        self.register_fallible(key, move || Ok(provider()))
    }
    /// Registers a value with caller-supplied encoding, without a serde bound.
    pub fn register_value<T: 'static>(
        &mut self,
        key: &str,
        supplier: impl Fn() -> T + 'static,
        encode: impl Fn(&T) -> Result<Vec<u8>, String> + 'static,
    ) -> Result<(), StateKeeperError> {
        self.register_fallible(key, move || encode(&supplier()).map(Some))
    }
    /// Registers an optional value with caller-supplied encoding.
    pub fn register_optional_value<T: 'static>(
        &mut self,
        key: &str,
        supplier: impl Fn() -> Option<T> + 'static,
        encode: impl Fn(&T) -> Result<Vec<u8>, String> + 'static,
    ) -> Result<(), StateKeeperError> {
        self.register_fallible(key, move || {
            supplier().map(|value| encode(&value)).transpose()
        })
    }
    /// Creates a keeper seeded with opaque restored bytes.
    pub fn with_restored(restored: BTreeMap<String, Vec<u8>>) -> Self {
        Self::from(EssentyStateKeeper::with_restored(restored))
    }
    /// Restores both persistent bytes and memory-only navigation without a codec.
    pub fn from_snapshot(snapshot: StateSnapshot) -> Self {
        Self {
            memory: snapshot.memory,
            memory_navigation: snapshot.memory_navigation,
            ..Self::with_restored(snapshot.persistent)
        }
    }
    /// Enables memory-only persistence for a router key, before constructing it.
    ///
    /// Applies to Generic Navigation and all standard families. This explicit
    /// mode takes precedence over a supplied byte codec. `save()` omits the
    /// router; `save_snapshot()` retains its state and nested scopes. The host
    /// owns the resulting snapshot and passes it to `from_snapshot()`.
    pub fn enable_memory_navigation(&mut self, key: &str) -> Result<(), StateKeeperError> {
        if self.is_registered(key) {
            return Err(StateKeeperError::DuplicateKey(key.into()));
        }
        self.memory_navigation.insert(key.into());
        Ok(())
    }
    /// Saves bytes and memory values for an in-process recreation.
    pub fn save_snapshot(&self) -> Result<StateSnapshot, StateKeeperError> {
        let mut persistent = self.returned.clone();
        persistent.extend(self.inner.save()?);
        for (key, provider) in self.fallible.iter() {
            if !self.snapshot_providers.contains_key(key)
                && let Some(bytes) = provider()?
            {
                persistent.insert(key.clone(), bytes);
            }
        }
        let mut memory = self.memory.clone();
        for (key, provider) in self.snapshot_providers.iter() {
            let entry = provider().map_err(|reason| StateKeeperError::Encode {
                key: key.clone(),
                reason,
            })?;
            if let Some(bytes) = entry.persistent {
                persistent.insert(key.clone(), bytes);
            }
            if let Some(value) = entry.memory {
                memory.insert(key.clone(), value);
            }
        }
        for key in &self.memory_navigation {
            persistent.remove(key);
        }
        Ok(StateSnapshot {
            persistent,
            memory,
            memory_navigation: self.memory_navigation.clone(),
        })
    }
    pub(crate) fn memory_navigation(&self, key: &str) -> bool {
        self.memory_navigation.contains(key)
    }
    pub(crate) fn consume_memory(&mut self, key: &str) -> Option<MemoryValue> {
        self.memory.remove(key)
    }
    pub(crate) fn return_memory(&mut self, key: String, value: MemoryValue) {
        self.memory.insert(key, value);
    }
    pub(crate) fn add_snapshot_provider(
        &mut self,
        key: &str,
        provider: impl Fn() -> Result<SavedEntry, String> + 'static,
    ) {
        self.snapshot_providers
            .insert(key.into(), Box::new(provider));
    }
    /// Consumes restored bytes once, including frames returned by rollback.
    pub fn consume_bytes(&mut self, key: &str) -> Option<Vec<u8>> {
        self.returned
            .remove(key)
            .or_else(|| self.inner.consume_bytes(key))
    }
    /// Saves providers and all unconsumed restored bytes.
    pub fn save(&self) -> Result<BTreeMap<String, Vec<u8>>, StateKeeperError> {
        let mut saved = self.returned.clone();
        saved.extend(self.inner.save()?);
        for (key, provider) in self.fallible.iter() {
            if let Some(bytes) = provider()? {
                saved.insert(key.clone(), bytes);
            }
        }
        for key in &self.memory_navigation {
            saved.remove(key);
        }
        Ok(saved)
    }
    pub(crate) fn return_bytes(&mut self, key: String, bytes: Vec<u8>) {
        self.returned.insert(key, bytes);
    }
    /// Removes a provider, retaining unconsumed restored data.
    pub fn unregister(&mut self, key: &str) -> bool {
        let removed = self.take_registration(key);
        removed.existed
    }
    pub(crate) fn take_registration(&mut self, key: &str) -> RemovedRegistration {
        let provider = self.fallible.remove(key);
        let snapshot = self.snapshot_providers.remove(key);
        // Adapter-owned callbacks live in `provider`/`snapshot`. The primitive
        // removes only its inert reservation, retaining all other keys/input.
        let existed = self.inner.unregister(key);
        RemovedRegistration {
            existed,
            provider,
            snapshot,
        }
    }
    pub(crate) fn register_fallible(
        &mut self,
        key: &str,
        provider: impl Fn() -> Result<Option<Vec<u8>>, String> + 'static,
    ) -> Result<(), StateKeeperError> {
        self.inner.register_optional(key, || None)?;
        let name = key.to_owned();
        self.fallible.insert(
            key.to_owned(),
            Box::new(move || {
                provider().map_err(|reason| StateKeeperError::Encode {
                    key: name.clone(),
                    reason,
                })
            }),
        );
        Ok(())
    }
    pub(crate) fn replace_reserved(
        &mut self,
        key: &str,
        provider: impl Fn() -> Result<Option<Vec<u8>>, String> + 'static,
    ) -> Result<(), StateKeeperError> {
        self.unregister(key);
        self.register_fallible(key, provider)
    }
    /// Whether restored input remains available under this key.
    pub fn has_consumable(&self, key: &str) -> bool {
        self.memory.contains_key(key)
            || self.returned.contains_key(key)
            || self.inner.has_consumable(key)
    }
    /// Number of unconsumed restored entries.
    pub fn pending_count(&self) -> usize {
        self.inner.pending_count()
            + self.returned.len()
            + self
                .memory
                .keys()
                .filter(|key| !self.returned.contains_key(*key) && !self.inner.has_consumable(key))
                .count()
    }
    /// Decodes restored input through an explicitly supplied codec.
    pub fn consume_value<T>(
        &mut self,
        key: &str,
        decode: impl FnOnce(&[u8]) -> Result<T, String>,
    ) -> Result<Option<T>, StateKeeperError> {
        self.consume_bytes(key)
            .map(|bytes| {
                decode(&bytes).map_err(|reason| StateKeeperError::Decode {
                    key: key.into(),
                    reason,
                })
            })
            .transpose()
    }
}
impl std::fmt::Debug for StateKeeper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateKeeper")
            .field("inner", &self.inner)
            .field("returned_keys", &self.returned.keys())
            .finish()
    }
}
impl From<EssentyStateKeeper> for StateKeeper {
    fn from(inner: EssentyStateKeeper) -> Self {
        Self {
            inner,
            returned: BTreeMap::new(),
            fallible: Callbacks::default(),
            memory: BTreeMap::new(),
            snapshot_providers: Callbacks::default(),
            memory_navigation: BTreeSet::new(),
        }
    }
}
impl Deref for StateKeeper {
    type Target = EssentyStateKeeper;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl DerefMut for StateKeeper {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
