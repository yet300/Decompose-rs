//! Restore page selection through a whole-state codec while retaining child state.
use decompose_rs::{
    TestComponentContext,
    router::pages::{Pages, PagesCodec, PagesNavigation, PagesOptions, child_pages_with_initial},
};
use std::rc::Rc;

fn main() {
    let parent = TestComponentContext::new();
    let codec = PagesCodec::new_state(
        |pages: &Pages<u8>| {
            let mut bytes = vec![pages.selected_index.unwrap_or(255) as u8];
            bytes.extend(pages.items.iter().map(|c| **c));
            Ok(bytes)
        },
        |bytes| {
            let (_, items) = bytes.split_first().ok_or("missing selection")?;
            // Choose the first page on recreation, retaining saved membership
            // and the nested child state associated with every page.
            Ok(Pages {
                items: items.iter().copied().map(Rc::new).collect(),
                selected_index: (!items.is_empty()).then_some(0),
            })
        },
    );
    let pages = child_pages_with_initial(
        &parent,
        &PagesNavigation::new(),
        || Pages::new(vec![10, 20], 1).unwrap(),
        PagesOptions {
            codec: Some(codec.clone()),
            ..Default::default()
        },
        |_, context| context,
    )
    .unwrap();
    assert_eq!(pages.get().selected_index, Some(1));
    let recreated = parent.recreate(false).unwrap();
    let restored = child_pages_with_initial(
        &recreated,
        &PagesNavigation::new(),
        || panic!("restoration should skip the initializer"),
        PagesOptions {
            codec: Some(codec),
            ..Default::default()
        },
        |_, context| context,
    )
    .unwrap();
    assert_eq!(restored.get().selected_index, Some(0));
    assert_eq!(restored.get().items.len(), 2);
    println!("Restored both pages with selection changed from 1 to 0.");
}
