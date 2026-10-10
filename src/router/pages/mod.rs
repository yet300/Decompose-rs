//! A selectable list of children with lazy component creation and saved tombstones.
mod navigation;
mod state;
pub use super::{Child, NavigationError};
use crate::{ComponentContext, value::Value};
pub use navigation::{NavigationResult, PagesEvent, PagesNavigation, PagesNavigator};
pub use state::PagesCodec;
use std::rc::Rc;
/// Configuration membership and its numeric selection.
pub struct Pages<C> {
    /// Ordered configurations; may be empty.
    pub items: Vec<Rc<C>>,
    /// None exactly when membership is empty.
    pub selected_index: Option<usize>,
}
impl<C> Clone for Pages<C> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            selected_index: self.selected_index,
        }
    }
}
impl<C> Default for Pages<C> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            selected_index: None,
        }
    }
}
impl<C> Pages<C> {
    /// Constructs owned configurations, validating the selection and normalizing empty pages.
    pub fn new(items: Vec<C>, selected_index: usize) -> Result<Self, NavigationError> {
        let pages = Self {
            selected_index: (!items.is_empty()).then_some(selected_index),
            items: items.into_iter().map(Rc::new).collect(),
        };
        pages.validate()?;
        Ok(pages)
    }
    pub(crate) fn validate(&self) -> Result<(), NavigationError> {
        if (self.items.is_empty() && self.selected_index.is_some())
            || (!self.items.is_empty() && self.selected_index.is_none_or(|i| i >= self.items.len()))
        {
            return Err(NavigationError::InvalidSelection {
                selected_index: self.selected_index,
                item_count: self.items.len(),
            });
        }
        Ok(())
    }
}
pub use super::children::{ChildItem as PageChild, ChildStatus as PageStatus};
/// Selected page resumes, direct neighbours are created, and other pages are destroyed.
pub fn default_page_status<C>(index: usize, pages: &Pages<C>) -> PageStatus {
    match pages.selected_index {
        Some(i) if i == index => PageStatus::Resumed,
        Some(i) if i.abs_diff(index) == 1 => PageStatus::Created,
        _ => PageStatus::Destroyed,
    }
}
/// Immutable child membership and numeric selection.
pub struct ChildPages<C, T> {
    /// Ordered live and destroyed children.
    pub items: Vec<PageChild<C, T>>,
    /// None for empty pages, otherwise an index into items.
    pub selected_index: Option<usize>,
}
impl<C, T> Clone for ChildPages<C, T> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            selected_index: self.selected_index,
        }
    }
}
impl<C, T> Default for ChildPages<C, T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            selected_index: None,
        }
    }
}
impl<C, T> ChildPages<C, T> {
    /// The selected entry, including an intentionally destroyed selected page.
    pub fn selected(&self) -> Option<&PageChild<C, T>> {
        self.selected_index.and_then(|i| self.items.get(i))
    }
}
type PageStatusPolicy<C> = dyn Fn(usize, &Pages<C>) -> PageStatus;
/// Pages construction options. Membership is transient unless a codec is supplied.
pub struct PagesOptions<C> {
    /// Unique state-provider and retained-instance key.
    pub key: String,
    /// Select the previous page on unhandled Back when selection is above zero.
    pub handle_back_button: bool,
    /// Match equal configurations by occurrence when enabled.
    pub allow_duplicates: bool,
    /// Explicit whole-list configuration serialization.
    pub codec: Option<PagesCodec<C>>,
    /// Optional lifecycle policy; defaults to selected and direct neighbours.
    pub page_status: Option<Rc<PageStatusPolicy<C>>>,
}
impl<C> Default for PagesOptions<C> {
    fn default() -> Self {
        Self {
            key: "DefaultChildPages".into(),
            handle_back_button: false,
            allow_duplicates: false,
            codec: None,
            page_status: None,
        }
    }
}
struct PagesModel<C>(PagesOptions<C>);
impl<C: Eq + 'static, T: 'static> super::engine::Model<C, T> for PagesModel<C> {
    type State = Pages<C>;
    type Snapshot = ChildPages<C, T>;
    fn persistent(&self) -> bool {
        self.0.codec.is_some()
    }
    fn configurations(&self, state: &Pages<C>) -> Vec<Rc<C>> {
        state.items.clone()
    }
    fn validate(&self, state: &Pages<C>) -> Result<(), NavigationError> {
        state.validate()?;
        if !self.0.allow_duplicates
            && state
                .items
                .iter()
                .enumerate()
                .any(|(i, c)| state.items[..i].contains(c))
        {
            return Err(NavigationError::DuplicateConfiguration);
        }
        Ok(())
    }
    fn statuses(&self, state: &Pages<C>) -> Vec<PageStatus> {
        (0..state.items.len())
            .map(|i| {
                self.0
                    .page_status
                    .as_ref()
                    .map_or_else(|| default_page_status(i, state), |f| f(i, state))
            })
            .collect()
    }
    fn snapshot(&self, children: Vec<PageChild<C, T>>, state: &Pages<C>) -> Self::Snapshot {
        ChildPages {
            items: children,
            selected_index: state.selected_index,
        }
    }
    fn back_enabled(&self, state: &Pages<C>) -> bool {
        self.0.handle_back_button && state.selected_index.is_some_and(|i| i > 0)
    }
    fn back_transform(&self, mut state: Pages<C>) -> Pages<C> {
        if let Some(i) = state.selected_index {
            state.selected_index = Some(i.saturating_sub(1));
        }
        state
    }
    fn decode(
        &self,
        bytes: &[u8],
    ) -> Result<Option<super::engine::SavedState<Pages<C>>>, NavigationError> {
        self.0
            .codec
            .as_ref()
            .map(|codec| state::decode(codec, bytes))
            .transpose()
    }
    fn encode(
        &self,
        state: &Pages<C>,
        children: &Vec<(String, std::collections::BTreeMap<String, Vec<u8>>)>,
        next_key: u64,
    ) -> Result<Option<Vec<u8>>, NavigationError> {
        self.0
            .codec
            .as_ref()
            .map(|codec| state::encode(codec, state, children, next_key))
            .transpose()
    }
}
/// Builds children with the hosting context's factory, services and lifecycle.
/// Reentrant events are FIFO; the parent owns navigation even after views are dropped.
pub fn child_pages<Ctx: ComponentContext + 'static, C: Eq + 'static, T: 'static>(
    context: &Ctx,
    navigation: &(impl super::children::NavigationSource<PagesEvent<C>> + ?Sized),
    initial: Pages<C>,
    options: PagesOptions<C>,
    factory: impl Fn(&C, Ctx) -> T + 'static,
) -> Result<Value<ChildPages<C, T>>, NavigationError> {
    super::engine::build_with_subscriber(
        context,
        |receiver| {
            super::navigation::subscribe(
                navigation,
                receiver,
                std::convert::identity,
                std::convert::identity,
                |result| NavigationResult {
                    old_pages: result.old_stack,
                    new_pages: result.new_stack,
                },
            )
        },
        || initial,
        options.key.clone(),
        PagesModel(options),
        factory,
    )
    .map(|(value, _)| value)
}
