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
pub use backend::{
    HistoryBackend, HistoryDestination, HistoryEntry, HistoryLocation, MemoryHistoryBackend,
};
pub use controller::HistoryController;
pub use stack_history::StackHistoryController;

use crate::value::{Subscription, Value};
use std::rc::Rc;

/// Failure in history conversion, model restoration, or host operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HistoryError {
    /// Saved route bytes belong to an incompatible navigation schema.
    #[error("history schema mismatch: saved {0:?}")]
    VersionMismatch(HistoryVersionMismatch),
    /// Each observed navigation must have at least one history item.
    #[error("web navigation history must not be empty")]
    EmptyHistory,
    /// A navigation cannot be its own ancestor.
    #[error("cyclic web navigation tree")]
    CyclicNavigation,
    /// An application-supplied key conversion failed.
    #[error("history route codec: {0}")]
    Codec(String),
    /// Route codec failure preserving its application error object.
    #[error("history route codec: {0}")]
    CodecSource(#[source] crate::ErrorSource),
    /// The host could not perform a requested operation.
    #[error("history backend: {0}")]
    Backend(String),
    /// Host operation failure preserving its application error object.
    #[error("history backend: {0}")]
    BackendSource(#[source] crate::ErrorSource),
    /// The underlying router rejected a restored state.
    #[error(transparent)]
    Navigation(#[from] super::NavigationError),
}

impl HistoryError {
    /// Owns a route codec error for later history completion/diagnostics.
    pub fn codec_source(error: impl std::error::Error + 'static) -> Self {
        Self::CodecSource(crate::ErrorSource::new(error))
    }
    /// Owns a host error for later history completion/diagnostics.
    pub fn backend_source(error: impl std::error::Error + 'static) -> Self {
        Self::BackendSource(crate::ErrorSource::new(error))
    }
}

/// Completed history restoration, or accepted work with a deferred completion.
#[derive(Debug)]
#[must_use = "retain pending history receipts instead of resubmitting accepted work"]
pub enum HistoryOutcome {
    /// Restoration and notifications finished successfully.
    Completed,
    /// A callback/host adapter deferred completion.
    Pending(HistoryReceipt),
}

#[derive(Default)]
struct HistoryResult {
    result: std::cell::RefCell<Option<Result<(), HistoryError>>>,
    completed: std::cell::Cell<bool>,
}

/// Result observation for one accepted history operation. Drop does not cancel it.
#[must_use = "dropping a receipt discards its result, not the accepted history operation"]
pub struct HistoryReceipt {
    result: Rc<HistoryResult>,
}
impl HistoryReceipt {
    /// Whether the adapter has yet to invoke its one-shot completion.
    pub fn is_pending(&self) -> bool {
        !self.result.completed.get()
    }
    /// Takes the completed result once; `None` means pending or already taken.
    pub fn take_result(&self) -> Option<Result<(), HistoryError>> {
        self.result.result.borrow_mut().take()
    }
}
impl std::fmt::Debug for HistoryReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryReceipt")
            .field("pending", &self.is_pending())
            .finish_non_exhaustive()
    }
}
fn history_result(
    dispatch: impl FnOnce(HistoryCompletion),
) -> Result<HistoryOutcome, HistoryError> {
    let result = Rc::new(HistoryResult::default());
    let output = result.clone();
    dispatch(Box::new(move |value| {
        *output.result.borrow_mut() = Some(value);
        output.completed.set(true);
    }));
    let receipt = HistoryReceipt { result };
    match receipt.take_result() {
        Some(result) => result.map(|()| HistoryOutcome::Completed),
        None => Ok(HistoryOutcome::Pending(receipt)),
    }
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
/// `index` is relative to the managed-history boundary. Attachment preserves an
/// existing entry's index, even when the model has a different stack length. Hosts must
/// distinguish this state from unrelated entries. The format is Rust-defined;
/// it does not decode Kotlin SerializableContainer or Kotlin JSON saves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryState {
    /// Navigation schema, independent of the host's serialization format.
    /// Host adapters must map a missing field in legacy entries to `None`.
    pub version: Option<String>,
    /// Relative index of this managed entry.
    pub index: usize,
    /// Nonempty root navigation history.
    pub nodes: Vec<HistoryNode>,
}

