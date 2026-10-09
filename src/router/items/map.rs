use std::rc::Rc;
/// Small equality-keyed map. Lookups are linear; keys need neither Hash nor Ord.
pub struct ItemMap<C, V> {
    entries: Vec<(Rc<C>, V)>,
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
        }
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
        self.entries.retain_mut(|(k, v)| f(k, v));
    }
    /// Removes all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
impl<C: Eq, V> ItemMap<C, V> {
    /// Looks up an equal configuration.
    pub fn get(&self, key: &C) -> Option<&V> {
        self.entries
            .iter()
            .find(|(k, _)| k.as_ref() == key)
            .map(|(_, v)| v)
    }
    /// Whether an equal configuration exists.
    pub fn contains_key(&self, key: &C) -> bool {
        self.get(key).is_some()
    }
    /// Inserts or replaces both the equal key handle and value, returning the old value.
    pub fn insert(&mut self, key: Rc<C>, value: V) -> Option<V> {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|(k, _)| k.as_ref() == key.as_ref())
        {
            Some(std::mem::replace(entry, (key, value)).1)
        } else {
            self.entries.push((key, value));
            None
        }
    }
    /// Removes an equal configuration, returning its value.
    pub fn remove(&mut self, key: &C) -> Option<V> {
        self.entries
            .iter()
            .position(|(k, _)| k.as_ref() == key)
            .map(|i| self.entries.remove(i).1)
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
        }
    }
}
