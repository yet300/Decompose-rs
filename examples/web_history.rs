use decompose_rs::router::stack::{StackNavigation, StackNavigator, StackOptions, child_stack};
use decompose_rs::router::webhistory::{
    HistoryBackend, HistoryController, HistoryError, MemoryHistoryBackend, RouteCodec, RouteInfo,
    StackHistoryController, stack_web_navigation,
};
use decompose_rs::{ComponentContext, TestComponentContext};
use std::rc::Rc;

fn codec() -> RouteCodec<u8> {
    RouteCodec::new(
        |key| Ok(vec![*key]),
        |bytes| match bytes {
            [key] => Ok(*key),
            _ => Err(HistoryError::Codec("expected one byte".into())),
        },
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let context = TestComponentContext::new();
    let navigation = StackNavigation::new();
    let stack = child_stack(
        &context,
        &navigation,
        vec![1u8],
        StackOptions::default(),
        |_, _| (),
    )?;
    let backend = Rc::new(MemoryHistoryBackend::new());
    let web = stack_web_navigation(navigation.clone(), stack.clone(), codec(), true, |child| {
        let mut info = RouteInfo::path(format!("screen/{}", child.configuration));
        info.parameters.push(("draft".into(), "a b".into()));
        info
    });
    let binding = HistoryController::attach(Rc::new(web), backend.clone())?
        .with_lifecycle(&context.lifecycle());
    navigation.push_or_panic(2);
    assert_eq!(backend.entries()[1].url, "/screen/2?draft=a%20b");
    backend.go(-1, Box::new(|result| result.unwrap()));
    assert_eq!(*stack.get().active.configuration, 1);
    backend.go(1, Box::new(|result| result.unwrap()));
    assert_eq!(*stack.get().active.configuration, 2);
    binding.cancel();
    assert_eq!(backend.listener_count(), 0);

    // The older stack-only contract initializes one entry per configuration.
    let host = Rc::new(MemoryHistoryBackend::new());
    let legacy = StackHistoryController::new(
        host.clone(),
        codec(),
        |key| format!("/{key}"),
        |path| {
            path.trim_matches('/')
                .parse()
                .map_err(|error: std::num::ParseIntError| HistoryError::Codec(error.to_string()))
        },
    );
    let _binding = legacy.attach(navigation, stack, |target, _current| !target.is_empty())?;
    assert_eq!(legacy.history_paths()?, ["/1", "/2"]);
    println!("Host-independent Back/Forward, URL encoding and both history algorithms passed.");
    Ok(())
}
