//! Interaction traces through Div's input dispatch, headless, at 900×700.
//!
//! The `div_gap_*` tests pin what Div does not do yet. Each fails when Div
//! closes its gap, so the gap entry and the test can then change together.

use super::*;
use div::{
    EngineOptions, Key, KeyEventType, Modifiers, MouseButton, MouseEventType, ViewportMetrics,
};

fn viewer() -> Rc<Viewer> {
    let document = Document::with_options(
        ViewportMetrics::from_logical_size(900.0, 700.0, 1.0).unwrap(),
        EngineOptions::default(),
    )
    .unwrap();
    Viewer::new(&document, false).unwrap()
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn centre(element: &Element) -> (f32, f32) {
    let rect = element.bounding_rect().unwrap().expect("a box");
    (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// Move, press and release the primary button, as a user clicks.
fn click(document: &Document, (x, y): (f32, f32)) {
    for kind in [
        MouseEventType::Move,
        MouseEventType::Down,
        MouseEventType::Up,
    ] {
        document
            .dispatch_mouse_event(kind, x, y, MouseButton::Left, Modifiers::NONE)
            .unwrap();
    }
    document.update_all().unwrap();
}

fn press(document: &Document, key: Key) {
    for kind in [KeyEventType::Down, KeyEventType::Up] {
        document
            .dispatch_key(kind, key, Modifiers::NONE, false)
            .unwrap();
    }
    document.update_all().unwrap();
}

/// A document taller than the window, with a footnote far below its
/// reference.
fn long_document() -> PathBuf {
    let mut markdown = String::from("A reference.[^1]\n\n");
    for i in 0..60 {
        markdown.push_str(&format!("Paragraph {i} of filler text.\n\n"));
    }
    markdown.push_str("[^1]: The footnote.\n");
    let directory = std::env::temp_dir().join(format!(
        "markdown-viewer-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("long.md");
    std::fs::write(&path, markdown).unwrap();
    path
}

#[test]
fn opens_a_file_and_reports_what_div_cannot_take() {
    let viewer = viewer();
    viewer.open(&fixture("readme.md")).unwrap();
    viewer.document.update_all().unwrap();
    assert_eq!(viewer.title.text_content().unwrap(), "readme.md");
    let status = viewer.status.text_content().unwrap();
    assert!(status.contains("stand-ins (div-gap G-TAGS)"), "{status}");
    assert!(status.contains("blockquote"), "{status}");
    assert_eq!(
        viewer
            .article
            .get_attribute("data-theme")
            .unwrap()
            .as_deref(),
        Some("light")
    );
}

#[test]
fn the_wheel_scrolls_the_document() {
    let viewer = viewer();
    viewer.open(&long_document()).unwrap();
    viewer.document.update_all().unwrap();
    assert!(viewer.scroller.scroll_height().unwrap() > 700);
    let (x, y) = centre(&viewer.scroller);
    viewer
        .document
        .dispatch_wheel_event(x, y, 0.0, 120.0, Modifiers::NONE)
        .unwrap();
    viewer.document.update_all().unwrap();
    assert_eq!(viewer.scroller.scroll_top().unwrap(), 120.0);
}

#[test]
fn the_theme_button_switches_the_theme() {
    let viewer = viewer();
    viewer.document.update_all().unwrap();
    assert_eq!(viewer.theme_button.text_content().unwrap(), "Dark");
    click(&viewer.document, centre(&viewer.theme_button));
    assert_eq!(viewer.theme(), Theme::Dark);
    assert_eq!(
        viewer
            .article
            .get_attribute("data-theme")
            .unwrap()
            .as_deref(),
        Some("dark")
    );
    assert_eq!(viewer.theme_button.text_content().unwrap(), "Light");
}

#[test]
fn opening_another_file_replaces_the_document() {
    let viewer = viewer();
    viewer.open(&fixture("readme.md")).unwrap();
    viewer.open(&fixture("hr.md")).unwrap();
    let expected: String = markdown_tree::parse(
        &std::fs::read_to_string(fixture("hr.md"))
            .unwrap()
            .replace("\r\n", "\n"),
    )
    .iter()
    .map(text)
    .collect();
    assert_eq!(viewer.article.text_content().unwrap(), expected);
}

fn text(node: &markdown_tree::Node) -> String {
    match node {
        markdown_tree::Node::Text(text) => text.clone(),
        markdown_tree::Node::Element(element) => element.children.iter().map(text).collect(),
    }
}

/// MD-23: no key scrolls a scroll container, not even a focused one, and
/// a scroll container is not focusable without `tabindex` (Chromium makes
/// it keyboard-focusable when it has no focusable content).
#[test]
fn div_gap_md23_keys_do_not_scroll() {
    let viewer = viewer();
    viewer.open(&long_document()).unwrap();
    viewer.document.update_all().unwrap();
    click(&viewer.document, centre(&viewer.scroller));
    assert!(viewer.scroller.focus().is_err());
    for key in [Key::PageDown, Key::ArrowDown, Key::Space, Key::End] {
        press(&viewer.document, key);
        assert_eq!(viewer.scroller.scroll_top().unwrap(), 0.0, "{key:?}");
    }
    viewer.scroller.set_attribute("tabindex", "0").unwrap();
    viewer.scroller.focus().unwrap();
    for key in [Key::PageDown, Key::ArrowDown, Key::Space, Key::End] {
        press(&viewer.document, key);
        assert_eq!(
            viewer.scroller.scroll_top().unwrap(),
            0.0,
            "focused, {key:?}"
        );
    }
}

/// MD-17: a click on a footnote reference, `<a href="#user-content-fn-1">`,
/// does not scroll to the footnote.
#[test]
fn div_gap_md17_fragment_links_do_not_navigate() {
    let viewer = viewer();
    viewer.open(&long_document()).unwrap();
    viewer.document.update_all().unwrap();
    let link = viewer
        .document
        .element_by_id("user-content-fnref-1")
        .unwrap()
        .expect("the footnote reference");
    assert_eq!(
        link.get_attribute("href").unwrap().as_deref(),
        Some("#user-content-fn-1")
    );
    let note = viewer
        .document
        .element_by_id("user-content-fn-1")
        .unwrap()
        .expect("the footnote");
    let below = note.bounding_rect().unwrap().unwrap().y;
    assert!(below > 700.0);
    click(&viewer.document, centre(&link));
    assert_eq!(viewer.scroller.scroll_top().unwrap(), 0.0);
    assert_eq!(note.bounding_rect().unwrap().unwrap().y, below);
}

/// MD-15: images load through the viewer's loader, but Div takes no size
/// from their data, so each is 300×150.
#[test]
fn div_gap_md15_images_have_no_intrinsic_size() {
    let viewer = viewer();
    viewer.open(&fixture("image.md")).unwrap();
    viewer.document.update_all().unwrap();
    let status = viewer.status.text_content().unwrap();
    assert!(!status.contains("images not loaded"), "{status}");
    let mut sizes = Vec::new();
    collect_image_sizes(&viewer.article, &mut sizes);
    assert_eq!(sizes, [(300.0, 150.0), (300.0, 150.0)]);
}

fn collect_image_sizes(element: &Element, sizes: &mut Vec<(f32, f32)>) {
    if element.get_attribute("src").unwrap().is_some() {
        let rect = element.bounding_rect().unwrap().unwrap();
        sizes.push((rect.width, rect.height));
    }
    let mut child = element.first_child().unwrap();
    while let Some(element) = child {
        collect_image_sizes(&element, sizes);
        child = element.next_sibling().unwrap();
    }
}
