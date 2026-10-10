use super::{
    ChildPanels, ChildPanelsMode, NavigationError, Panels, PanelsEvent, PanelsOptions, state,
};
use crate::{
    ComponentContext,
    router::{
        Child,
        engine::{Model, SavedState},
        pages::{PageChild, PageStatus},
    },
    value::Value,
};
use std::{collections::BTreeMap, rc::Rc};

enum RoleConfig<MC, DC, EC> {
    Main(Rc<MC>),
    Details(Rc<DC>),
    Extra(Rc<EC>),
}
impl<MC: Eq, DC: Eq, EC: Eq> PartialEq for RoleConfig<MC, DC, EC> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Main(a), Self::Main(b)) => a == b,
            (Self::Details(a), Self::Details(b)) => a == b,
            (Self::Extra(a), Self::Extra(b)) => a == b,
            _ => false,
        }
    }
}
impl<MC: Eq, DC: Eq, EC: Eq> Eq for RoleConfig<MC, DC, EC> {}
enum RoleComponent<MT, DT, ET> {
    Main(Rc<MT>),
    Details(Rc<DT>),
    Extra(Rc<ET>),
}
struct PanelsModel<MC, DC, EC>(PanelsOptions<MC, DC, EC>);
impl<MC: Eq + 'static, DC: Eq + 'static, EC: Eq + 'static, MT: 'static, DT: 'static, ET: 'static>
    Model<RoleConfig<MC, DC, EC>, RoleComponent<MT, DT, ET>> for PanelsModel<MC, DC, EC>
{
    type State = Panels<MC, DC, EC>;
    type Snapshot = ChildPanels<MC, MT, DC, DT, EC, ET>;
    fn persistent(&self) -> bool {
        self.0.codec.is_some()
    }
    fn configurations(&self, p: &Self::State) -> Vec<Rc<RoleConfig<MC, DC, EC>>> {
        let mut out = vec![Rc::new(RoleConfig::Main(p.main.clone()))];
        if let Some(d) = &p.details {
            out.push(Rc::new(RoleConfig::Details(d.clone())));
        }
        if let Some(e) = &p.extra {
            out.push(Rc::new(RoleConfig::Extra(e.clone())));
        }
        out
    }
    fn validate(&self, _: &Self::State) -> Result<(), NavigationError> {
        Ok(())
    }
    fn statuses(&self, p: &Self::State) -> Vec<PageStatus> {
        let mut out = vec![if p.mode != ChildPanelsMode::Single
            || p.details.is_none() && p.extra.is_none()
        {
            PageStatus::Resumed
        } else {
            PageStatus::Created
        }];
        if p.details.is_some() {
            out.push(if p.mode == ChildPanelsMode::Triple || p.extra.is_none() {
                PageStatus::Resumed
            } else {
                PageStatus::Created
            });
        }
        if p.extra.is_some() {
            out.push(PageStatus::Resumed);
        }
        out
    }
    fn snapshot(
        &self,
        children: Vec<PageChild<RoleConfig<MC, DC, EC>, RoleComponent<MT, DT, ET>>>,
        p: &Self::State,
    ) -> Self::Snapshot {
        let (mut main, mut details, mut extra) = (None, None, None);
        for child in children {
            let PageChild::Created(child) = child else {
                unreachable!("panels are always live")
            };
            match (child.configuration.as_ref(), child.instance.as_ref()) {
                (RoleConfig::Main(c), RoleComponent::Main(i)) => {
                    main = Some(Child {
                        configuration: c.clone(),
                        instance: i.clone(),
                        key: child.key,
                    })
                }
                (RoleConfig::Details(c), RoleComponent::Details(i)) => {
                    details = Some(Child {
                        configuration: c.clone(),
                        instance: i.clone(),
                        key: child.key,
                    })
                }
                (RoleConfig::Extra(c), RoleComponent::Extra(i)) => {
                    extra = Some(Child {
                        configuration: c.clone(),
                        instance: i.clone(),
                        key: child.key,
                    })
                }
                _ => unreachable!("typed panel factory role mismatch"),
            }
        }
        ChildPanels {
            main: main.expect("required main"),
            details,
            extra,
            mode: p.mode,
        }
    }
    fn on_state_changed(&self, p: &Self::State, old: Option<&Self::State>) {
        if let Some(f) = &self.0.on_state_changed {
            f(p, old);
        }
    }
    fn back_enabled(&self, p: &Self::State) -> bool {
        self.0.handle_back_button
            && p.mode == ChildPanelsMode::Single
            && (p.extra.is_some() || p.details.is_some())
    }
    fn back_transform(&self, mut p: Self::State) -> Self::State {
        if p.extra.take().is_none() {
            p.details = None;
        }
        p
    }
    fn decode(&self, bytes: &[u8]) -> Result<Option<SavedState<Self::State>>, NavigationError> {
        self.0
            .codec
            .as_ref()
            .map(|c| state::decode(c, bytes))
            .transpose()
    }
    fn encode(
        &self,
        p: &Self::State,
        children: &Vec<(String, BTreeMap<String, Vec<u8>>)>,
        next_key: u64,
    ) -> Result<Option<Vec<u8>>, NavigationError> {
        self.0
            .codec
            .as_ref()
            .map(|c| state::encode(c, p, children, next_key))
            .transpose()
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn build<Ctx, MC, MT, DC, DT, EC, ET>(
    context: &Ctx,
    navigation: &(impl crate::router::children::NavigationSource<PanelsEvent<MC, DC, EC>> + ?Sized),
    initial: impl FnOnce() -> Panels<MC, DC, EC>,
    options: PanelsOptions<MC, DC, EC>,
    main: impl Fn(&MC, Ctx) -> MT + 'static,
    details: impl Fn(&DC, Ctx) -> DT + 'static,
    extra: impl Fn(&EC, Ctx) -> ET + 'static,
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
    crate::router::engine::build_with_subscriber(
        context,
        |receiver| {
            crate::router::navigation::subscribe(
                navigation,
                receiver,
                std::convert::identity,
                std::convert::identity,
                |result| super::NavigationResult {
                    old_panels: result.old_stack,
                    new_panels: result.new_stack,
                },
            )
        },
        initial,
        options.key.clone(),
        PanelsModel(options),
        move |c, ctx| match c {
            RoleConfig::Main(c) => RoleComponent::Main(Rc::new(main(c, ctx))),
            RoleConfig::Details(c) => RoleComponent::Details(Rc::new(details(c, ctx))),
            RoleConfig::Extra(c) => RoleComponent::Extra(Rc::new(extra(c, ctx))),
        },
    )
    .map(|(value, _)| value)
}
