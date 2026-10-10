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
    /// Applies a transformer to each receiver, completing after publication.
    fn navigate_with(
        &self,
        transformer: impl Fn(Pages<C>) -> Pages<C> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    );
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
    /// Selects the first page and completes after publication.
    fn select_first_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.select_with(0, completion)
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
    /// Removes every page and completes after publication.
    fn clear_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.navigate_with(|_| Pages::default(), completion)
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
    /// Fallible counterpart of `navigate`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_navigate(
        &self,
        transformer: impl Fn(Pages<C>) -> Pages<C> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| {
            self.navigate_with(transformer, completion)
        })
    }
    /// Fallible counterpart of `select`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_select(
        &self,
        index: usize,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| self.select_with(index, completion))
    }
    /// Fallible counterpart of `select_next`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_select_next(
        &self,
        circular: bool,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| {
            self.select_next_with(circular, completion)
        })
    }
    /// Fallible counterpart of `select_prev`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_select_prev(
        &self,
        circular: bool,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| {
            self.select_prev_with(circular, completion)
        })
    }
    /// Fallible counterpart of `select_first`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_select_first(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| self.select_first_with(completion))
    }
    /// Fallible counterpart of `select_last`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_select_last(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| self.select_last_with(completion))
    }
    /// Fallible counterpart of `clear`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_clear(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| self.clear_with(completion))
    }
    /// Fallible counterpart of `set_items`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    fn try_set_items(
        &self,
        items: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        crate::router::navigation::collect_results(|completion| {
            self.set_items_with(items, completion)
        })
    }
    /// Explicitly panicking compatibility operation. See `try_navigate` for errors.
    #[track_caller]
    fn navigate_or_panic(&self, transformer: impl Fn(Pages<C>) -> Pages<C> + 'static) {
        self.navigate_with(transformer, |r| {
            if let Err(e) = r {
                panic!("pages navigation failed: {e:?}")
            }
        })
    }
    /// Explicitly panicking compatibility operation. See `try_select` for errors.
    #[track_caller]
    fn select_or_panic(&self, index: usize) {
        self.navigate_or_panic(move |mut p| {
            p.selected_index = (!p.items.is_empty()).then_some(index);
            p
        })
    }
    /// Explicitly panicking compatibility operation. See `try_select_next` for errors.
    #[track_caller]
    fn select_next_or_panic(&self, circular: bool) {
        self.select_next_with(circular, panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_select_prev` for errors.
    #[track_caller]
    fn select_prev_or_panic(&self, circular: bool) {
        self.select_prev_with(circular, panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_select_first` for errors.
    #[track_caller]
    fn select_first_or_panic(&self) {
        self.select_or_panic(0)
    }
    /// Explicitly panicking compatibility operation. See `try_select_last` for errors.
    #[track_caller]
    fn select_last_or_panic(&self) {
        self.select_last_with(panic_completion)
    }
    /// Explicitly panicking compatibility operation. See `try_clear` for errors.
    #[track_caller]
    fn clear_or_panic(&self) {
        self.navigate_or_panic(|_| Pages::default())
    }
    /// Explicitly panicking compatibility operation. See `try_set_items` for errors.
    #[track_caller]
    fn set_items_or_panic(&self, items: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        self.set_items_with(items, panic_completion)
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
    /// Applies a transformer to each receiver, completing after publication.
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Pages<C>) -> Pages<C> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::navigate_with(self, transformer, completion)
    }
    /// Selects an index, reporting invalid nonempty selection as a typed error.
    pub fn select_with(
        &self,
        index: usize,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::select_with(self, index, completion)
    }
    /// Selects the next page and receives each receiver's result.
    pub fn select_next_with(
        &self,
        circular: bool,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::select_next_with(self, circular, completion)
    }
    /// Selects the previous page and receives each receiver's result.
    pub fn select_prev_with(
        &self,
        circular: bool,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::select_prev_with(self, circular, completion)
    }
    /// Selects the first page and completes after publication.
    pub fn select_first_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::select_first_with(self, completion)
    }
    /// Selects the last page and completes after publication.
    pub fn select_last_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::select_last_with(self, completion)
    }
    /// Removes every page and completes after publication.
    pub fn clear_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        PagesNavigator::clear_with(self, completion)
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
        let event = PagesEvent::with_completion(transformer, completion);
        let abandon = event.clone();
        self.source
            .navigate_rc_abandonable(Rc::new(event), move || {
                abandon.complete(Err(NavigationError::ChildUnavailable))
            });
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

