//! Markdown to hast, as remark-parse, remark-gfm and remark-rehype
//! (mdast-util-to-hast 13) build it, then written as HTML for rehype-raw.
//!
//! comrak parses the GFM source. The handlers below follow
//! mdast-util-to-hast's, function by function, on comrak's AST.

use std::collections::HashMap;

use comrak::nodes::{AstNode, ListType, NodeValue, TableAlignment};
use comrak::{parse_document, Arena, Options};

use crate::uri::normalize_uri;

/// A hast node before rehype-raw: elements and text, plus raw HTML from
/// the Markdown source.
#[derive(Debug)]
pub(crate) enum H {
    Element(HElement),
    Text(String),
    Raw(String),
}

#[derive(Debug)]
pub(crate) struct HElement {
    pub name: &'static str,
    /// Attributes as hast-util-to-parse5 writes them: `true` becomes the
    /// empty string, `false` is left out.
    pub attributes: Vec<(&'static str, String)>,
    pub children: Vec<H>,
}

fn element(name: &'static str, attributes: Vec<(&'static str, String)>, children: Vec<H>) -> H {
    H::Element(HElement {
        name,
        attributes,
        children,
    })
}

fn text(value: impl Into<String>) -> H {
    H::Text(value.into())
}

/// comrak's options for remark-parse with remark-gfm: CommonMark plus GFM
/// tables, task lists, strikethrough, autolink literals and footnotes.
fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.footnotes = true;
    options
}

/// Parses Markdown and builds the hast tree that remark-rehype gives
/// rehype-raw (the root's children).
pub(crate) fn from_markdown(markdown: &str) -> Vec<H> {
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &options());
    let mut state = State::default();
    for node in root.descendants() {
        if let NodeValue::FootnoteDefinition(definition) = &node.data().value {
            state
                .footnote_by_id
                .entry(footnote_id(&definition.name))
                .or_insert(node);
        }
    }
    let mut children = wrap(state.all(root), false);
    if let Some(footer) = state.footer() {
        children.push(text("\n"));
        children.push(footer);
    }
    children
}

/// `normalizeIdentifier` then the upper-casing mdast-util-to-hast keys
/// footnotes by.
fn footnote_id(label: &str) -> String {
    let collapsed = label
        .split([' ', '\t', '\n', '\r'])
        .filter(|s| !s.is_empty());
    collapsed
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .to_uppercase()
}

/// mdast-util-to-hast's `wrap`: a line ending between nodes, and around
/// them when `loose`.
fn wrap(nodes: Vec<H>, loose: bool) -> Vec<H> {
    let mut result = Vec::with_capacity(nodes.len() * 2 + 1);
    if loose {
        result.push(text("\n"));
    }
    let count = nodes.len();
    for (index, node) in nodes.into_iter().enumerate() {
        if index > 0 {
            result.push(text("\n"));
        }
        result.push(node);
    }
    if loose && count > 0 {
        result.push(text("\n"));
    }
    result
}

#[derive(Default)]
struct State<'a> {
    footnote_by_id: HashMap<String, &'a AstNode<'a>>,
    footnote_order: Vec<String>,
    footnote_counts: HashMap<String, usize>,
}

impl<'a> State<'a> {
    fn all(&mut self, node: &'a AstNode<'a>) -> Vec<H> {
        let mut result = Vec::new();
        for child in node.children() {
            result.extend(self.one(child));
        }
        result
    }

