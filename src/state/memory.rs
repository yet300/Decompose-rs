use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

pub(crate) type MemoryValue = Rc<dyn Any>;
pub(crate) type SnapshotProvider = Box<dyn Fn() -> Result<SavedEntry, String>>;
/// The two representations of one provider invocation.
#[derive(Default)]
pub(crate) struct SavedEntry {
    pub(crate) persistent: Option<Vec<u8>>,
    pub(crate) memory: Option<MemoryValue>,
}

/// Owned state for synchronous in-process recreation.
///
/// Memory values are never encoded into [`Self::persistent`] bytes. Cloning a
/// snapshot shares its immutable memory values; dropping its last owner releases
/// them. Snapshots do not own providers, lifecycles or navigation controllers.
#[derive(Clone, Default)]
pub struct StateSnapshot {
    pub(crate) persistent: BTreeMap<String, Vec<u8>>,
    pub(crate) memory: BTreeMap<String, MemoryValue>,
    pub(crate) memory_navigation: BTreeSet<String>,
}
impl StateSnapshot {
    /// Bytes that may be serialized by the host; no memory navigation is included.
    pub fn persistent(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.persistent
    }
    /// Extracts the persistent portion and releases this snapshot's memory values.
    pub fn into_persistent(self) -> BTreeMap<String, Vec<u8>> {
        self.persistent
    }
    /// Number of direct memory entries, including nested scope snapshots.
    pub fn memory_count(&self) -> usize {
        self.memory.len()
    }
    pub(crate) fn from_bytes(persistent: BTreeMap<String, Vec<u8>>) -> Self {
        Self {
            persistent,
            ..Self::default()
        }
    }
}
impl std::fmt::Debug for StateSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateSnapshot")
            .field("persistent_keys", &self.persistent.keys())
            .field("memory_keys", &self.memory.keys())
            .finish_non_exhaustive()
    }
}
