//! The viewer's window content: a toolbar and the document, scrolled in a
//! container below it.
//!
//! The toolbar is the viewer's own UI and carries its own style. The
//! document is the Markdown as `markdown-tree` parses it, mounted by
//! `markdown-div` with no style: github-markdown-css cannot load into Div
//! yet (MD-3, MD-4, MD-5), so the document shows Div's defaults for HTML
//! elements. Where the web platform would do more, the code says
//! `div-gap`.

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use div::typed_style::parse_literal;
use div::{Document, Element, StyleProperty};

/// The viewer's color theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }
}

pub struct Viewer {
    pub document: Document,
    pub toolbar: Element,
    /// Present when the viewer can show a native Open dialog.
    pub open_button: Option<Element>,
    pub theme_button: Element,
    pub title: Element,
    pub status: Element,
    /// The scroll container holding the article.
    pub scroller: Element,
    /// `<article class="markdown-body">`, as on the reference pages.
    pub article: Element,
    theme: Cell<Theme>,
    path: RefCell<Option<PathBuf>>,
}

type Result<T> = std::result::Result<T, Box<dyn Error>>;

impl Viewer {
    /// Builds the viewer in `document`'s root. `with_open` adds an Open
    /// button, which the caller connects to a dialog.
    pub fn new(document: &Document, with_open: bool) -> Result<Rc<Self>> {
        let root = Element::create(document, "div")?;
        style(
            &root,
            &[
                ("display", "flex"),
                ("flex-direction", "column"),
                ("height", "100vh"),
            ],
        )?;
        let toolbar = Element::create(document, "div")?;
        style(
            &toolbar,
            &[
                ("display", "flex"),
                ("align-items", "center"),
                ("column-gap", "8px"),
                ("padding-top", "6px"),
                ("padding-bottom", "6px"),
                ("padding-left", "12px"),
                ("padding-right", "12px"),
                ("flex-shrink", "0"),
                ("font-family", "sans-serif"),
                ("font-size", "13px"),
            ],
        )?;
        let open_button = if with_open {
            let button = button(document, "Open…")?;
            toolbar.append_child(&button)?;
            Some(button)
        } else {
            None
        };
        let theme_button = button(document, "Dark")?;
        toolbar.append_child(&theme_button)?;
        let title = Element::create(document, "span")?;
        style(&title, &[("font-weight", "600")])?;
        toolbar.append_child(&title)?;
        let status = Element::create(document, "span")?;
        style(&status, &[("flex-grow", "1"), ("text-align", "right")])?;
        toolbar.append_child(&status)?;
        root.append_child(&toolbar)?;

        let scroller = Element::create(document, "div")?;
        style(
            &scroller,
            &[
                ("flex-grow", "1"),
                ("flex-basis", "0px"),
                ("min-height", "0px"),
                ("overflow-y", "auto"),
            ],
        )?;
        // The wheel and the scroll bar scroll it. Keys do not, focused or
        // not, and the viewer does not script them in Div's place.
        // div-gap: MD-23 keyboard scrolling and keyboard-focusable scrollers.
        // Links in the document are plain inline elements: clicking one
        // does nothing. div-gap: MD-17 `a[href]` activation, fragment
        // navigation and opening URLs.
        let article = Element::create(document, "article")?;
        article.set_attribute("class", "markdown-body")?;
        // The page style reference/build.mjs gives the article (the one
        // github-markdown-css's readme recommends). It is the viewer's own
        // layout, not the package's look. Its `max-width: 767px` media
        // query, 15 px padding on narrow windows, needs media queries
        // (div-gap: MD-6 media queries).
        style(
            &article,
            &[
                ("box-sizing", "border-box"),
                ("min-width", "200px"),
                ("max-width", "980px"),
                ("margin-left", "auto"),
                ("margin-right", "auto"),
                ("padding-top", "45px"),
                ("padding-bottom", "45px"),
                ("padding-left", "45px"),
                ("padding-right", "45px"),
            ],
        )?;
        scroller.append_child(&article)?;
        root.append_child(&scroller)?;
        document.body().append_child(&root)?;

        let viewer = Rc::new(Self {
            document: document.clone(),
            toolbar,
            open_button,
            theme_button: theme_button.clone(),
            title,
            status,
            scroller,
            article,
            theme: Cell::new(Theme::Light),
            path: RefCell::new(None),
        });
        viewer.set_theme(Theme::Light)?;
        let weak = Rc::downgrade(&viewer);
        theme_button.on("click", move |_| {
            if let Some(viewer) = weak.upgrade() {
                let theme = viewer.theme().other();
                if let Err(error) = viewer.set_theme(theme) {
                    viewer.show_status(&format!("Theme: {error}"));
                }
            }
        })?;
        Ok(viewer)
    }

