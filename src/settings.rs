//! Explicit configuration for the synchronous, single-threaded Decompose core.
//!
//! Router options snapshot the duplicate policy when constructed with
//! [`DecomposeConfig`]; an explicitly assigned `allow_duplicates` takes
//! precedence and existing routers retain their policy. Items always requires
//! unique configurations. No OS/UI main-thread check is performed: the core
//! has no host event loop, and its `Rc` handles cannot cross threads.

use crate::{DecomposeError, router::NavigationError};
use std::{cell::Cell, rc::Rc};

/// Operation that produced a typed diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticOperation {
    /// Initial construction, including restoration.
    Construction,
    /// An explicit navigation event.
    Navigation,
    /// An automatic Back action.
    Back,
    /// Saving navigation state.
    Save,
}

/// Diagnostic observation; the original operation still returns its error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecomposeDiagnostic {
    /// Navigation failure with the hosting router's stable registration key.
    #[error("{operation:?} in router '{key}': {error}")]
    Navigation {
        /// Operation that failed.
        operation: DiagnosticOperation,
        /// Hosting router key.
        key: String,
        /// Original typed failure.
        #[source]
        error: NavigationError,
    },
    /// Component-context construction failure.
    #[error(transparent)]
    Component(DecomposeError),
}

/// Callback for diagnostics. Returning normally never swallows the error.
pub type DecomposeErrorHandler = dyn Fn(&DecomposeDiagnostic);

/// Explicit, owned policy and diagnostics for a context and its descendants.
/// Defaults are deterministic and silent. No global settings are read.
#[derive(Clone, Default)]
pub struct DecomposeConfig {
    /// Default duplicate policy for Stack, Pages and Generic option constructors.
    pub duplicate_configurations_enabled: bool,
    on_error: Option<Rc<DecomposeErrorHandler>>,
}

impl DecomposeConfig {
    /// Installs an opt-in diagnostic hook, retained for the context's lifetime.
    /// The callback must own its captures because future events may invoke it.
    #[must_use]
    pub fn with_error_handler(mut self, handler: impl Fn(&DecomposeDiagnostic) + 'static) -> Self {
        self.on_error = Some(Rc::new(handler));
        self
    }

    pub(crate) fn report(&self, error: DecomposeDiagnostic) {
        if let Some(handler) = &self.on_error
            && !is_reporting()
        {
            without_diagnostics(|| handler(&error));
        }
    }
}

impl std::fmt::Debug for DecomposeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecomposeConfig")
            .field(
                "duplicate_configurations_enabled",
                &self.duplicate_configurations_enabled,
            )
            .field("has_error_handler", &self.on_error.is_some())
            .finish()
    }
}

thread_local! {
    static REPORTING: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn is_reporting() -> bool {
    REPORTING.with(Cell::get)
}

pub(crate) fn without_diagnostics<R>(body: impl FnOnce() -> R) -> R {
    REPORTING.with(|reporting| {
        struct Restore<'a>(&'a Cell<bool>, bool);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                self.0.set(self.1);
            }
        }
        let _restore = Restore(reporting, reporting.replace(true));
        body()
    })
}

pub(crate) fn report_navigation_error(
    config: Option<&DecomposeConfig>,
    operation: DiagnosticOperation,
    key: &str,
    error: &NavigationError,
) {
    let Some(config) = config else {
        return;
    };
    config.report(DecomposeDiagnostic::Navigation {
        operation,
        key: key.to_owned(),
        error: error.clone(),
    });
}
