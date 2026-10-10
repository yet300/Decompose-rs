//! Settings for the synchronous, single-threaded Decompose core.
//!
//! Settings are local to the current thread because callbacks may own `Rc`
//! services. Router options snapshot the duplicate policy when their default
//! value is constructed; an explicitly assigned `allow_duplicates` takes
//! precedence and existing routers retain their policy. Items always requires
//! unique configurations. No OS/UI main-thread check is performed: the core
//! has no host event loop, and its `Rc` handles cannot cross threads.

use crate::{DecomposeError, router::NavigationError};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

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
        error: NavigationError,
    },
    /// Component-context construction failure.
    #[error(transparent)]
    Component(DecomposeError),
}

/// Callback for diagnostics. Returning normally never swallows the error.
pub type DecomposeErrorHandler = dyn Fn(&DecomposeDiagnostic);

/// Global defaults and diagnostic handler for the current thread.
#[derive(Clone)]
pub struct DecomposeSettings {
    /// Default for Stack, Pages and Generic `allow_duplicates` options.
    pub duplicate_configurations_enabled: bool,
    /// Receives typed errors before their normal Result/completion delivery.
    /// Defaults to printing to stderr. User panics continue to propagate.
    pub on_decompose_error: Rc<DecomposeErrorHandler>,
}

impl Default for DecomposeSettings {
    fn default() -> Self {
        Self {
            duplicate_configurations_enabled: false,
            on_decompose_error: Rc::new(|error| eprintln!("Decompose: {error}")),
        }
    }
}
impl std::fmt::Debug for DecomposeSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecomposeSettings")
            .field(
                "duplicate_configurations_enabled",
                &self.duplicate_configurations_enabled,
            )
            .finish_non_exhaustive()
    }
}

thread_local! {
    static SETTINGS: RefCell<Rc<DecomposeSettings>> = RefCell::new(Rc::new(DecomposeSettings::default()));
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

impl DecomposeSettings {
    /// Returns a snapshot without retaining a mutable settings borrow.
    #[must_use]
    pub fn current() -> Self {
        SETTINGS.with(|settings| (**settings.borrow()).clone())
    }

    /// Replaces the current thread's settings and returns the previous value.
    pub fn replace(settings: Self) -> Self {
        let old = SETTINGS.with(|slot| slot.replace(Rc::new(settings)));
        (*old).clone()
    }

    /// Updates settings from their current value. A reentrant change retries
    /// the transform with the newer value, so transforms must tolerate retries.
    /// No settings borrow is held while the user transform executes.
    pub fn update(mut transform: impl FnMut(Self) -> Self) {
        loop {
            let old = SETTINGS.with(|slot| slot.borrow().clone());
            let next = Rc::new(transform((*old).clone()));
            let replaced = SETTINGS.with(|slot| {
                let mut current = slot.borrow_mut();
                if Rc::ptr_eq(&current, &old) {
                    Some(std::mem::replace(&mut *current, next))
                } else {
                    None
                }
            });
            if replaced.is_some() {
                return;
            }
        }
    }

    /// Runs a synchronous scope with temporary settings. Nested scopes and
    /// panics restore the previous value; scope-local updates end with it.
    pub fn with<R>(settings: Self, body: impl FnOnce() -> R) -> R {
        struct Restore(Option<DecomposeSettings>);
        impl Drop for Restore {
            fn drop(&mut self) {
                if let Some(settings) = self.0.take() {
                    DecomposeSettings::replace(settings);
                }
            }
        }
        let _restore = Restore(Some(Self::replace(settings)));
        body()
    }

    /// Reports a diagnostic through the current handler. Handlers may replace
    /// settings or reenter navigation. Direct nested reports are suppressed
    /// to prevent recursive diagnostics; later queued operations report normally.
    /// A handler panic propagates and releases the recursion guard.
    pub fn report_error(error: DecomposeDiagnostic) {
        REPORTING.with(|reporting| {
            if reporting.replace(true) {
                return;
            }
            struct Reset<'a>(&'a Cell<bool>);
            impl Drop for Reset<'_> {
                fn drop(&mut self) {
                    self.0.set(false);
                }
            }
            let _reset = Reset(reporting);
            let callback = Self::current().on_decompose_error;
            callback(&error);
        });
    }
}

pub(crate) fn report_navigation_error(
    operation: DiagnosticOperation,
    key: &str,
    error: &NavigationError,
) {
    DecomposeSettings::report_error(DecomposeDiagnostic::Navigation {
        operation,
        key: key.to_owned(),
        error: error.clone(),
    });
}
