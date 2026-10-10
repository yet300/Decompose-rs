//! Browser-independent, recursive navigation history synchronization.
//!
//! A host implements [`HistoryBackend`] using its own browser API and state
//! serialization. [`MemoryHistoryBackend`] provides the same contract in memory.
//! No DOM, JavaScript, wasm runtime, or serialization library is required.

mod adapters;
mod backend;
mod controller;
mod stack_history;
pub use adapters::{
    PanelRouteKey, pages_web_navigation, panels_web_navigation, stack_web_navigation,
};
pub use backend::{HistoryBackend, HistoryEntry, MemoryHistoryBackend};
pub use controller::HistoryController;
pub use stack_history::StackHistoryController;

use crate::value::{Subscription, Value};
use std::rc::Rc;

/// Failure in history conversion, model restoration, or host operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HistoryError {
    /// Each observed navigation must have at least one history item.
    #[error("web navigation history must not be empty")]
    EmptyHistory,
    /// A navigation cannot be its own ancestor.
    #[error("cyclic web navigation tree")]
    CyclicNavigation,
    /// An application-supplied key conversion failed.
    #[error("history route codec: {0}")]
    Codec(String),
    /// The host could not perform a requested operation.
    #[error("history backend: {0}")]
    Backend(String),
    /// The underlying router rejected a restored state.
    #[error(transparent)]
    Navigation(#[from] super::NavigationError),
}

/// Completes once the requested operation and its notifications have finished.
pub type HistoryCompletion = Box<dyn FnOnce(Result<(), HistoryError>)>;

/// Persistable key tree for one navigation level, including inactive children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryNode {
    /// Application route codec bytes; distinct keys must have distinct encodings.
    pub key: Vec<u8>,
    /// Saved path contribution, used by the legacy stack forward mapper.
    pub path: String,
    /// All history items of this key's nested navigation.
    pub children: Vec<HistoryNode>,
}

/// State stored in a backend history entry.
///
/// `index` is relative to the entry where this binding began. Host adapters must
/// distinguish this state from unrelated entries. The format is Rust-defined;
/// it does not decode Kotlin SerializableContainer or Kotlin JSON saves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryState {
    /// Relative index of this managed entry.
    pub index: usize,
    /// Nonempty root navigation history.
    pub nodes: Vec<HistoryNode>,
}

/// One encoded item with its URL contribution and optional nested navigation.
#[derive(Clone)]
pub struct HistoryItem {
    /// Encoded, stable, comparable route key.
    pub key: Vec<u8>,
    /// URL path contribution, with surrounding slashes trimmed on concatenation.
    pub path: String,
    /// Ordered query parameters; deeper children override parent values.
    pub parameters: Vec<(String, String)>,
    /// Child observed while this item is active; inactive child state is saved too.
    pub child: Option<Rc<dyn WebNavigation>>,
}

/// URL and nested-navigation contributions supplied by standard router mappers.
#[derive(Clone, Default)]
pub struct RouteInfo {
    /// URL path contribution.
    pub path: String,
    /// Ordered query parameters.
    pub parameters: Vec<(String, String)>,
    /// Optional nested navigation.
    pub child: Option<Rc<dyn WebNavigation>>,
}
impl RouteInfo {
    /// Creates a path contribution without parameters or child navigation.
    pub fn path(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            ..Self::default()
        }
    }
    fn route<K>(self, key: Rc<K>) -> Route<K> {
        Route {
            key,
            path: self.path,
            parameters: self.parameters,
            child: self.child,
        }
    }
}

/// An object-safe model bridge for heterogeneous recursive navigation trees.
pub trait WebNavigation {
    /// Reads and encodes the current nonempty history without mutating it.
    fn history(&self) -> Result<Vec<HistoryItem>, HistoryError>;
    /// Registers change notifications, without replaying an initial notification.
    fn subscribe(&self, observer: Rc<dyn Fn()>) -> Subscription;
    /// Allows or denies browser-initiated navigation. Active children run first.
    fn before_navigate(&self) -> bool {
        true
    }
    /// Allows or denies a particular encoded target; defaults to the general guard.
    fn before_navigate_to(&self, _nodes: &[HistoryNode]) -> Result<bool, HistoryError> {
        Ok(self.before_navigate())
    }
    /// Restores saved nodes; defaults to restoring their encoded keys.
    fn navigate_history(&self, nodes: Vec<HistoryNode>, completion: HistoryCompletion) {
        self.navigate(nodes.into_iter().map(|node| node.key).collect(), completion);
    }
    /// Restores encoded keys, completing after publication, including queued work.
    /// The implementation must call completion exactly once, even on failure.
    fn navigate(&self, keys: Vec<Vec<u8>>, completion: HistoryCompletion);
}