    fn one(&mut self, node: &'a AstNode<'a>) -> Vec<H> {
        let value = node.data().value.clone();
        let single = |h| vec![h];
        match value {
            NodeValue::Document => self.all(node),
            NodeValue::BlockQuote => {
                single(element("blockquote", vec![], wrap(self.all(node), true)))
            }
            NodeValue::List(list) => {
                let items = self.all(node);
                let mut attributes = vec![];
                if list.list_type == ListType::Ordered && list.start != 1 {
                    attributes.push(("start", list.start.to_string()));
                }
                let task = items.iter().any(|item| match item {
                    H::Element(e) => e
                        .attributes
                        .iter()
                        .any(|(k, v)| *k == "class" && v == "task-list-item"),
                    _ => false,
                });
                if task {
                    attributes.push(("class", "contains-task-list".into()));
                }
                let name = if list.list_type == ListType::Ordered {
                    "ol"
                } else {
                    "ul"
                };
                single(element(name, attributes, wrap(items, true)))
            }
            NodeValue::Item(_) => single(self.list_item(node, None)),
            NodeValue::TaskItem(task) => single(self.list_item(node, Some(task.symbol))),
            NodeValue::CodeBlock(code) => {
                let mut lang = code
                    .info
                    .split([' ', '\t', '\n', '\r'])
                    .next()
                    .unwrap_or("");
                if !code.fenced {
                    lang = "";
                }
                let mut value = code
                    .literal
                    .strip_suffix('\n')
                    .unwrap_or(&code.literal)
                    .to_owned();
                if !value.is_empty() {
                    value.push('\n');
                }
                // rehype-raw passes hast to parse5 through `_processToken`,
                // which skips `onStartTag`'s reset of `skipNextNewLine`: the
                // newline parse5 drops after `<pre>` is the code's first.
                if let Some(rest) = value.strip_prefix('\n') {
                    value = rest.to_owned();
                }
                let attributes = if lang.is_empty() {
                    vec![]
                } else {
                    vec![("class", format!("language-{lang}"))]
                };
                let code = element("code", attributes, vec![text(value)]);
                single(element("pre", vec![], vec![code]))
            }
            NodeValue::HtmlBlock(html) => {
                // micromark keeps the line endings of an HTML block of
                // kinds 1-5 that runs to the end of the input unclosed.
                let closed = html.literal.strip_suffix('\n').unwrap_or(&html.literal);
                let last_line = closed
                    .rsplit('\n')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let ended = match html.block_type {
                    1 => ["</script>", "</pre>", "</style>", "</textarea>"]
                        .iter()
                        .any(|end| last_line.contains(end)),
                    2 => last_line.contains("-->"),
                    3 => last_line.contains("?>"),
                    4 => last_line.contains('>'),
                    5 => last_line.contains("]]>"),
                    _ => true,
                };
                single(H::Raw(
                    if ended { closed } else { &html.literal }.to_owned(),
                ))
            }
            NodeValue::Paragraph => single(element("p", vec![], self.all(node))),
            NodeValue::Heading(heading) => {
                const NAMES: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];
                single(element(
                    NAMES[usize::from(heading.level.clamp(1, 6)) - 1],
                    vec![],
                    self.all(node),
                ))
            }
            NodeValue::ThematicBreak => single(element("hr", vec![], vec![])),
            NodeValue::FootnoteDefinition(_) => vec![],
            NodeValue::Table(table) => single(self.table(node, &table.alignments)),
            NodeValue::Text(value) => single(text(value.as_ref())),
            NodeValue::SoftBreak => single(text("\n")),
            NodeValue::LineBreak => vec![element("br", vec![], vec![]), text("\n")],
            NodeValue::Code(code) => single(element("code", vec![], vec![text(code.literal)])),
            NodeValue::HtmlInline(html) => single(H::Raw(html)),
            NodeValue::Emph => single(element("em", vec![], self.all(node))),
            NodeValue::Strong => single(element("strong", vec![], self.all(node))),
            NodeValue::Strikethrough => single(element("del", vec![], self.all(node))),
            NodeValue::Link(link) => {
                let mut attributes = vec![("href", normalize_uri(&link.url))];
                // Known difference: comrak does not tell a missing title
                // from an empty one; mdast keeps `title=""`.
                if !link.title.is_empty() {
                    attributes.push(("title", link.title.clone()));
                }
                single(element("a", attributes, self.all(node)))
            }
            NodeValue::Image(link) => {
                let mut attributes =
                    vec![("src", normalize_uri(&link.url)), ("alt", plain_text(node))];
                if !link.title.is_empty() {
                    attributes.push(("title", link.title.clone()));
                }
                single(element("img", attributes, vec![]))
            }
            NodeValue::FootnoteReference(reference) => {
                single(self.footnote_reference(&reference.name))
            }
            // Nodes the options above do not produce: keep their content.
            _ => self.all(node),
        }
    }

    /// mdast-util-to-hast's `listItem`, with remark-gfm's task item.
    /// `task` is comrak's task marker: `Some(None)` for `[ ]`,
    /// `Some(Some('x'))` for `[x]`.
    fn list_item(&mut self, node: &'a AstNode<'a>, task: Option<Option<char>>) -> H {
        let loose = node
            .parent()
            .is_some_and(|parent| match &parent.data().value {
                NodeValue::List(list) => !list.tight,
                _ => false,
            });
        let mut results = self.all(node);
        let mut attributes = vec![];
        let mut checked = task.map(|symbol| symbol.is_some());
        // remark-gfm wants content after the marker; comrak also accepts
        // `- [x]` alone. Then the marker is text, as in mdast.
        if let Some(symbol) = task {
            if !has_content(results.first()) {
                checked = None;
                let marker = text(format!("[{}]", symbol.unwrap_or(' ')));
                match results.first_mut() {
                    Some(H::Element(p)) if p.name == "p" => p.children.insert(0, marker),
                    _ => results.insert(0, element("p", vec![], vec![marker])),
                }
            }
        }
        if let Some(checked) = checked {
            let head_is_p = matches!(results.first(), Some(H::Element(e)) if e.name == "p");
            if !head_is_p {
                results.insert(0, element("p", vec![], vec![]));
            }
            let Some(H::Element(paragraph)) = results.first_mut() else {
                unreachable!()
            };
            // mdast-util-gfm-task-list-item drops the first character after
            // the marker. comrak has dropped a space or tab already, but not
            // a line ending.
            paragraph
                .children
                .retain(|child| !matches!(child, H::Text(value) if value.is_empty()));
            if let Some(H::Text(value)) = paragraph.children.first_mut() {
                if let Some(rest) = value.strip_prefix('\n') {
                    *value = rest.to_owned();
                }
            }
            paragraph
                .children
                .retain(|child| !matches!(child, H::Text(value) if value.is_empty()));
            if !paragraph.children.is_empty() {
                paragraph.children.insert(0, text(" "));
            }
            let mut input = vec![("type", "checkbox".to_owned())];
            if checked {
                input.push(("checked", String::new()));
            }
            input.push(("disabled", String::new()));
            paragraph
                .children
                .insert(0, element("input", input, vec![]));
            attributes.push(("class", "task-list-item".to_owned()));
        }
        let mut children = Vec::new();
        let count = results.len();
        let mut tail_is_p = false;
        for (index, child) in results.into_iter().enumerate() {
            let is_p = matches!(&child, H::Element(e) if e.name == "p");
            if loose || index != 0 || !is_p {
                children.push(text("\n"));
            }
            tail_is_p = is_p;
            match child {
                H::Element(e) if is_p && !loose => children.extend(e.children),
                other => children.push(other),
            }
        }
        if count > 0 && (loose || !tail_is_p) {
            children.push(text("\n"));
        }
        element("li", attributes, children)
    }

    /// mdast-util-to-hast's `table` and `tableRow`.
    fn table(&mut self, node: &'a AstNode<'a>, alignments: &[TableAlignment]) -> H {
        let mut rows = Vec::new();
        for (row_index, row) in node.children().enumerate() {
            let tag = if row_index == 0 { "th" } else { "td" };
            let cells: Vec<_> = row.children().collect();
            let mut out = Vec::new();
            for (cell_index, alignment) in alignments.iter().enumerate() {
                let mut attributes = vec![];
                let align = match alignment {
                    TableAlignment::Left => Some("left"),
                    TableAlignment::Center => Some("center"),
                    TableAlignment::Right => Some("right"),
                    TableAlignment::None => None,
                };
                if let Some(align) = align {
                    attributes.push(("align", align.to_owned()));
                }
                let children = cells
                    .get(cell_index)
                    .map(|cell| self.all(cell))
                    .unwrap_or_default();
                out.push(element(tag, attributes, children));
            }
            rows.push(element("tr", vec![], wrap(out, true)));
        }
        let mut content = Vec::new();
        let mut rows = rows.into_iter();
        if let Some(first) = rows.next() {
            content.push(element("thead", vec![], wrap(vec![first], true)));
        }
        let body: Vec<_> = rows.collect();
        if !body.is_empty() {
            content.push(element("tbody", vec![], wrap(body, true)));
        }
        element("table", vec![], wrap(content, true))
    }

    /// mdast-util-to-hast's `footnoteReference`, with the default
    /// `clobberPrefix` (`user-content-`).
    fn footnote_reference(&mut self, label: &str) -> H {
        let id = footnote_id(label);
        let safe_id = normalize_uri(&id.to_lowercase());
        let counter = match self.footnote_order.iter().position(|known| *known == id) {
            Some(index) => index + 1,
            None => {
                self.footnote_order.push(id.clone());
                self.footnote_order.len()
            }
        };
        let reuse = self.footnote_counts.entry(id).or_insert(0);
        *reuse += 1;
        let suffix = if *reuse > 1 {
            format!("-{reuse}")
        } else {
            String::new()
        };
        let link = element(
            "a",
            vec![
                ("href", format!("#user-content-fn-{safe_id}")),
                ("id", format!("user-content-fnref-{safe_id}{suffix}")),
                ("data-footnote-ref", String::new()),
                ("aria-describedby", "footnote-label".into()),
            ],
            vec![text(counter.to_string())],
        );
        element("sup", vec![], vec![link])
    }

    /// mdast-util-to-hast's `footer`, with its default labels.
    fn footer(&mut self) -> Option<H> {
        let mut items = Vec::new();
        let mut reference_index = 0;
        while reference_index < self.footnote_order.len() {
            let id = self.footnote_order[reference_index].clone();
            reference_index += 1;
            let Some(&definition) = self.footnote_by_id.get(&id) else {
                continue;
            };
            let mut content = self.all(definition);
            let safe_id = normalize_uri(&id.to_lowercase());
            let counts = self.footnote_counts.get(&id).copied().unwrap_or(0);
            let mut back = Vec::new();
            for rereference in 1..=counts {
                if !back.is_empty() {
                    back.push(text(" "));
                }
                let suffix = if rereference > 1 {
                    format!("-{rereference}")
                } else {
                    String::new()
                };
                let mut children = vec![text("↩")];
                if rereference > 1 {
                    children.push(element("sup", vec![], vec![text(rereference.to_string())]));
                }
                back.push(element(
                    "a",
                    vec![
                        ("href", format!("#user-content-fnref-{safe_id}{suffix}")),
                        ("data-footnote-backref", String::new()),
                        (
                            "aria-label",
                            format!("Back to reference {reference_index}{suffix}"),
                        ),
                        ("class", "data-footnote-backref".into()),
                    ],
                    children,
                ));
            }
            match content.last_mut() {
                Some(H::Element(tail)) if tail.name == "p" => {
                    match tail.children.last_mut() {
                        Some(H::Text(value)) => value.push(' '),
                        _ => tail.children.push(text(" ")),
                    }
                    tail.children.extend(back);
                }
                _ => content.extend(back),
            }
            items.push(element(
                "li",
                vec![("id", format!("user-content-fn-{safe_id}"))],
                wrap(content, true),
            ));
        }
        if items.is_empty() {
            return None;
        }
        let label = element(
            "h2",
            vec![("class", "sr-only".into()), ("id", "footnote-label".into())],
            vec![text("Footnotes")],
        );
        Some(element(
            "section",
            vec![
                ("data-footnotes", String::new()),
                ("class", "footnotes".into()),
            ],
            vec![
                label,
                text("\n"),
                element("ol", vec![], wrap(items, true)),
                text("\n"),
            ],
        ))
    }
}

