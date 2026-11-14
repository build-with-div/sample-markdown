//! HTML parsing with html5ever, in the two places the reference pipeline
//! parses: rehype-raw's parse5 fragment parser (no context element, which
//! parse5 treats as `<template>`), and Chromium parsing React's markup
//! inside the page's `<article>`.

use html5ever::tendril::TendrilSink;
use html5ever::{local_name, ns, parse_fragment, Namespace as Ns, ParseOpts, QualName};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use crate::{Element, Namespace, Node};

#[derive(Clone, Copy)]
pub(crate) enum Context {
    /// parse5's fragment parser without a context element.
    Template,
    /// Inside `<article>` in a document's body.
    Article,
}

/// Parses `html` as a fragment and calls `f` with the fragment's nodes.
/// The nodes live only as long as the parsed DOM: `RcDom`'s drop empties
/// every node's children.
pub(crate) fn parse<T>(html: &str, context: Context, f: impl FnOnce(&[Handle]) -> T) -> T {
    let name = match context {
        Context::Template => local_name!("template"),
        Context::Article => local_name!("article"),
    };
    let context = QualName::new(None, ns!(html), name);
    let dom = parse_fragment(
        RcDom::default(),
        ParseOpts::default(),
        context,
        vec![],
        false,
    )
    .one(html);
    // The fragment's nodes are the children of the root `html` element.
    let root = dom.document.children.borrow().first().cloned();
    let handles = root
        .map(|root| root.children.borrow().clone())
        .unwrap_or_default();
    f(&handles)
}

pub(crate) fn namespace(ns: &Ns) -> Namespace {
    if *ns == ns!(svg) {
        Namespace::Svg
    } else if *ns == ns!(mathml) {
        Namespace::MathMl
    } else {
        Namespace::Html
    }
}

/// The children of an element, or of a `<template>`'s contents.
pub(crate) fn children(handle: &Handle) -> Vec<Handle> {
    if let NodeData::Element {
        name,
        template_contents,
        ..
    } = &handle.data
    {
        if name.ns == ns!(html) && name.local == local_name!("template") {
            if let Some(contents) = template_contents.borrow().as_ref() {
                return contents.children.borrow().clone();
            }
        }
    }
    handle.children.borrow().clone()
}

/// The parsed DOM as nodes: elements and text, without comments, with
/// attributes as parsed.
pub(crate) fn nodes(handles: &[Handle]) -> Vec<Node> {
    let mut out = Vec::new();
    for handle in handles {
        match &handle.data {
            NodeData::Text { contents } => push_text(&mut out, &contents.borrow()),
            NodeData::Element { name, attrs, .. } => out.push(Node::Element(Element {
                namespace: namespace(&name.ns),
                name: name.local.to_string(),
                attributes: attrs
                    .borrow()
                    .iter()
                    .map(|a| (attribute_name(&a.name), a.value.to_string()))
                    .collect(),
                children: nodes(&children(handle)),
            })),
            _ => {}
        }
    }
    out
}

pub(crate) fn attribute_name(name: &QualName) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

pub(crate) fn push_text(out: &mut Vec<Node>, value: &str) {
    if let Some(Node::Text(previous)) = out.last_mut() {
        previous.push_str(value);
    } else {
        out.push(Node::Text(value.to_owned()));
    }
}
