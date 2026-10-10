use super::Child;
use std::rc::Rc;
/// Requested child lifecycle, bounded by the hosting lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildStatus {
    Created,
    Started,
    Resumed,
    Destroyed,
}
/// A created component or an intentional destroyed entry retaining configuration and identity.
pub enum ChildItem<C, T> {
    /// Component with a live managed scope.
    Created(Child<C, T>),
    /// Lazy entry with no component or instance keeper.
    Destroyed { configuration: Rc<C>, key: String },
}
impl<C, T> Clone for ChildItem<C, T> {
    fn clone(&self) -> Self {
        match self {
            Self::Created(c) => Self::Created(c.clone()),
            Self::Destroyed { configuration, key } => Self::Destroyed {
                configuration: configuration.clone(),
                key: key.clone(),
            },
        }
    }
}
impl<C, T> ChildItem<C, T> {
    /// Configuration shared by either variant.
    pub fn configuration(&self) -> &Rc<C> {
        match self {
            Self::Created(c) => &c.configuration,
            Self::Destroyed { configuration, .. } => configuration,
        }
    }
    /// Identity remains stable across intentional destruction and resurrection.
    pub fn key(&self) -> &str {
        match self {
            Self::Created(c) => &c.key,
            Self::Destroyed { key, .. } => key,
        }
    }
    /// Component, when created. Older snapshots can retain ended components.
    pub fn instance(&self) -> Option<&Rc<T>> {
        match self {
            Self::Created(c) => Some(&c.instance),
            Self::Destroyed { .. } => None,
        }
    }
}

/// One immutable configuration and its requested lifecycle status.
pub struct ChildNavState<C> {
    /// Configuration identity is matched by equality across transitions.
    pub configuration: Rc<C>,
    /// Requested lifecycle, bounded by the parent.
    pub status: ChildStatus,
}
impl<C> ChildNavState<C> {
    /// Creates a child record from an owned configuration.
    pub fn new(configuration: C, status: ChildStatus) -> Self {
        Self {
            configuration: Rc::new(configuration),
            status,
        }
    }
}
impl<C> Clone for ChildNavState<C> {
    fn clone(&self) -> Self {
        Self {
            configuration: self.configuration.clone(),
            status: self.status,
        }
    }
}
/// Immutable, deterministic projection of navigation state into ordered children.
///
/// Repeated projections of one state must preserve configuration identity, order
/// and status. A codec must also preserve configuration identity and order.
pub trait NavState<C>: Clone + 'static {
    /// Projects all children, including intentional destroyed entries.
    fn children(&self) -> Vec<ChildNavState<C>>;
}
impl<C: 'static> NavState<C> for Vec<ChildNavState<C>> {
    fn children(&self) -> Vec<ChildNavState<C>> {
        self.clone()
    }
}