/// Whether a task item's first paragraph has content after the marker.
fn has_content(first: Option<&H>) -> bool {
    let Some(H::Element(p)) = first else {
        return false;
    };
    p.name == "p"
        && p.children.iter().any(|child| match child {
            H::Text(value) => !value.trim_matches([' ', '\t', '\n']).is_empty(),
            H::Element(e) => e.name != "br",
            H::Raw(_) => true,
        })
}

/// mdast-util-to-string over an image's label, which is the `alt`.
fn plain_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut out = String::new();
    for child in node.children() {
        match &child.data().value {
            NodeValue::Text(value) => out.push_str(value),
            NodeValue::Code(code) => out.push_str(&code.literal),
            NodeValue::HtmlInline(html) => out.push_str(html),
            NodeValue::SoftBreak => out.push('\n'),
            NodeValue::Image(_) => out.push_str(&plain_text(child)),
            _ => out.push_str(&plain_text(child)),
        }
    }
    out
}

const VOID: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "command", "embed", "frame", "hr", "image",
    "img", "input", "keygen", "link", "meta", "param", "source", "track", "wbr",
];

/// Writes hast as HTML. Text and attributes are escaped, raw HTML is kept,
/// so that parsing it again gives what rehype-raw's token stream gives.
pub(crate) fn write_html(nodes: &[H], out: &mut String) {
    for node in nodes {
        match node {
            H::Text(value) => escape(value, false, out),
            H::Raw(value) => out.push_str(value),
            H::Element(e) => {
                out.push('<');
                out.push_str(e.name);
                for (name, value) in &e.attributes {
                    out.push(' ');
                    out.push_str(name);
                    out.push_str("=\"");
                    escape(value, true, out);
                    out.push('"');
                }
                out.push('>');
                if !VOID.contains(&e.name) {
                    write_html(&e.children, out);
                    out.push_str("</");
                    out.push_str(e.name);
                    out.push('>');
                }
            }
        }
    }
}

pub(crate) fn escape(value: &str, attribute: bool, out: &mut String) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' if !attribute => out.push_str("&lt;"),
            '>' if !attribute => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            c => out.push(c),
        }
    }
}
