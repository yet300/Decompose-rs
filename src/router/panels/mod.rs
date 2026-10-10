//! Typed Main, Details and Extra navigation with mode-controlled lifecycles.
mod engine;
mod navigation;
mod state;
use super::{Child, NavigationError};
use crate::{ComponentContext, value::Value};
pub use navigation::{NavigationResult, PanelsEvent, PanelsNavigation, PanelsNavigator};
pub use state::PanelsCodec;
use std::{convert::Infallible, rc::Rc};

/// Main/Details membership with Extra uninhabited.
pub type TwoPanels<MC, DC> = Panels<MC, DC, Infallible>;
/// Main/Details child snapshots, without explicit never-type parameters.
pub type ChildTwoPanels<MC, MT, DC, DT> = ChildPanels<MC, MT, DC, DT, Infallible, Infallible>;
/// Main/Details navigation source.
pub type TwoPanelNavigation<MC, DC> = PanelsNavigation<MC, DC, Infallible>;
/// Main/Details construction options.
pub type TwoPanelOptions<MC, DC> = PanelsOptions<MC, DC, Infallible>;
/// Main/Details codec; use `without_extra` or typed `without_extra_fallible`.
pub type TwoPanelCodec<MC, DC> = PanelsCodec<MC, DC, Infallible>;

/// Controls which present panels are resumed. Every present panel stays created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChildPanelsMode {
    /// Resume Extra, otherwise Details, otherwise Main.
    #[default]
    Single,
    /// Resume Main and Extra, otherwise Main and Details.
    Dual,
    /// Resume every present panel.
    Triple,
}

/// Immutable typed membership and lifecycle mode.
pub struct Panels<MC, DC, EC = Infallible> {
    /// Required Main configuration.
    pub main: Rc<MC>,
    /// Optional Details configuration, independent of Extra.
    pub details: Option<Rc<DC>>,
    /// Optional Extra configuration, independent of Details.
    pub extra: Option<Rc<EC>>,
    /// Lifecycle mode.
    pub mode: ChildPanelsMode,
}
impl<MC, DC, EC> Panels<MC, DC, EC> {
    /// Creates Main-only membership in Single mode.
    pub fn new(main: MC) -> Self {
        Self {
            main: Rc::new(main),
            details: None,
            extra: None,
            mode: ChildPanelsMode::Single,
        }
    }
}
impl<MC, DC, EC> Clone for Panels<MC, DC, EC> {
    fn clone(&self) -> Self {
        Self {
            main: self.main.clone(),
            details: self.details.clone(),
            extra: self.extra.clone(),
            mode: self.mode,
        }
    }
}

/// Observable typed child instances; snapshots retain components, not lifecycles.
pub struct ChildPanels<MC, MT, DC, DT, EC = Infallible, ET = Infallible> {
    /// Required Main child.
    pub main: Child<MC, MT>,
    /// Optional Details child.
    pub details: Option<Child<DC, DT>>,
    /// Optional Extra child.
    pub extra: Option<Child<EC, ET>>,
    /// Current lifecycle mode.
    pub mode: ChildPanelsMode,
}
impl<MC, MT, DC, DT, EC, ET> Clone for ChildPanels<MC, MT, DC, DT, EC, ET> {
    fn clone(&self) -> Self {
        Self {
            main: self.main.clone(),
            details: self.details.clone(),
            extra: self.extra.clone(),
            mode: self.mode,
        }
    }
}

type StateChanged<MC, DC, EC> = dyn Fn(&Panels<MC, DC, EC>, Option<&Panels<MC, DC, EC>>);
/// Construction options. Membership is transient without an explicit codec.
pub struct PanelsOptions<MC, DC, EC = Infallible> {
    /// Unique state-provider and retained-instance key.
    pub key: String,
    /// Automatically pop Extra or Details on unhandled Back in Single mode.
    pub handle_back_button: bool,
    /// Whole-membership serialization; mode and child state are framed separately.
    pub codec: Option<PanelsCodec<MC, DC, EC>>,
    /// Runs after publication and before event completion; initial old state is None.
    pub on_state_changed: Option<Rc<StateChanged<MC, DC, EC>>>,
}
impl<MC, DC, EC> Default for PanelsOptions<MC, DC, EC> {
    fn default() -> Self {
        Self {
            key: "DefaultChildPanels".into(),
            handle_back_button: false,
            codec: None,
            on_state_changed: None,
        }
    }
}