    pub fn theme(&self) -> Theme {
        self.theme.get()
    }

    #[cfg_attr(not(any(feature = "linux", feature = "windows")), allow(dead_code))]
    pub fn path(&self) -> Option<PathBuf> {
        self.path.borrow().clone()
    }

    /// Switches the theme.
    ///
    /// The toolbar follows it. The document is marked with `data-theme`,
    /// the attribute github-markdown-css's themes select on, but nothing
    /// in Div reads it: the package's colors need its stylesheet and custom
    /// properties (MD-4, MD-5). The viewer also cannot start in the
    /// system's theme. div-gap: MD-6 `prefers-color-scheme` and the
    /// document's color scheme.
    pub fn set_theme(&self, theme: Theme) -> Result<()> {
        self.theme.set(theme);
        self.article.set_attribute("data-theme", theme.name())?;
        let (background, color, button_background) = match theme {
            Theme::Light => ("#f6f8fa", "#1f2328", "#e6eaef"),
            Theme::Dark => ("#151b23", "#f0f6fc", "#2a313c"),
        };
        style(
            &self.toolbar,
            &[("background-color", background), ("color", color)],
        )?;
        for button in self.open_button.iter().chain([&self.theme_button]) {
            style(
                button,
                &[("background-color", button_background), ("color", color)],
            )?;
        }
        self.theme_button
            .set_text(theme.other().name_capitalized())?;
        Ok(())
    }

    /// Reads and shows a Markdown file. Relative image paths load from its
    /// directory.
    pub fn open(&self, path: &Path) -> Result<()> {
        let markdown = std::fs::read_to_string(path)?.replace("\r\n", "\n");
        let nodes = markdown_tree::parse(&markdown);
        let loader =
            markdown_div::directory_loader(path.parent().unwrap_or(Path::new(".")).to_owned());
        self.article.remove_all_children()?;
        let mounted = markdown_div::mount(&self.document, &self.article, &nodes, Some(&loader))?;
        *self.path.borrow_mut() = Some(path.to_owned());
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.title.set_text(&name)?;
        self.show_status(&summary(mounted.elements.len(), &mounted.report));
        self.scroller.scroll_to(0.0, 0.0)?;
        Ok(())
    }

    pub fn show_status(&self, text: &str) {
        // The status line is best effort: an error setting it has nowhere
        // else to go.
        let _ = self.status.set_text(text);
    }
}

impl Theme {
    fn name_capitalized(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// One line on what the document needed from Div and did not get.
pub fn summary(elements: usize, report: &markdown_div::Report) -> String {
    let mut parts = vec![format!("{elements} elements")];
    if !report.stand_ins.is_empty() {
        let tags: Vec<String> = report
            .stand_ins
            .iter()
            .map(|(tag, (_, count))| format!("{tag} {count}"))
            .collect();
        parts.push(format!("stand-ins (div-gap G-TAGS): {}", tags.join(", ")));
    }
    if !report.images_not_loaded.is_empty() {
        parts.push(format!(
            "images not loaded: {}",
            report.images_not_loaded.len()
        ));
    }
    parts.join(" · ")
}

fn button(document: &Document, label: &str) -> Result<Element> {
    let button = Element::create(document, "button")?;
    button.set_text(label)?;
    style(
        &button,
        &[
            ("padding-top", "3px"),
            ("padding-bottom", "3px"),
            ("padding-left", "10px"),
            ("padding-right", "10px"),
            ("font-family", "sans-serif"),
            ("font-size", "13px"),
        ],
    )?;
    Ok(button)
}

/// Sets the viewer's own style declarations.
fn style(element: &Element, declarations: &[(&str, &str)]) -> Result<()> {
    for (name, value) in declarations {
        let property =
            StyleProperty::from_css_name(name).ok_or_else(|| format!("unknown property {name}"))?;
        let value = parse_literal(property, value).map_err(|error| format!("{name}: {error}"))?;
        element.set_property(property, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
