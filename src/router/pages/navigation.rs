use super::{NavigationError, Pages};
use crate::router::{
    NavigationEvent,
    children::{NavigationSource, SimpleNavigation},
};
use std::rc::Rc;
/// Actual receiver membership before and after a committed event.
pub struct NavigationResult<C> {
    /// State before this receiver applies the event.
    pub old_pages: Pages<C>,
    /// State after publication.
    pub new_pages: Pages<C>,
}
impl<C> Clone for NavigationResult<C> {
    fn clone(&self) -> Self {
        Self {
            old_pages: self.old_pages.clone(),
            new_pages: self.new_pages.clone(),
        }
    }
}
/// Public pages state transformation and per-receiver completion.
pub type PagesEvent<C> = NavigationEvent<Pages<C>, NavigationResult<C>>;

/// Standard pages operations shared by sources and custom navigators.
///
/// Implement `navigate_with` to inherit the standard helper policies.
/// Generic callbacks use static dispatch; this trait is not object-safe.
pub trait PagesNavigator<C: 'static> {
    /// Applies a transformer, panicking on transition errors.
    fn navigate(&self, transformer: impl Fn(Pages<C>) -> Pages<C> + 'static) {
        self.navigate_with(transformer, |r| {
            if let Err(e) = r {
                panic!("pages navigation failed: {e:?}")
            }
        })
    }
    /// Applies a transformer to each receiver, completing after publication.
    fn navigate_with(
        &self,
        transformer: impl Fn(Pages<C>) -> Pages<C> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    );
    /// Selects an index. Empty pages remain empty; nonempty invalid indices panic.
    fn select(&self, index: usize) {
        self.navigate(move |mut p| {
            p.selected_index = (!p.items.is_empty()).then_some(index);
            p
        })
    }
    /// Selects an index, reporting invalid nonempty selection as a typed error.
    fn select_with(
        &self,
        index: usize,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.navigate_with(
            move |mut p| {
                p.selected_index = (!p.items.is_empty()).then_some(index);
                p
            },
            completion,
        )
    }
    /// Selects the next page, optionally wrapping at the boundary.
    fn select_next(&self, circular: bool) {
        self.select_next_with(circular, panic_completion)
    }
    /// Selects the next page and receives each receiver's result.
    fn select_next_with(
        &self,
        circular: bool,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.navigate_with(
            move |mut p| {
                if let Some(i) = p.selected_index {
                    p.selected_index = Some(if i + 1 < p.items.len() {
                        i + 1
                    } else if circular {
                        0
                    } else {
                        i
                    });
                }
                p
            },
            completion,
        )
    }
    /// Selects the previous page, optionally wrapping at the boundary.
    fn select_prev(&self, circular: bool) {
        self.select_prev_with(circular, panic_completion)
    }
    /// Selects the previous page and receives each receiver's result.
    fn select_prev_with(
        &self,
        circular: bool,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.navigate_with(
            move |mut p| {
                if let Some(i) = p.selected_index {
                    p.selected_index = Some(if i > 0 {
                        i - 1
                    } else if circular {
                        p.items.len() - 1
                    } else {
                        0
                    });
                }
                p
            },
            completion,
        )
    }
    /// Selects the first page.
    fn select_first(&self) {
        self.select(0)
    }
    /// Selects the first page and completes after publication.
    fn select_first_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.select_with(0, completion)
    }
    /// Selects the last page.
    fn select_last(&self) {
        self.select_last_with(panic_completion)
    }
    /// Selects the last page and completes after publication.
    fn select_last_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.navigate_with(
            |mut p| {
                p.selected_index = p.items.len().checked_sub(1);
                p
            },
            completion,
        )
    }
    /// Removes every page.
    fn clear(&self) {
        self.navigate(|_| Pages::default())
    }
    /// Removes every page and completes after publication.
    fn clear_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.navigate_with(|_| Pages::default(), completion)
    }
    /// Replaces membership, clamping the old numeric index to the new range.
    fn set_items(&self, items: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        self.set_items_with(items, panic_completion)
    }
    /// Replaces membership, clamping selection and reporting each receiver's result.
    fn set_items_with(
        &self,
        items: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.navigate_with(
            move |p| {
                let items = items(p.items);
                let selected_index = items
                    .len()
                    .checked_sub(1)
                    .map(|last| p.selected_index.unwrap_or(0).min(last));
                Pages {
                    items,
                    selected_index,
                }
            },
            completion,
        )
    }
}

