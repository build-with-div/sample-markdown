//! `@shikijs/rehype` 4.6.0's core, `rehypeShikiFromHighlighter`, with its
//! default options and one theme: each `<pre>` whose first child is a
//! `<code class="language-…">` in a loaded language becomes Shiki's
//! `codeToHast` output.
//!
//! The plugin runs on rehype's tree after rehype-raw and rehype-sanitize.
//! Here it runs on the tree after the React step, which has not touched
//! `pre` or `code`; the nodes it puts in go through that step's attribute
//! rules (`class`, `style`, `tabindex`) as hast-util-to-jsx-runtime would
//! take them. The highlighter is a [`Highlight`](crate::Highlight).

use crate::react;
use crate::{Element, Highlight, Namespace, Node};

/// `visit` visits elements top-down and skips the replacement's subtree.
pub(crate) fn apply(nodes: &mut Vec<Node>, highlight: &dyn Highlight) {
    let mut index = 0;
    while index < nodes.len() {
        if let Node::Element(element) = &mut nodes[index] {
            if let Some(replacement) = highlight_pre(element, highlight) {
                let count = replacement.len();
                nodes.splice(index..=index, replacement);
                index += count;
                continue;
            }
            apply(&mut element.children, highlight);
        }
        index += 1;
    }
}

fn text(nodes: &[Node], out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(value) => out.push_str(value),
            Node::Element(element) => text(&element.children, out),
        }
    }
}

/// `PreHandler`, the language checks and `highlight` for one element.
fn highlight_pre(element: &Element, highlight: &dyn Highlight) -> Option<Vec<Node>> {
    if element.namespace != Namespace::Html || element.name != "pre" {
        return None;
    }
    let Some(Node::Element(head)) = element.children.first() else {
        return None;
    };
    if head.name != "code" {
        return None;
    }
    // No `defaultLanguage`, no `fallbackLanguage`, not `lazy`: a block
    // without a loaded language stays as it is.
    let lang = head
        .attribute("class")?
        .split(react::is_html_space)
        .find(|class| class.starts_with("language-"))
        .map(|class| &class["language-".len()..])
        .filter(|lang| !lang.is_empty())?;
    if !highlight.has_language(lang) {
        return None;
    }
    // hast-util-to-string, then `stripEndNewline`.
    let mut code = String::new();
    text(&head.children, &mut code);
    if code.ends_with('\n') {
        code.pop();
    }
    let nodes = highlight.code_to_hast(&code, lang)?;
    Some(nodes.into_iter().map(to_react).collect())
}

/// hast-util-to-jsx-runtime and React for the highlighter's nodes.
fn to_react(node: Node) -> Node {
    match node {
        Node::Text(value) => Node::Text(value),
        Node::Element(element) => Node::Element(Element {
            attributes: react::attributes(
                &element.name,
                element.namespace,
                react::properties(element.namespace, element.attributes),
            ),
            children: element.children.into_iter().map(to_react).collect(),
            namespace: element.namespace,
            name: element.name,
        }),
    }
}
