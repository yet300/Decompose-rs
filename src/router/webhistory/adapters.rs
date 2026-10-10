//! Bridges for the standard router models.
use super::*;
use crate::router::{
    Child,
    pages::{ChildPages, Pages, PagesNavigator},
    panels::{ChildPanels, PanelsNavigator},
    stack::{ChildStack, StackNavigator},
};
use std::cell::RefCell;

pub(super) fn completion_bridge(
    completion: HistoryCompletion,
) -> impl Fn(Result<(), super::super::NavigationError>) {
    let completion = RefCell::new(Some(completion));
    move |result| {
        let done = completion.borrow_mut().take();
        if let Some(done) = done {
            done(result.map_err(HistoryError::from));
        }
    }
}

/// Binds a stack. With history disabled only the active child contributes a key,
/// so browser restoration replaces the stack with that single configuration.
/// The navigator must have exactly one connected router for this binding.
pub fn stack_web_navigation<C: 'static, T: 'static>(
    navigator: impl StackNavigator<C> + 'static,
    stack: Value<ChildStack<C, T>>,
    codec: RouteCodec<C>,
    enable_history: bool,
    mapper: impl Fn(&Child<C, T>) -> RouteInfo + 'static,
) -> TypedWebNavigation<C> {
    let routes = stack.map(move |stack| {
        if enable_history {
            stack
                .items()
                .map(|child| mapper(child).route(child.configuration.clone()))
                .collect()
        } else {
            vec![mapper(&stack.active).route(stack.active.configuration.clone())]
        }
    });
    TypedWebNavigation::new(routes, codec, move |keys, done| {
        let done = completion_bridge(done);
        navigator.navigate_with(
            move |_| keys.clone(),
            move |result| done(result.map(|_| ())),
        );
    })
}

/// Binds the whole pages state as one history key. Selection or membership
/// changes rewrite the current entry; nested histories may still push and pop.
/// Empty pages remain representable by the single whole-state key.
pub fn pages_web_navigation<C: 'static, T: 'static>(
    navigator: impl PagesNavigator<C> + 'static,
    pages: Value<ChildPages<C, T>>,
    codec: RouteCodec<Pages<C>>,
    mapper: impl Fn(&ChildPages<C, T>) -> RouteInfo + 'static,
) -> TypedWebNavigation<Pages<C>> {
    let routes = pages.map(move |pages| {
        vec![
            mapper(pages).route(Rc::new(Pages {
                items: pages
                    .items
                    .iter()
                    .map(|child| child.configuration().clone())
                    .collect(),
                selected_index: pages.selected_index,
            })),
        ]
    });
    TypedWebNavigation::new(routes, codec, move |keys, done| {
        let [key] = keys.as_slice() else {
            done(Err(HistoryError::Codec(
                "pages history must contain one state".into(),
            )));
            return;
        };
        let pages = key.as_ref().clone();
        let done = completion_bridge(done);
        navigator.navigate_with(
            move |_| pages.clone(),
            move |result| done(result.map(|_| ())),
        );
    })
}

/// Persistable panel membership. Lifecycle mode stays local to the router,
/// matching upstream history keys, which contain configurations only.
pub struct PanelRouteKey<MC, DC, EC = std::convert::Infallible> {
    /// Main configuration.
    pub main: Rc<MC>,
    /// Optional Details configuration.
    pub details: Option<Rc<DC>>,
    /// Optional Extra configuration.
    pub extra: Option<Rc<EC>>,
}
impl<MC, DC, EC> Clone for PanelRouteKey<MC, DC, EC> {
    fn clone(&self) -> Self {
        Self {
            main: self.main.clone(),
            details: self.details.clone(),
            extra: self.extra.clone(),
        }
    }
}

/// Binds typed panel membership as one history key, preserving the router's
/// current lifecycle mode when browser navigation restores configurations.
#[allow(clippy::type_complexity)]
pub fn panels_web_navigation<
    MC: 'static,
    MT: 'static,
    DC: 'static,
    DT: 'static,
    EC: 'static,
    ET: 'static,
>(
    navigator: impl PanelsNavigator<MC, DC, EC> + 'static,
    panels: Value<ChildPanels<MC, MT, DC, DT, EC, ET>>,
    codec: RouteCodec<PanelRouteKey<MC, DC, EC>>,
    mapper: impl Fn(&ChildPanels<MC, MT, DC, DT, EC, ET>) -> RouteInfo + 'static,
) -> TypedWebNavigation<PanelRouteKey<MC, DC, EC>> {
    let routes = panels.map(move |panels| {
        vec![
            mapper(panels).route(Rc::new(PanelRouteKey {
                main: panels.main.configuration.clone(),
                details: panels
                    .details
                    .as_ref()
                    .map(|child| child.configuration.clone()),
                extra: panels
                    .extra
                    .as_ref()
                    .map(|child| child.configuration.clone()),
            })),
        ]
    });
    TypedWebNavigation::new(routes, codec, move |keys, done| {
        let [key] = keys.as_slice() else {
            done(Err(HistoryError::Codec(
                "panels history must contain one state".into(),
            )));
            return;
        };
        let key = key.as_ref().clone();
        let done = completion_bridge(done);
        navigator.navigate_with(
            move |mut panels| {
                panels.main = key.main.clone();
                panels.details = key.details.clone();
                panels.extra = key.extra.clone();
                panels
            },
            move |result| done(result.map(|_| ())),
        );
    })
}