/// Shared synchronous navigation; each receiver applies events to its own state.
///
/// Events are shared by `Rc`, never replayed, and discarded without receivers.
/// Nested broadcasts are FIFO. Panics abandon queued events and propagate;
/// the source remains usable afterward.
pub struct PagesNavigation<C> {
    source: SimpleNavigation<PagesEvent<C>>,
}
impl<C> Clone for PagesNavigation<C> {
    fn clone(&self) -> Self {
        Self {
            source: self.source.clone(),
        }
    }
}
impl<C: 'static> Default for PagesNavigation<C> {
    fn default() -> Self {
        Self::new()
    }
}
impl<C: 'static> PagesNavigation<C> {
    /// Creates an empty source. Events without receivers are discarded.
    pub fn new() -> Self {
        Self {
            source: SimpleNavigation::new(),
        }
    }
    /// Applies a transformer, panicking on transition errors.
    pub fn navigate(&self, transformer: impl Fn(Pages<C>) -> Pages<C> + 'static) {
        PagesNavigator::navigate(self, transformer)
    }
    /// Applies a transformer to each receiver, completing after publication.
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Pages<C>) -> Pages<C> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::navigate_with(self, transformer, completion)
    }
    /// Selects an index. Empty pages remain empty; nonempty invalid indices panic.
    pub fn select(&self, index: usize) {
        PagesNavigator::select(self, index)
    }
    /// Selects an index, reporting invalid nonempty selection as a typed error.
    pub fn select_with(
        &self,
        index: usize,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::select_with(self, index, completion)
    }
    /// Selects the next page, optionally wrapping at the boundary.
    pub fn select_next(&self, circular: bool) {
        PagesNavigator::select_next(self, circular)
    }
    /// Selects the next page and receives each receiver's result.
    pub fn select_next_with(
        &self,
        circular: bool,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::select_next_with(self, circular, completion)
    }
    /// Selects the previous page, optionally wrapping at the boundary.
    pub fn select_prev(&self, circular: bool) {
        PagesNavigator::select_prev(self, circular)
    }
    /// Selects the previous page and receives each receiver's result.
    pub fn select_prev_with(
        &self,
        circular: bool,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::select_prev_with(self, circular, completion)
    }
    /// Selects the first page.
    pub fn select_first(&self) {
        PagesNavigator::select_first(self)
    }
    /// Selects the first page and completes after publication.
    pub fn select_first_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::select_first_with(self, completion)
    }
    /// Selects the last page.
    pub fn select_last(&self) {
        PagesNavigator::select_last(self)
    }
    /// Selects the last page and completes after publication.
    pub fn select_last_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::select_last_with(self, completion)
    }
    /// Removes every page.
    pub fn clear(&self) {
        PagesNavigator::clear(self)
    }
    /// Removes every page and completes after publication.
    pub fn clear_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::clear_with(self, completion)
    }
    /// Replaces membership, clamping the old numeric index to the new range.
    pub fn set_items(&self, items: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        PagesNavigator::set_items(self, items)
    }
    /// Replaces membership, clamping selection and reporting each receiver's result.
    pub fn set_items_with(
        &self,
        items: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::set_items_with(self, items, completion)
    }
}
impl<C: 'static> PagesNavigator<C> for PagesNavigation<C> {
    fn navigate_with(
        &self,
        transformer: impl Fn(Pages<C>) -> Pages<C> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.source
            .navigate(PagesEvent::with_completion(transformer, completion));
    }
}
impl<C: 'static> NavigationSource<PagesEvent<C>> for PagesNavigation<C> {
    fn subscribe(&self, observer: Rc<dyn Fn(Rc<PagesEvent<C>>)>) -> crate::value::Subscription {
        self.source.subscribe(observer)
    }
}
fn panic_completion<C>(r: Result<NavigationResult<C>, NavigationError>) {
    if let Err(e) = r {
        panic!("pages navigation failed: {e:?}")
    }
}
