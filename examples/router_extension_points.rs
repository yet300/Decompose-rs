use decompose_rs::router::NavigationError;
use decompose_rs::router::children::{NavigationSource, SimpleNavigation};
use decompose_rs::router::stack::{
    NavigationResult, StackEvent, StackNavigator, StackOptions, child_stack,
};
use decompose_rs::value::Subscription;
use decompose_rs::{ComponentContext, DefaultComponentContext, LifecycleRegistry, LifecycleState};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Debug, PartialEq, Eq)]
enum Route {
    Home,
    Details(u32),
    Settings,
}

// A custom source can intercept operations and reuse every standard helper.
struct AuditedNavigation {
    events: SimpleNavigation<StackEvent<Route>>,
    depths: Rc<RefCell<Vec<usize>>>,
    connections: Rc<Cell<usize>>,
}

impl NavigationSource<StackEvent<Route>> for AuditedNavigation {
    fn subscribe(&self, observer: Rc<dyn Fn(Rc<StackEvent<Route>>)>) -> Subscription {
        let token = self.events.subscribe(observer);
        self.connections.set(self.connections.get() + 1);
        let connections = self.connections.clone();
        Subscription::from_cancel(move || {
            connections.set(connections.get() - 1);
            drop(token);
        })
    }
}

impl StackNavigator<Route> for AuditedNavigation {
    fn navigate_with(
        &self,
        transformer: impl Fn(Vec<Rc<Route>>) -> Vec<Rc<Route>> + 'static,
        completion: impl Fn(Result<NavigationResult<Route>, NavigationError>) + 'static,
    ) {
        let depths = self.depths.clone();
        self.events.navigate(StackEvent::with_completion(
            move |old| {
                depths.borrow_mut().push(old.len());
                transformer(old)
            },
            completion,
        ));
    }
}

fn open_details<N: StackNavigator<Route>>(navigation: &N) {
    navigation.push_new_or_panic(Route::Details(7));
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed)?;
    let context = DefaultComponentContext::new(lifecycle);
    let navigation = AuditedNavigation {
        events: SimpleNavigation::new(),
        depths: Rc::new(RefCell::new(Vec::new())),
        connections: Rc::new(Cell::new(0)),
    };
    let source: &dyn NavigationSource<StackEvent<Route>> = &navigation;
    let stack = child_stack(
        &context,
        source,
        vec![Route::Home],
        StackOptions {
            handle_back_button: true,
            ..StackOptions::default()
        },
        |_, context| context,
    )?;
    assert_eq!(navigation.connections.get(), 1);
    open_details(&navigation);
    assert_eq!(*stack.get().active.configuration, Route::Details(7));

    let completed = Rc::new(Cell::new(false));
    let done = completed.clone();
    let published = stack.clone();
    navigation.navigate_with(
        |mut state| {
            state.push(Rc::new(Route::Settings));
            state
        },
        move |result| {
            let result = result.unwrap();
            assert_eq!(result.old_stack.len(), 2);
            assert_eq!(result.new_stack.len(), 3);
            assert_eq!(*published.get().active.configuration, Route::Settings);
            done.set(true);
        },
    );
    assert!(completed.get());
    assert_eq!(*navigation.depths.borrow(), vec![1, 2]);
    context.back_handler().back();
    assert_eq!(*stack.get().active.configuration, Route::Details(7));
    context.lifecycle().destroy()?;
    assert_eq!(navigation.connections.get(), 0);
    navigation.push_or_panic(Route::Settings); // No connected receiver: discarded.
    assert_eq!(*navigation.depths.borrow(), vec![1, 2]);
    println!("custom source through dyn NavigationSource: connected and cancelled");
    println!("generic navigator helpers, per-receiver completion and Back verified");
    Ok(())
}
