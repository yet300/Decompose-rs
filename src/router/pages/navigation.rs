use super::{NavigationError, Pages};
use crate::router::navigation::NavigationSource;
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
/// Shared synchronous navigation; each receiver applies the event to its own state.
pub struct PagesNavigation<C> {
    pub(crate) source: NavigationSource<Pages<C>>,
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
    /// Creates a source. Events without receivers are discarded.
    pub fn new() -> Self {
        Self {
            source: NavigationSource::new(),
        }
    }
    /// Applies a transformer, panicking on transition errors.
    pub fn navigate(&self, transformer: impl Fn(Pages<C>) -> Pages<C> + 'static) {
        self.navigate_with(transformer, |r| {
            if let Err(e) = r {
                panic!("pages navigation failed: {e:?}")
            }
        })
    }
    /// Applies a transformer to each receiver, completing after publication.
    pub fn navigate_with(
        &self,
        transformer: impl Fn(Pages<C>) -> Pages<C> + 'static,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.source.navigate_with(transformer, move |r| {
            completion(r.map(|r| NavigationResult {
                old_pages: r.old_stack,
                new_pages: r.new_stack,
            }))
        });
    }
    /// Selects an index. Empty pages remain empty; nonempty invalid indices panic.
    pub fn select(&self, index: usize) {
        self.navigate(move |mut p| {
            p.selected_index = (!p.items.is_empty()).then_some(index);
            p
        })
    }
    /// Selects an index, reporting invalid nonempty selection as a typed error.
    pub fn select_with(
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
    pub fn select_next(&self, circular: bool) {
        self.select_next_with(circular, panic_completion)
    }
    /// Selects the next page and receives each receiver's result.
    pub fn select_next_with(
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
    pub fn select_prev(&self, circular: bool) {
        self.select_prev_with(circular, panic_completion)
    }
    /// Selects the previous page and receives each receiver's result.
    pub fn select_prev_with(
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
    pub fn select_first(&self) {
        self.select(0)
    }
    /// Selects the first page and completes after publication.
    pub fn select_first_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.select_with(0, completion)
    }
    /// Selects the last page.
    pub fn select_last(&self) {
        self.select_last_with(panic_completion)
    }
    /// Selects the last page and completes after publication.
    pub fn select_last_with(
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
    pub fn clear(&self) {
        self.navigate(|_| Pages::default())
    }
    /// Removes every page and completes after publication.
    pub fn clear_with(
        &self,
        completion: impl Fn(Result<NavigationResult<C>, NavigationError>) + 'static,
    ) {
        self.navigate_with(|_| Pages::default(), completion)
    }
    /// Replaces membership, clamping the old numeric index to the new range.
    pub fn set_items(&self, items: impl Fn(Vec<Rc<C>>) -> Vec<Rc<C>> + 'static) {
        self.set_items_with(items, panic_completion)
    }
    /// Replaces membership, clamping selection and reporting each receiver's result.
    pub fn set_items_with(
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
fn panic_completion<C>(r: Result<NavigationResult<C>, NavigationError>) {
    if let Err(e) = r {
        panic!("pages navigation failed: {e:?}")
    }
}