impl<C: 'static> PagesNavigation<C> {
    /// Fallible counterpart of `navigate`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_navigate(
        &self,
        transformer: impl Fn(Pages<C>) -> Pages<C> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        PagesNavigator::try_navigate(self, transformer)
    }
    /// Fallible counterpart of `select`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_select(
        &self,
        index: usize,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        PagesNavigator::try_select(self, index)
    }
    /// Fallible counterpart of `select_next`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_select_next(
        &self,
        circular: bool,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        PagesNavigator::try_select_next(self, circular)
    }
    /// Fallible counterpart of `select_prev`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_select_prev(
        &self,
        circular: bool,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        PagesNavigator::try_select_prev(self, circular)
    }
    /// Fallible counterpart of `select_first`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_select_first(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        PagesNavigator::try_select_first(self)
    }
    /// Fallible counterpart of `select_last`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_select_last(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        PagesNavigator::try_select_last(self)
    }
    /// Fallible counterpart of `clear`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_clear(
        &self,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        PagesNavigator::try_clear(self)
    }
    /// Fallible counterpart of `set_items`; accepted nested work returns a pending receipt.
    /// Stored transformations own their captures because FIFO delivery may outlive this call.
    pub fn try_set_items(
        &self,
        items: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static,
    ) -> Result<crate::router::NavigationOutcome<NavigationResult<C>>, NavigationError> {
        PagesNavigator::try_set_items(self, items)
    }
    /// Explicitly panicking compatibility operation. See `try_navigate` for errors.
    #[track_caller]
    pub fn navigate_or_panic(&self, transformer: impl Fn(Pages<C>) -> Pages<C> + 'static) {
        PagesNavigator::navigate_or_panic(self, transformer)
    }
    /// Explicitly panicking compatibility operation. See `try_select` for errors.
    #[track_caller]
    pub fn select_or_panic(&self, index: usize) {
        PagesNavigator::select_or_panic(self, index)
    }
    /// Explicitly panicking compatibility operation. See `try_select_next` for errors.
    #[track_caller]
    pub fn select_next_or_panic(&self, circular: bool) {
        PagesNavigator::select_next_or_panic(self, circular)
    }
    /// Explicitly panicking compatibility operation. See `try_select_prev` for errors.
    #[track_caller]
    pub fn select_prev_or_panic(&self, circular: bool) {
        PagesNavigator::select_prev_or_panic(self, circular)
    }
    /// Explicitly panicking compatibility operation. See `try_select_first` for errors.
    #[track_caller]
    pub fn select_first_or_panic(&self) {
        PagesNavigator::select_first_or_panic(self)
    }
    /// Explicitly panicking compatibility operation. See `try_select_last` for errors.
    #[track_caller]
    pub fn select_last_or_panic(&self) {
        PagesNavigator::select_last_or_panic(self)
    }
    /// Explicitly panicking compatibility operation. See `try_clear` for errors.
    #[track_caller]
    pub fn clear_or_panic(&self) {
        PagesNavigator::clear_or_panic(self)
    }
    /// Explicitly panicking compatibility operation. See `try_set_items` for errors.
    #[track_caller]
    pub fn set_items_or_panic(&self, items: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        PagesNavigator::set_items_or_panic(self, items)
    }
}
