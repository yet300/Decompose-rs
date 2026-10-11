use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    rc::Rc,
};

/// Optional equality-compatible configuration fingerprints for large item lists.
/// Equal configurations must have equal fingerprints, which must remain stable
/// while a configuration belongs to a router or map. Collisions are allowed:
/// every candidate is still compared with `Eq`.
/// Selecting this policy also promises stable equality identity, enabling reuse
/// of already validated uniqueness on unchanged updates.
pub struct ConfigurationIndex<C>(Rc<dyn Fn(&C) -> u64>);
impl<C> Clone for ConfigurationIndex<C> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<C> ConfigurationIndex<C> {
    /// Opts into stable equality identity without a Hash bound or custom key.
    /// Unchanged routing updates validate in O(n); changed lists retain the
    /// equality-only quadratic fallback because every fingerprint collides.
    pub fn equality_only() -> Self {
        Self::new(|_| 0)
    }
    /// Uses a stable fingerprint callback. Its result must agree with `Eq`.
    pub fn new(fingerprint: impl Fn(&C) -> u64 + 'static) -> Self {
        Self(Rc::new(fingerprint))
    }
    pub(crate) fn fingerprint(&self, configuration: &C) -> u64 {
        (self.0)(configuration)
    }
}
impl<C: Hash> ConfigurationIndex<C> {
    /// Uses `Hash` with Rust's default hasher. No `Clone` bound is needed.
    pub fn hashed() -> Self {
        Self::new(|configuration| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            configuration.hash(&mut hasher);
            hasher.finish()
        })
    }
}
/// Ordered equality-keyed map; keys need neither `Hash` nor `Ord`.
/// Iteration preserves first insertion order; replacing an equal key keeps its
/// position but replaces its shared handle. Equality-only fallback lookup,
/// insertion and removal are O(n); building from n distinct entries is O(n²).
/// Indexed shared handles select a candidate in expected O(1), confirmed by `Eq`. This supports
/// Eq-only configurations and deterministic navigation order. `with_index` uses
/// fingerprint buckets for expected O(1) lookup/insertion, with O(n) removal
/// and O(n²) worst-case construction under complete fingerprint collisions.
pub struct ItemMap<C, V> {
    entries: Vec<(Rc<C>, V)>,
    index: Option<ConfigurationIndex<C>>,
    buckets: HashMap<u64, Vec<usize>>,
    handles: HashMap<*const C, usize>,
    indexes_valid: bool,
}
impl<C, V> Default for ItemMap<C, V> {
    fn default() -> Self {
        Self::new()
    }
}
impl<C, V> ItemMap<C, V> {
    /// Creates an empty map.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: None,
            buckets: HashMap::new(),
            handles: HashMap::new(),
            indexes_valid: true,
        }
    }
    /// Creates an ordered map with optional indexed equality lookups.
    pub fn with_index(index: ConfigurationIndex<C>) -> Self {
        Self {
            index: Some(index),
            ..Self::new()
        }
    }
    pub(crate) fn from_unique(
        entries: Vec<(Rc<C>, V)>,
        index: Option<ConfigurationIndex<C>>,
    ) -> Self {
        let mut map = Self {
            entries,
            index,
            buckets: HashMap::new(),
            handles: HashMap::new(),
            indexes_valid: true,
        };
        map.rebuild_index();
        map
    }
    pub(crate) fn use_index(&mut self, index: Option<ConfigurationIndex<C>>) {
        self.index = index;
        self.rebuild_index();
    }
    fn rebuild_index(&mut self) {
        self.indexes_valid = false;
        self.buckets.clear();
        self.handles.clear();
        if let Some(index) = &self.index {
            for (position, (key, _)) in self.entries.iter().enumerate() {
                self.handles.insert(Rc::as_ptr(key), position);
                self.buckets
                    .entry(index.fingerprint(key))
                    .or_default()
                    .push(position);
            }
        }
        self.indexes_valid = true;
    }
    /// Number of unique keys.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Whether no entries exist.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&Rc<C>, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
    /// Payload references in insertion order, without exposing shared handles.
    pub fn iter_ref(&self) -> impl Iterator<Item = (&C, &V)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_ref(), value))
    }
    /// Configuration handles in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &Rc<C>> {
        self.entries.iter().map(|(k, _)| k)
    }
    /// Values in insertion order.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }
    /// Retains matching entries, allowing values to be updated.
    pub fn retain(&mut self, mut f: impl FnMut(&C, &mut V) -> bool) {
        // Invalidate before arbitrary callbacks; a panic may compact entries.
        self.indexes_valid = false;
        self.entries.retain_mut(|(k, v)| f(k, v));
        self.rebuild_index();
    }
    /// Removes all entries.
    pub fn clear(&mut self) {
        self.indexes_valid = false;
        self.entries.clear();
        self.buckets.clear();
        self.handles.clear();
        self.indexes_valid = true;
    }
}
impl<C: Eq, V> ItemMap<C, V> {
    /// Inserts an owned configuration, storing shared ownership internally.
    pub fn insert_owned(&mut self, key: C, value: V) -> Option<V> {
        self.insert(Rc::new(key), value)
    }
    /// Looks up an equal configuration.
    pub fn get(&self, key: &C) -> Option<&V> {
        self.position(key).map(|position| &self.entries[position].1)
    }
    fn position(&self, key: &C) -> Option<usize> {
        if !self.indexes_valid {
            return self
                .entries
                .iter()
                .position(|(stored, _)| stored.as_ref() == key);
        }
        // Shared handles choose a candidate, never replace equality. Fresh
        // allocations still reach the indexed or equality-only fallback.
        if self.index.is_some()
            && let Some(&position) = self.handles.get(&std::ptr::from_ref(key))
            && self.entries[position].0.as_ref() == key
        {
            return Some(position);
        }
        match &self.index {
            Some(index) => self
                .buckets
                .get(&index.fingerprint(key))
                .and_then(|positions| {
                    positions
                        .iter()
                        .copied()
                        .find(|&position| self.entries[position].0.as_ref() == key)
                }),
            None => self
                .entries
                .iter()
                .position(|(stored, _)| stored.as_ref() == key),
        }
    }
    /// Whether an equal configuration exists.
    pub fn contains_key(&self, key: &C) -> bool {
        self.get(key).is_some()
    }
    /// Inserts or replaces both the equal key handle and value, returning the old value.
    pub fn insert(&mut self, key: Rc<C>, value: V) -> Option<V> {
        if let Some(position) = self.position(&key) {
            if self.index.is_some() {
                self.handles.remove(&Rc::as_ptr(&self.entries[position].0));
                self.handles.insert(Rc::as_ptr(&key), position);
            }
            Some(std::mem::replace(&mut self.entries[position], (key, value)).1)
        } else {
            if let Some(index) = &self.index {
                self.buckets
                    .entry(index.fingerprint(&key))
                    .or_default()
                    .push(self.entries.len());
                self.handles.insert(Rc::as_ptr(&key), self.entries.len());
            }
            self.entries.push((key, value));
            None
        }
    }

    /// Removes an equal configuration, returning its value.
    pub fn remove(&mut self, key: &C) -> Option<V> {
        let position = self.position(key)?;
        self.indexes_valid = false;
        let removed = self.entries.remove(position).1;
        self.rebuild_index();
        Some(removed)
    }

    /// Builds a map from owned configurations; later equal keys replace earlier ones.
    pub fn from_owned(entries: impl IntoIterator<Item = (C, V)>) -> Self {
        entries.into_iter().map(|(k, v)| (Rc::new(k), v)).collect()
    }
}
impl<C: Eq, V> FromIterator<(Rc<C>, V)> for ItemMap<C, V> {
    fn from_iter<I: IntoIterator<Item = (Rc<C>, V)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}
impl<C, V: Clone> Clone for ItemMap<C, V> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            index: self.index.clone(),
            buckets: self.buckets.clone(),
            handles: self.handles.clone(),
            indexes_valid: self.indexes_valid,
        }
    }
}
