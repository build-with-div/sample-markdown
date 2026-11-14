//! Markdown to the element tree a browser shows for react-markdown.
//!
//! This is a port of react-markdown 10 with remark-gfm 4 and rehype-raw 7,
//! the web's usual way to show Markdown, rendered by React 19's
//! `renderToStaticMarkup`. [`parse`] returns the DOM that Chromium builds
//! from that markup: the same elements, attributes and text, in the same
//! order. Rendering it is the job of an engine (in this sample, Div); this
//! crate has no look of its own.
//!
//! The steps follow the reference pipeline:
//!
//! 1. comrak parses CommonMark and GFM (micromark's job in remark-parse and
//!    remark-gfm), and the `hast` module builds hast with mdast-util-to-hast's rules.
//! 2. html5ever parses that hast with the raw HTML in place, as rehype-raw
//!    does with parse5.
//! 3. The `react` module applies hastscript's property types, react-markdown's URL
//!    filter, hast-util-to-jsx-runtime's props and React's attribute rules.
//! 4. html5ever parses React's markup as Chromium does inside `<article>`.
//!
//! `tests/reference.rs` checks the result against corpora the npm packages
//! built (the fixtures, the CommonMark spec, GFM cases). The reference
//! pages highlight code, so `markdown-highlight` checks those.
//!
//! [`parse_with`] can add rehype-sanitize with its default schema, and
//! `@shikijs/rehype` with a [`Highlight`] (`markdown-highlight` ports Shiki).
//!
//! Not ported: React 19 hoists `<title>`, `<meta>` and `<link>` to the
//! start of the output, and comrak does not tell an empty link title from
//! a missing one.

mod dom;
mod hast;
mod react;
mod rehype_shiki;
mod sanitize;
mod uri;

/// The namespace of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Namespace {
    Html,
    Svg,
    MathMl,
}

/// A node of the tree: an element or text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
}

/// An element, with its attributes in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    pub namespace: Namespace,
    /// The local name, lowercase for HTML (`"blockquote"`, `"h2"`).
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Element {
    /// The value of an attribute.
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Options for [`parse_with`].
#[derive(Clone, Copy, Default)]
pub struct Options<'a> {
    /// Add rehype-sanitize with its default schema (GitHub's) after
    /// rehype-raw, for Markdown from untrusted sources. As in that pipeline,
    /// footnote ids get the `user-content-` prefix twice.
    pub sanitize: bool,
    /// Add `@shikijs/rehype` (its core, with default options and one
    /// theme) after them, highlighting code blocks with this highlighter.
    pub highlight: Option<&'a dyn Highlight>,
}

impl std::fmt::Debug for Options<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("sanitize", &self.sanitize)
            .field("highlight", &self.highlight.is_some())
            .finish()
    }
}

/// The Shiki highlighter `@shikijs/rehype` calls, with its theme chosen.
/// `markdown-highlight` implements it.
pub trait Highlight {
    /// `getLoadedLanguages().includes(lang) || isSpecialLang(lang)`: a
    /// loaded language or alias, a plain-text language or `ansi`.
    fn has_language(&self, lang: &str) -> bool;

    /// `codeToHast(code, { lang, theme })`: the root's children, attributes
    /// named and written as Shiki's hast properties (`class`, `style`,
    /// `tabindex`). `None` leaves the block as it is, where Shiki would
    /// throw or where the highlighter does not port Shiki.
    fn code_to_hast(&self, code: &str, lang: &str) -> Option<Vec<Node>>;
}

/// Parses Markdown into the nodes Chromium shows for react-markdown's output
/// (with remark-gfm and rehype-raw), as children of the container element.
pub fn parse(markdown: &str) -> Vec<Node> {
    parse_with(markdown, Options::default())
}

/// [`parse`] with options.
pub fn parse_with(markdown: &str, options: Options) -> Vec<Node> {
    let hast = hast::from_markdown(markdown);
    let mut html = String::new();
    hast::write_html(&hast, &mut html);
    let mut raw = dom::parse(&html, dom::Context::Template, |handles| {
        react::nodes(handles, options.sanitize)
    });
    if let Some(highlight) = options.highlight {
        rehype_shiki::apply(&mut raw, highlight);
    }
    parse_html(&to_html(&raw))
}

/// Parses HTML as Chromium parses it inside an `<article>` in a document's
/// body. The tests read the reference pages with it.
pub fn parse_html(html: &str) -> Vec<Node> {
    dom::parse(html, dom::Context::Article, dom::nodes)
}

/// Writes nodes as React's `renderToStaticMarkup` writes them.
pub fn to_html(nodes: &[Node]) -> String {
    let mut out = String::new();
    react::write_markup(nodes, false, &mut out);
    out
}