type Encoder<K> = dyn Fn(&K) -> Result<Vec<u8>, HistoryError>;
type Decoder<K> = dyn Fn(&[u8]) -> Result<K, HistoryError>;

/// Explicit route-key conversion with no Clone, Hash, or serde requirement.
pub struct RouteCodec<K> {
    encode: Rc<Encoder<K>>,
    decode: Rc<Decoder<K>>,
}
impl<K> Clone for RouteCodec<K> {
    fn clone(&self) -> Self {
        Self {
            encode: self.encode.clone(),
            decode: self.decode.clone(),
        }
    }
}
impl<K> RouteCodec<K> {
    /// Creates a deterministic codec. Equal keys must encode equally and unequal
    /// keys differently, because encoded key equality determines history changes.
    pub fn new(
        encode: impl Fn(&K) -> Result<Vec<u8>, HistoryError> + 'static,
        decode: impl Fn(&[u8]) -> Result<K, HistoryError> + 'static,
    ) -> Self {
        Self {
            encode: Rc::new(encode),
            decode: Rc::new(decode),
        }
    }
    /// Encodes one key.
    pub fn encode(&self, key: &K) -> Result<Vec<u8>, HistoryError> {
        (self.encode)(key)
    }
    /// Decodes one key.
    pub fn decode(&self, bytes: &[u8]) -> Result<K, HistoryError> {
        (self.decode)(bytes)
    }
}

/// One typed route, sharing keys without cloning configurations.
pub struct Route<K> {
    /// Key passed back to the model on browser navigation.
    pub key: Rc<K>,
    /// URL path contribution.
    pub path: String,
    /// Ordered URL query contributions.
    pub parameters: Vec<(String, String)>,
    /// Optional nested navigation.
    pub child: Option<Rc<dyn WebNavigation>>,
}
impl<K> Clone for Route<K> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            path: self.path.clone(),
            parameters: self.parameters.clone(),
            child: self.child.clone(),
        }
    }
}
impl<K> Route<K> {
    /// Creates a route without parameters or nested navigation.
    pub fn new(key: Rc<K>, path: impl Into<String>) -> Self {
        Self {
            key,
            path: path.into(),
            parameters: Vec::new(),
            child: None,
        }
    }
}

type Navigate<K> = dyn Fn(Vec<Rc<K>>, HistoryCompletion);

/// Typed bridge for any observable navigation model.
///
/// Callbacks are stored by subscriptions, so they and the keys are `'static`.
/// This adds no Clone/Hash/Send/Sync bounds. Router-specific helpers use this
/// same bridge and completion contract.
pub struct TypedWebNavigation<K> {
    routes: Value<Vec<Route<K>>>,
    codec: RouteCodec<K>,
    navigate: Box<Navigate<K>>,
    before: Box<dyn Fn() -> bool>,
}
impl<K: 'static> TypedWebNavigation<K> {
    /// Binds a typed route observable and restoration callback.
    pub fn new(
        routes: Value<Vec<Route<K>>>,
        codec: RouteCodec<K>,
        navigate: impl Fn(Vec<Rc<K>>, HistoryCompletion) + 'static,
    ) -> Self {
        Self {
            routes,
            codec,
            navigate: Box::new(navigate),
            before: Box::new(|| true),
        }
    }
    /// Sets the browser navigation guard; application-driven changes bypass it.
    pub fn with_before_navigate(mut self, before: impl Fn() -> bool + 'static) -> Self {
        self.before = Box::new(before);
        self
    }
}
impl<K: 'static> WebNavigation for TypedWebNavigation<K> {
    fn history(&self) -> Result<Vec<HistoryItem>, HistoryError> {
        let routes = self.routes.get();
        if routes.is_empty() {
            return Err(HistoryError::EmptyHistory);
        }
        routes
            .iter()
            .map(|route| {
                Ok(HistoryItem {
                    key: self.codec.encode(&route.key)?,
                    path: route.path.clone(),
                    parameters: route.parameters.clone(),
                    child: route.child.clone(),
                })
            })
            .collect()
    }
    fn subscribe(&self, observer: Rc<dyn Fn()>) -> Subscription {
        let initial = std::cell::Cell::new(true);
        self.routes.subscribe(move |_| {
            if !initial.replace(false) {
                observer();
            }
        })
    }
    fn before_navigate(&self) -> bool {
        (self.before)()
    }
    fn navigate(&self, keys: Vec<Vec<u8>>, completion: HistoryCompletion) {
        let decoded = keys
            .iter()
            .map(|key| self.codec.decode(key).map(Rc::new))
            .collect();
        match decoded {
            Ok(keys) => (self.navigate)(keys, completion),
            Err(error) => completion(Err(error)),
        }
    }
}
