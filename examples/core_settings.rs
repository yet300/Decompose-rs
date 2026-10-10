use decompose_rs::router::stack::{StackNavigation, StackOptions, child_stack};
use decompose_rs::{DecomposeSettings, TestComponentContext};
use std::rc::Rc;

fn main() {
    let settings = DecomposeSettings {
        duplicate_configurations_enabled: true,
        on_decompose_error: Rc::new(|error| println!("Observed diagnostic: {error}")),
    };
    DecomposeSettings::with(settings, || {
        let context = TestComponentContext::new();
        let source = StackNavigation::new();
        let stack = child_stack(
            &context,
            &source,
            vec!["home", "home"],
            StackOptions::default(),
            |_, _| (),
        )
        .unwrap();
        assert_eq!(stack.get().items().count(), 2);
        source.navigate_with(
            |_| Vec::new(),
            |result| {
                assert!(result.is_err());
                println!("The completion still receives the typed error.");
            },
        );
        assert_eq!(stack.get().items().count(), 2);
    });
    assert!(!DecomposeSettings::current().duplicate_configurations_enabled);
}