/// Incompatible saved navigation schema, detected before route decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryVersionMismatch {
    /// Schema of the saved entry; `None` denotes unversioned history.
    pub saved: Option<String>,
    /// Schema requested by the current application.
    pub current: Option<String>,
}

/// Host-independent input to a root recreation hook. Saved route bytes are
/// intentionally excluded: recreation must use fresh state and the current URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRecreation {
    /// The incompatible schemas.
    pub mismatch: HistoryVersionMismatch,
    /// URL of the host entry requesting recreation.
    pub url: String,
}

/// Completes a root recreation, synchronously or later without an async runtime.
/// A canceled controller ignores late completion and releases its returned root.
pub type HistoryRecreationCompletion = Box<dyn FnOnce(Result<HistoryRoot, HistoryError>)>;

/// Fresh model binding returned by the host's recreation hook. The host keeps
/// ownership of its actual root component and creates a fresh context/state store.
pub struct HistoryRoot {
    pub(super) navigation: Rc<dyn WebNavigation>,
    pub(super) lifecycle: Option<essenty_lifecycle::LifecycleRegistry>,
}
impl HistoryRoot {
    /// Creates a fresh root binding with explicit host-managed lifetime.
    pub fn new(navigation: Rc<dyn WebNavigation>) -> Self {
        Self {
            navigation,
            lifecycle: None,
        }
    }
    /// Automatically cancels history when the replacement lifecycle is destroyed.
    pub fn with_lifecycle(mut self, lifecycle: &essenty_lifecycle::LifecycleRegistry) -> Self {
        self.lifecycle = Some(lifecycle.clone());
        self
    }
}

/// Policy for incompatible Back/Forward entries.
#[derive(Clone, Default)]
pub enum HistoryRecoveryPolicy {
    /// Report a typed mismatch and return to the committed host entry.
    #[default]
    Reject,
    /// Detach old model observers, then request a fresh navigation root.
    /// The host owns lifecycle destruction and fresh context/state creation.
    /// Call completion exactly once. Failed recreation reattaches the old model.
    Recreate(Rc<dyn Fn(HistoryRecreation, HistoryRecreationCompletion)>),
}

/// Optional navigation version and portable root recovery policy.
#[derive(Clone, Default)]
pub struct HistoryOptions {
    /// `None` accepts only unversioned entries; versions compare exactly.
    pub version: Option<String>,
    /// Behavior after an allowed navigation reaches incompatible saved history.
    pub recovery: HistoryRecoveryPolicy,
}
impl HistoryOptions {
    /// Validates a host-owned saved-root envelope before its payload decoder or
    /// root factory runs. Missing legacy versions are represented by `None`.
    /// On mismatch the host must use fresh state, optionally with the saved URL.
    pub fn validate_saved_version(
        &self,
        saved: Option<&str>,
    ) -> Result<(), HistoryVersionMismatch> {
        if saved == self.version.as_deref() {
            Ok(())
        } else {
            Err(HistoryVersionMismatch {
                saved: saved.map(str::to_owned),
                current: self.version.clone(),
            })
        }
    }
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
    /// Restores keys with an immediate Result when possible. Accepted deferred
    /// work returns a receipt; do not resubmit it. Object-safe and runtime-free.
    fn try_navigate(&self, keys: Vec<Vec<u8>>) -> Result<HistoryOutcome, HistoryError> {
        history_result(|completion| self.navigate(keys, completion))
    }
    /// Restores the full nested target using the same completion observation.
    fn try_navigate_history(
        &self,
        nodes: Vec<HistoryNode>,
    ) -> Result<HistoryOutcome, HistoryError> {
        history_result(|completion| self.navigate_history(nodes, completion))
    }
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
    /// Creates a route from an owned key, allocating shared storage internally.
    pub fn from_key(key: K, path: impl Into<String>) -> Self {
        Self::new(Rc::new(key), path)
    }
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
    /// Binds a truly synchronous restoration function returning Result. The
    /// function is retained with the model and must own its captures. Use `new`
    /// for sources/hosts that defer completion through FIFO or host callbacks.
    pub fn new_sync(
        routes: Value<Vec<Route<K>>>,
        codec: RouteCodec<K>,
        navigate: impl Fn(Vec<Rc<K>>) -> Result<(), HistoryError> + 'static,
    ) -> Self {
        Self::new(routes, codec, move |keys, completion| {
            completion(navigate(keys))
        })
    }
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
