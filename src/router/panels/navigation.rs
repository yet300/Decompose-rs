use super::{ChildPanelsMode, NavigationError, Panels};
use crate::router::{
    NavigationEvent,
    children::{NavigationSource, SimpleNavigation},
};
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
/// Public panels state transformation and per-receiver completion.
pub type PanelsEvent<MC, DC, EC = Infallible> =
    NavigationEvent<Panels<MC, DC, EC>, NavigationResult<MC, DC, EC>>;

/// Standard panels operations shared by sources and custom navigators.
///
/// Implement `navigate_with` to inherit the standard helper policies.
/// Generic callbacks use static dispatch; this trait is not object-safe.
pub trait PanelsNavigator<MC: 'static, DC: 'static, EC: 'static = Infallible> {
    /// Completes per receiver after publication and state-change notification.
    fn navigate_with(
        &self,
        transformer: impl Fn(Panels<MC, DC, EC>) -> Panels<MC, DC, EC> + 'static,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    );
    /// Sets all three configurations, preserving the current mode. Reports each receiver's committed result.
    fn set_panels_with(
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
    /// Sets Details and Extra independently. Reports each receiver's committed result.
    fn set_details_extra_with(
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
    /// Sets or dismisses Extra. Reports each receiver's committed result.
    fn set_extra_with(
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
    /// Replaces Main, preserving Details and Extra. Reports each receiver's committed result.
    fn activate_main_with(
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
    /// Replaces Details, preserving Extra. Reports each receiver's committed result.
    fn activate_details_with(
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
    /// Replaces Extra, preserving Details. Reports each receiver's committed result.
    fn activate_extra_with(
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
    /// Dismisses Details without dismissing Extra. Reports each receiver's committed result.
    fn dismiss_details_with(
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
    /// Dismisses Extra without dismissing Details. Reports each receiver's committed result.
    fn dismiss_extra_with(
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
    /// Dismisses Extra first, otherwise Details, in any mode. Reports each receiver's committed result.
    fn pop_with(
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
    /// Changes lifecycle mode, retaining child identities. Reports each receiver's committed result.
    fn set_mode_with(
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
    /// Fallible counterpart of `navigate`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_navigate(
        &self,
        transformer: impl Fn(Panels<MC, DC, EC>) -> Panels<MC, DC, EC> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        crate::router::navigation::collect_results(|completion| {
            self.navigate_with(transformer, completion)
        })
    }
    /// Fallible counterpart of `set_panels`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_set_panels(
        &self,
        main: MC,
        details: Option<DC>,
        extra: Option<EC>,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        crate::router::navigation::collect_results(|completion| {
            self.set_panels_with(main, details, extra, completion)
        })
    }
    /// Fallible counterpart of `set_details_extra`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_set_details_extra(
        &self,
        details: Option<DC>,
        extra: Option<EC>,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        crate::router::navigation::collect_results(|completion| {
            self.set_details_extra_with(details, extra, completion)
        })
    }
    /// Fallible counterpart of `set_extra`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_set_extra(
        &self,
        extra: Option<EC>,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        crate::router::navigation::collect_results(|completion| {
            self.set_extra_with(extra, completion)
        })
    }
    /// Fallible counterpart of `activate_main`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_activate_main(
        &self,
        main: MC,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        crate::router::navigation::collect_results(|completion| {
            self.activate_main_with(main, completion)
        })
    }
    /// Fallible counterpart of `activate_details`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_activate_details(
        &self,
        details: DC,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        crate::router::navigation::collect_results(|completion| {
            self.activate_details_with(details, completion)
        })
    }
    /// Fallible counterpart of `activate_extra`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_activate_extra(
        &self,
        extra: EC,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        crate::router::navigation::collect_results(|completion| {
            self.activate_extra_with(extra, completion)
        })
    }
    /// Fallible counterpart of `dismiss_details`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_dismiss_details(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        crate::router::navigation::collect_results(|completion| {
            self.dismiss_details_with(completion)
        })
    }
    /// Fallible counterpart of `dismiss_extra`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_dismiss_extra(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        crate::router::navigation::collect_results(|completion| self.dismiss_extra_with(completion))
    }
    /// Fallible counterpart of `pop`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_pop(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        crate::router::navigation::collect_results(|completion| self.pop_with(completion))
    }
    /// Fallible counterpart of `set_mode`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_set_mode(
        &self,
        mode: ChildPanelsMode,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        crate::router::navigation::collect_results(|completion| {
            self.set_mode_with(mode, completion)
        })
    }
    /// Explicitly panicking compatibility operation. See `try_navigate` for errors.
    #[track_caller]
    fn navigate_or_panic(
        &self,
        transformer: impl Fn(Panels<MC, DC, EC>) -> Panels<MC, DC, EC> + 'static,
    ) {
        self.navigate_with(transformer, panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_set_panels` for errors.
    #[track_caller]
    fn set_panels_or_panic(&self, main: MC, details: Option<DC>, extra: Option<EC>) {
        self.set_panels_with(main, details, extra, panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_set_details_extra` for errors.
    #[track_caller]
    fn set_details_extra_or_panic(&self, details: Option<DC>, extra: Option<EC>) {
        self.set_details_extra_with(details, extra, panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_set_extra` for errors.
    #[track_caller]
    fn set_extra_or_panic(&self, extra: Option<EC>) {
        self.set_extra_with(extra, panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_activate_main` for errors.
    #[track_caller]
    fn activate_main_or_panic(&self, main: MC) {
        self.activate_main_with(main, panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_activate_details` for errors.
    #[track_caller]
    fn activate_details_or_panic(&self, details: DC) {
        self.activate_details_with(details, panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_activate_extra` for errors.
    #[track_caller]
    fn activate_extra_or_panic(&self, extra: EC) {
        self.activate_extra_with(extra, panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_dismiss_details` for errors.
    #[track_caller]
    fn dismiss_details_or_panic(&self) {
        self.dismiss_details_with(panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_dismiss_extra` for errors.
    #[track_caller]
    fn dismiss_extra_or_panic(&self) {
        self.dismiss_extra_with(panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_pop` for errors.
    #[track_caller]
    fn pop_or_panic(&self) {
        self.pop_with(panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_set_mode` for errors.
    #[track_caller]
    fn set_mode_or_panic(&self, mode: ChildPanelsMode) {
        self.set_mode_with(mode, panic_completion)
    }
}

/// Shared synchronous navigation; each receiver applies events to its own state.
///
/// Events are shared by `Rc`, never replayed, and discarded without receivers.
/// Nested broadcasts are FIFO. Panics abandon queued events and propagate;
/// the source remains usable afterward.
pub struct PanelsNavigation<MC, DC, EC = Infallible> {
    source: SimpleNavigation<PanelsEvent<MC, DC, EC>>,
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
    /// Creates an empty source. Events without receivers are discarded.
    pub fn new() -> Self {
        Self {
            source: SimpleNavigation::new(),
        }
    }
    /// Completes per receiver after publication and state-change notification.
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Panels<MC, DC, EC>) -> Panels<MC, DC, EC> + 'static,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        PanelsNavigator::navigate_with(self, transformer, completion)
    }
    /// Sets all three configurations, preserving the current mode. Reports each receiver's committed result.
    pub fn set_panels_with(
        &self,
        main: MC,
        details: Option<DC>,
        extra: Option<EC>,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        PanelsNavigator::set_panels_with(self, main, details, extra, completion)
    }
    /// Sets Details and Extra independently. Reports each receiver's committed result.
    pub fn set_details_extra_with(
        &self,
        details: Option<DC>,
        extra: Option<EC>,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        PanelsNavigator::set_details_extra_with(self, details, extra, completion)
    }
    /// Sets or dismisses Extra. Reports each receiver's committed result.
    pub fn set_extra_with(
        &self,
        extra: Option<EC>,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        PanelsNavigator::set_extra_with(self, extra, completion)
    }
    /// Replaces Main, preserving Details and Extra. Reports each receiver's committed result.
    pub fn activate_main_with(
        &self,
        main: MC,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        PanelsNavigator::activate_main_with(self, main, completion)
    }
    /// Replaces Details, preserving Extra. Reports each receiver's committed result.
    pub fn activate_details_with(
        &self,
        details: DC,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        PanelsNavigator::activate_details_with(self, details, completion)
    }
    /// Replaces Extra, preserving Details. Reports each receiver's committed result.
    pub fn activate_extra_with(
        &self,
        extra: EC,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        PanelsNavigator::activate_extra_with(self, extra, completion)
    }
    /// Dismisses Details without dismissing Extra. Reports each receiver's committed result.
    pub fn dismiss_details_with(
        &self,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        PanelsNavigator::dismiss_details_with(self, completion)
    }
    /// Dismisses Extra without dismissing Details. Reports each receiver's committed result.
    pub fn dismiss_extra_with(
        &self,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        PanelsNavigator::dismiss_extra_with(self, completion)
    }
    /// Dismisses Extra first, otherwise Details, in any mode. Reports each receiver's committed result.
    pub fn pop_with(
        &self,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        PanelsNavigator::pop_with(self, completion)
    }
    /// Changes lifecycle mode, retaining child identities. Reports each receiver's committed result.
    pub fn set_mode_with(
        &self,
        mode: ChildPanelsMode,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        PanelsNavigator::set_mode_with(self, mode, completion)
    }
}
impl<MC: 'static, DC: 'static, EC: 'static> PanelsNavigator<MC, DC, EC>
    for PanelsNavigation<MC, DC, EC>
{
    fn navigate_with(
        &self,
        transformer: impl Fn(Panels<MC, DC, EC>) -> Panels<MC, DC, EC> + 'static,
        completion: impl Fn(Result<NavigationResult<MC, DC, EC>, NavigationError>) + 'static,
    ) {
        let event = PanelsEvent::with_completion(transformer, completion);
        let abandon = event.clone();
        self.source
            .navigate_rc_abandonable(Rc::new(event), move || {
                abandon.complete(Err(NavigationError::ChildUnavailable))
            });
    }
}
impl<MC: 'static, DC: 'static, EC: 'static> NavigationSource<PanelsEvent<MC, DC, EC>>
    for PanelsNavigation<MC, DC, EC>
{
    fn subscribe(
        &self,
        observer: Rc<dyn Fn(Rc<PanelsEvent<MC, DC, EC>>)>,
    ) -> crate::value::Subscription {
        self.source.subscribe(observer)
    }
}
fn panic_completion<MC, DC, EC>(r: Result<NavigationResult<MC, DC, EC>, NavigationError>) {
    if let Err(e) = r {
        panic!("panels navigation failed: {e:?}")
    }
}

impl<MC: 'static, DC: 'static, EC: 'static> PanelsNavigation<MC, DC, EC> {
    /// Fallible counterpart of `navigate`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_navigate(
        &self,
        transformer: impl Fn(Panels<MC, DC, EC>) -> Panels<MC, DC, EC> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        PanelsNavigator::try_navigate(self, transformer)
    }
    /// Fallible counterpart of `set_panels`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_set_panels(
        &self,
        main: MC,
        details: Option<DC>,
        extra: Option<EC>,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        PanelsNavigator::try_set_panels(self, main, details, extra)
    }
    /// Fallible counterpart of `set_details_extra`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_set_details_extra(
        &self,
        details: Option<DC>,
        extra: Option<EC>,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        PanelsNavigator::try_set_details_extra(self, details, extra)
    }
    /// Fallible counterpart of `set_extra`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_set_extra(
        &self,
        extra: Option<EC>,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        PanelsNavigator::try_set_extra(self, extra)
    }
    /// Fallible counterpart of `activate_main`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_activate_main(
        &self,
        main: MC,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        PanelsNavigator::try_activate_main(self, main)
    }
    /// Fallible counterpart of `activate_details`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_activate_details(
        &self,
        details: DC,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        PanelsNavigator::try_activate_details(self, details)
    }
    /// Fallible counterpart of `activate_extra`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_activate_extra(
        &self,
        extra: EC,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        PanelsNavigator::try_activate_extra(self, extra)
    }
    /// Fallible counterpart of `dismiss_details`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_dismiss_details(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        PanelsNavigator::try_dismiss_details(self)
    }
    /// Fallible counterpart of `dismiss_extra`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_dismiss_extra(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        PanelsNavigator::try_dismiss_extra(self)
    }
    /// Fallible counterpart of `pop`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_pop(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        PanelsNavigator::try_pop(self)
    }
    /// Fallible counterpart of `set_mode`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_set_mode(
        &self,
        mode: ChildPanelsMode,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<MC, DC, EC>>, NavigationError>
    {
        PanelsNavigator::try_set_mode(self, mode)
    }
    /// Explicitly panicking compatibility operation. See `try_navigate` for errors.
    #[track_caller]
    pub fn navigate_or_panic(
        &self,
        transformer: impl Fn(Panels<MC, DC, EC>) -> Panels<MC, DC, EC> + 'static,
    ) {
        PanelsNavigator::navigate_or_panic(self, transformer)
    }
    /// Explicitly panicking compatibility operation. See `try_set_panels` for errors.
    #[track_caller]
    pub fn set_panels_or_panic(&self, main: MC, details: Option<DC>, extra: Option<EC>) {
        PanelsNavigator::set_panels_or_panic(self, main, details, extra)
    }
    /// Explicitly panicking compatibility operation. See `try_set_details_extra` for errors.
    #[track_caller]
    pub fn set_details_extra_or_panic(&self, details: Option<DC>, extra: Option<EC>) {
        PanelsNavigator::set_details_extra_or_panic(self, details, extra)
    }
    /// Explicitly panicking compatibility operation. See `try_set_extra` for errors.
    #[track_caller]
    pub fn set_extra_or_panic(&self, extra: Option<EC>) {
        PanelsNavigator::set_extra_or_panic(self, extra)
    }
    /// Explicitly panicking compatibility operation. See `try_activate_main` for errors.
    #[track_caller]
    pub fn activate_main_or_panic(&self, main: MC) {
        PanelsNavigator::activate_main_or_panic(self, main)
    }
    /// Explicitly panicking compatibility operation. See `try_activate_details` for errors.
    #[track_caller]
    pub fn activate_details_or_panic(&self, details: DC) {
        PanelsNavigator::activate_details_or_panic(self, details)
    }
    /// Explicitly panicking compatibility operation. See `try_activate_extra` for errors.
    #[track_caller]
    pub fn activate_extra_or_panic(&self, extra: EC) {
        PanelsNavigator::activate_extra_or_panic(self, extra)
    }
    /// Explicitly panicking compatibility operation. See `try_dismiss_details` for errors.
    #[track_caller]
    pub fn dismiss_details_or_panic(&self) {
        PanelsNavigator::dismiss_details_or_panic(self)
    }
    /// Explicitly panicking compatibility operation. See `try_dismiss_extra` for errors.
    #[track_caller]
    pub fn dismiss_extra_or_panic(&self) {
        PanelsNavigator::dismiss_extra_or_panic(self)
    }
    /// Explicitly panicking compatibility operation. See `try_pop` for errors.
    #[track_caller]
    pub fn pop_or_panic(&self) {
        PanelsNavigator::pop_or_panic(self)
    }
    /// Explicitly panicking compatibility operation. See `try_set_mode` for errors.
    #[track_caller]
    pub fn set_mode_or_panic(&self, mode: ChildPanelsMode) {
        PanelsNavigator::set_mode_or_panic(self, mode)
    }
}