/// Builds three independently typed roles using the hosting context's factory.
/// Reentrant events are FIFO; dropping views does not end navigation.
#[allow(clippy::type_complexity)]
pub fn child_panels<Ctx, MC, MT, DC, DT, EC, ET>(
    context: &Ctx,
    navigation: &(impl super::children::NavigationSource<PanelsEvent<MC, DC, EC>> + ?Sized),
    initial: Panels<MC, DC, EC>,
    options: PanelsOptions<MC, DC, EC>,
    main_factory: impl Fn(&MC, Ctx) -> MT + 'static,
    details_factory: impl Fn(&DC, Ctx) -> DT + 'static,
    extra_factory: impl Fn(&EC, Ctx) -> ET + 'static,
) -> Result<Value<ChildPanels<MC, MT, DC, DT, EC, ET>>, NavigationError>
where
    Ctx: ComponentContext + 'static,
    MC: Eq + 'static,
    DC: Eq + 'static,
    EC: Eq + 'static,
    MT: 'static,
    DT: 'static,
    ET: 'static,
{
    child_panels_with_initial(
        context,
        navigation,
        || initial,
        options,
        main_factory,
        details_factory,
        extra_factory,
    )
}

/// Builds panels, evaluating `initial` once only when no saved state is restored.
/// The initializer may borrow local data and is dropped during construction.
#[allow(clippy::type_complexity)]
pub fn child_panels_with_initial<Ctx, MC, MT, DC, DT, EC, ET>(
    context: &Ctx,
    navigation: &(impl super::children::NavigationSource<PanelsEvent<MC, DC, EC>> + ?Sized),
    initial: impl FnOnce() -> Panels<MC, DC, EC>,
    options: PanelsOptions<MC, DC, EC>,
    main_factory: impl Fn(&MC, Ctx) -> MT + 'static,
    details_factory: impl Fn(&DC, Ctx) -> DT + 'static,
    extra_factory: impl Fn(&EC, Ctx) -> ET + 'static,
) -> Result<Value<ChildPanels<MC, MT, DC, DT, EC, ET>>, NavigationError>
where
    Ctx: ComponentContext + 'static,
    MC: Eq + 'static,
    DC: Eq + 'static,
    EC: Eq + 'static,
    MT: 'static,
    DT: 'static,
    ET: 'static,
{
    engine::build(
        context,
        navigation,
        initial,
        options,
        main_factory,
        details_factory,
        extra_factory,
    )
}

/// Builds Main and Details with Extra uninhabited by type.
#[allow(clippy::type_complexity)]
pub fn child_panels_without_extra<Ctx, MC, MT, DC, DT>(
    context: &Ctx,
    navigation: &(impl super::children::NavigationSource<PanelsEvent<MC, DC>> + ?Sized),
    initial: Panels<MC, DC>,
    options: PanelsOptions<MC, DC>,
    main_factory: impl Fn(&MC, Ctx) -> MT + 'static,
    details_factory: impl Fn(&DC, Ctx) -> DT + 'static,
) -> Result<Value<ChildPanels<MC, MT, DC, DT>>, NavigationError>
where
    Ctx: ComponentContext + 'static,
    MC: Eq + 'static,
    DC: Eq + 'static,
    MT: 'static,
    DT: 'static,
{
    child_panels_without_extra_with_initial(
        context,
        navigation,
        || initial,
        options,
        main_factory,
        details_factory,
    )
}

/// Builds panels, evaluating `initial` once only when no saved state is restored.
/// The initializer may borrow local data and is dropped during construction.
#[allow(clippy::type_complexity)]
pub fn child_panels_without_extra_with_initial<Ctx, MC, MT, DC, DT>(
    context: &Ctx,
    navigation: &(impl super::children::NavigationSource<PanelsEvent<MC, DC>> + ?Sized),
    initial: impl FnOnce() -> Panels<MC, DC>,
    options: PanelsOptions<MC, DC>,
    main_factory: impl Fn(&MC, Ctx) -> MT + 'static,
    details_factory: impl Fn(&DC, Ctx) -> DT + 'static,
) -> Result<Value<ChildPanels<MC, MT, DC, DT>>, NavigationError>
where
    Ctx: ComponentContext + 'static,
    MC: Eq + 'static,
    DC: Eq + 'static,
    MT: 'static,
    DT: 'static,
{
    child_panels_with_initial(
        context,
        navigation,
        initial,
        options,
        main_factory,
        details_factory,
        |never: &Infallible, _| match *never {},
    )
}
