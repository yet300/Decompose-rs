use super::{ChildPanelsMode, NavigationError, Panels};
use crate::router::navigation::NavigationSource;
use std::{convert::Infallible, rc::Rc};
/// Actual receiver membership before and after a committed event.
pub struct NavigationResult<MC, DC, EC = Infallible> {
    /// Previous receiver state.
    pub old_panels: Panels<MC, DC, EC>,
    /// Committed receiver state.
    pub new_panels: Panels<MC, DC, EC>,
}
impl<MC, DC, EC> Clone for NavigationResult<MC, DC, EC> {
    fn clone(&self) -> Self {
        Self {
            old_panels: self.old_panels.clone(),
            new_panels: self.new_panels.clone(),
        }
    }
}
/// Shared synchronous navigation; each receiver applies events to its own state.
pub struct PanelsNavigation<MC, DC, EC = Infallible> {
    pub(crate) source: NavigationSource<Panels<MC, DC, EC>>,
}
impl<MC, DC, EC> Clone for PanelsNavigation<MC, DC, EC> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
        }
    }
}
impl<MC: 'static, DC: 'static, EC: 'static> Default for PanelsNavigation<MC, DC, EC> {
    fn default() -> Self {
        Self::new()
    }
}
impl<MC: 'static, DC: 'static, EC: 'static> PanelsNavigation<MC, DC, EC> {
    /// Creates a source. Events without receivers are discarded.
    pub fn new() -> Self {
        Self {
            source: NavigationSource::new(),
        }
    }
    /// Applies a transformer, panicking on transition errors.
    pub fn navigate(
        &self,
        transformer: impl Fn(Panels<MC, DC, EC>) -> Panels<MC, DC, EC> + 'static,
    ) {
        self.navigate_with(transformer, panic_completion)
    }
    /// Completes per receiver after publication and state-change notification.
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Panels<MC, DC, EC>) -> Panels<MC, DC, EC> + 'static,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        self.source.navigate_with(transformer, move |r| {
            completion(r.map(|r| NavigationResult {
                old_panels: r.old_stack,
                new_panels: r.new_stack,
            }))
        });
    }
    /// Sets all three configurations, preserving the current mode.
    pub fn set_panels(&self, main: MC, details: Option<DC>, extra: Option<EC>) {
        self.set_panels_with(main, details, extra, panic_completion)
    }
    /// Sets all three configurations, preserving the current mode. Reports each receiver's committed result.
    pub fn set_panels_with(
        &self,
        main: MC,
        details: Option<DC>,
        extra: Option<EC>,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        let main = Rc::new(main);
        let details = details.map(Rc::new);
        let extra = extra.map(Rc::new);
        self.navigate_with(
            move |mut p| {
                p.main = main.clone();
                p.details = details.clone();
                p.extra = extra.clone();
                p
            },
            completion,
        )
    }
    /// Sets Details and Extra independently.
    pub fn set_details_extra(&self, details: Option<DC>, extra: Option<EC>) {
        self.set_details_extra_with(details, extra, panic_completion)
    }
    /// Sets Details and Extra independently. Reports each receiver's committed result.
    pub fn set_details_extra_with(
        &self,
        details: Option<DC>,
        extra: Option<EC>,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        let details = details.map(Rc::new);
        let extra = extra.map(Rc::new);
        self.navigate_with(
            move |mut p| {
                p.details = details.clone();
                p.extra = extra.clone();
                p
            },
            completion,
        )
    }
    /// Sets or dismisses Extra.
    pub fn set_extra(&self, extra: Option<EC>) {
        self.set_extra_with(extra, panic_completion)
    }
    /// Sets or dismisses Extra. Reports each receiver's committed result.
    pub fn set_extra_with(
        &self,
        extra: Option<EC>,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        let extra = extra.map(Rc::new);
        self.navigate_with(
            move |mut p| {
                p.extra = extra.clone();
                p
            },
            completion,
        )
    }
    /// Replaces Main, preserving Details and Extra.
    pub fn activate_main(&self, main: MC) {
        self.activate_main_with(main, panic_completion)
    }
    /// Replaces Main, preserving Details and Extra. Reports each receiver's committed result.
    pub fn activate_main_with(
        &self,
        main: MC,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        let main = Rc::new(main);
        self.navigate_with(
            move |mut p| {
                p.main = main.clone();
                p
            },
            completion,
        )
    }
    /// Replaces Details, preserving Extra.
    pub fn activate_details(&self, details: DC) {
        self.activate_details_with(details, panic_completion)
    }
    /// Replaces Details, preserving Extra. Reports each receiver's committed result.
    pub fn activate_details_with(
        &self,
        details: DC,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        let details = Rc::new(details);
        self.navigate_with(
            move |mut p| {
                p.details = Some(details.clone());
                p
            },
            completion,
        )
    }
    /// Replaces Extra, preserving Details.
    pub fn activate_extra(&self, extra: EC) {
        self.activate_extra_with(extra, panic_completion)
    }
    /// Replaces Extra, preserving Details. Reports each receiver's committed result.
    pub fn activate_extra_with(
        &self,
        extra: EC,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        let extra = Rc::new(extra);
        self.navigate_with(
            move |mut p| {
                p.extra = Some(extra.clone());
                p
            },
            completion,
        )
    }
    /// Dismisses Details without dismissing Extra.
    pub fn dismiss_details(&self) {
        self.dismiss_details_with(panic_completion)
    }
    /// Dismisses Details without dismissing Extra. Reports each receiver's committed result.
    pub fn dismiss_details_with(
        &self,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        self.navigate_with(
            move |mut p| {
                p.details = None;
                p
            },
            completion,
        )
    }
    /// Dismisses Extra without dismissing Details.
    pub fn dismiss_extra(&self) {
        self.dismiss_extra_with(panic_completion)
    }
    /// Dismisses Extra without dismissing Details. Reports each receiver's committed result.
    pub fn dismiss_extra_with(
        &self,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        self.navigate_with(
            move |mut p| {
                p.extra = None;
                p
            },
            completion,
        )
    }
    /// Dismisses Extra first, otherwise Details, in any mode.
    pub fn pop(&self) {
        self.pop_with(panic_completion)
    }
    /// Dismisses Extra first, otherwise Details, in any mode. Reports each receiver's committed result.
    pub fn pop_with(
        &self,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        self.navigate_with(
            move |mut p| {
                if p.extra.take().is_none() {
                    p.details = None;
                }
                p
            },
            completion,
        )
    }
    /// Changes lifecycle mode, retaining child identities.
    pub fn set_mode(&self, mode: ChildPanelsMode) {
        self.set_mode_with(mode, panic_completion)
    }
    /// Changes lifecycle mode, retaining child identities. Reports each receiver's committed result.
    pub fn set_mode_with(
        &self,
        mode: ChildPanelsMode,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        self.navigate_with(
            move |mut p| {
                p.mode = mode;
                p
            },
            completion,
        )
    }
}
fn panic_completion<MC, DC, EC>(r: Result<NavigationResult<MC, DC, EC>, NavigationError>) {
    if let Err(e) = r {
        panic!("panels navigation failed: {e:?}")
    }
}
