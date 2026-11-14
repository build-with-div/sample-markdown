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
//! `tests/reference.rs` checks the result against the committed reference
//! pages and the CommonMark spec corpus, both built by the npm packages.
//!
//! [`parse_with`] can add rehype-sanitize with its default schema.
//!
//! Not ported: React 19 hoists `<title>`, `<meta>` and `<link>` to the
//! start of the output, and comrak does not tell an empty link title from
//! a missing one.

mod dom;
mod hast;
mod react;
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Add rehype-sanitize with its default schema (GitHub's) after
    /// rehype-raw, for Markdown from untrusted sources. As in that pipeline,
    /// footnote ids get the `user-content-` prefix twice.
    pub sanitize: bool,
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
    let raw = dom::parse(&html, dom::Context::Template, |handles| {
        react::nodes(handles, options.sanitize)
    });
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
