//! Essenty state storage with reversible Decompose child-state consumption.
use essenty_state_keeper::{StateKeeper as EssentyStateKeeper, StateKeeperError};
type FallibleProvider = Box<dyn Fn() -> Result<Option<Vec<u8>>, StateKeeperError>>;
use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
};

/// State keeper used by component contexts.
///
/// Delegates provider registration and codecs to Essenty. Decompose keeps
/// consumed child frames here during failed construction, so retries and
/// saves cannot lose previously restored state. Convert an existing Essenty
/// keeper with `StateKeeper::from(keeper)`.
#[derive(Default)]
pub struct StateKeeper {
    inner: EssentyStateKeeper,
    returned: BTreeMap<String, Vec<u8>>,
    fallible: BTreeMap<String, FallibleProvider>,
}
impl StateKeeper {
    /// Creates an empty keeper.
    pub fn new() -> Self {
        Self::default()
    }
    /// Creates a keeper seeded with opaque restored bytes.
    pub fn with_restored(restored: BTreeMap<String, Vec<u8>>) -> Self {
        Self::from(EssentyStateKeeper::with_restored(restored))
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
        for (key, provider) in &self.fallible {
            if let Some(bytes) = provider()? {
                saved.insert(key.clone(), bytes);
            }
        }
        Ok(saved)
    }
    pub(crate) fn return_bytes(&mut self, key: String, bytes: Vec<u8>) {
        self.returned.insert(key, bytes);
    }
    /// Removes a provider, retaining unconsumed restored data.
    pub fn unregister(&mut self, key: &str) -> bool {
        self.fallible.remove(key);
        self.inner.unregister(key)
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
        self.returned.contains_key(key) || self.inner.has_consumable(key)
    }
    /// Number of unconsumed restored entries.
    pub fn pending_count(&self) -> usize {
        self.inner.pending_count() + self.returned.len()
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
            fallible: BTreeMap::new(),
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
