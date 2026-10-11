//! Explicit configuration, owned value reads and fallible navigation.
use decompose_rs::{
    ComponentContext, DecomposeConfig, DefaultComponentContext, LifecycleRegistry, LifecycleState,
    router::{
        NavigationOutcome,
        stack::{StackCodec, StackNavigation, StackNavigator, StackOptions, child_stack},
    },
    value::MutableValue,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = DecomposeConfig::default(); // Silent, deterministic; no global reads.
    let lifecycle = LifecycleRegistry::new();
    lifecycle.move_to(LifecycleState::Resumed)?;
    let context = DefaultComponentContext::new(lifecycle.clone()).with_config(config.clone());

    let counter = MutableValue::new(1_u8);
    let increment = 2;
    counter.modify(|value| *value += increment); // May borrow this stack frame.
    println!("counter = {}", counter.get_cloned());
    let _subscription = counter
        .value()
        .subscribe_cloned(|value| println!("observed {value}"));

    let codec = StackCodec::new_fallible(
        |items: &[std::rc::Rc<u8>]| Ok(items.iter().map(|item| **item).collect()),
        |bytes| Ok(bytes.to_vec()),
    );
    let mut options = StackOptions::with_config(&config);
    options.codec = Some(codec);
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &context,
        &navigation,
        vec![1],
        options,
        |configuration, child| {
            child
                .state_store()
                .register("example", || vec![42])
                .expect("unique per-child key");
            format!("screen {configuration}")
        },
    )?;

    match navigation.try_push(2)? {
        NavigationOutcome::Completed(results) => println!("updated {} receiver(s)", results.len()),
        NavigationOutcome::Pending(receipt) => {
            // Possible when called during a surrounding dispatch/notification.
            // Keep the receipt and take_results() after that scope finishes.
            println!("accepted deferred navigation: {receipt:?}");
        }
    }
    let label = String::from("active");
    println!(
        "{}",
        stack.with(|state| format!("{label}: {}", state.active.instance))
    );
    let saved = context.state_store().save()?;
    println!("saved {} router entry", saved.len());
    lifecycle.destroy()?;
    Ok(())
}
