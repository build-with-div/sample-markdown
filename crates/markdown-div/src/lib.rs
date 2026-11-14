//! Mounts the element tree of [`markdown_tree`] in a Div document, through
//! divui's public API only.
//!
//! [`mount`] creates one Div element per element of the tree, sets its
//! attributes as written, appends its text, and returns the elements in
//! tree order. It adds no style: the look comes from the document's
//! stylesheets, which is where github-markdown-css belongs once Div can
//! take it (MD-3, MD-4 in `docs/design.md`). Until then, a mounted tree
//! shows what Div does with HTML elements and no stylesheet.
//!
//! What Div cannot take yet is labeled `div-gap` and counted in the
//! [`Report`], never hidden.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use div::{Document, Element, Error};
use markdown_tree::{Namespace, Node};
use sha2::{Digest, Sha256};

/// An image's bytes, as an app would have fetched them for `<img src>`.
pub struct Image {
    pub bytes: Vec<u8>,
    /// The MIME type the response declared (`image/png`).
    pub mime_type: String,
    /// The SHA-256 of `bytes`, in hex. Div keys its decoded-image cache on it.
    pub sha256: String,
}

/// Fetches the image an `<img src>` names, if the app can.
pub type ImageLoader<'a> = &'a dyn Fn(&str) -> Option<Image>;

/// A loader for a document read from a directory: relative `src` values
/// name files under `base`, with the MIME type a static file server gives
/// their extension.
///
/// div-gap: G-IMG-SRC `<img src>` resolution and fetching (MD-15). A page
/// resolves `src` against its base URL and fetches it; Div does neither, so
/// the app does this much. URLs with a scheme or an absolute path load
/// nothing here.
pub fn directory_loader(base: PathBuf) -> impl Fn(&str) -> Option<Image> {
    move |src: &str| {
        let path = src.split(['?', '#']).next().unwrap_or("");
        if path.is_empty() || path.starts_with('/') || path.contains(':') {
            return None;
        }
        let path = percent_decode(path)?;
        let mime_type = match Path::new(&path).extension()?.to_str()? {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "webp" => "image/webp",
            _ => return None,
        };
        let bytes = std::fs::read(base.join(&path)).ok()?;
        let sha256 = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Some(Image {
            bytes,
            mime_type: mime_type.to_owned(),
            sha256,
        })
    }
}

/// `%XX` escapes to bytes, as a file URL's path becomes a file name.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What mounting ran into.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Element names `Element::create` rejects, with the stand-in used and
    /// how often. Names outside HTML have their namespace (`svg:path`).
    pub stand_ins: BTreeMap<String, (&'static str, usize)>,
    /// `src` values of images that were not loaded.
    pub images_not_loaded: Vec<String>,
}

/// A mounted tree.
pub struct Mounted {
    /// Every element created, in tree order (depth first, parents first).
    pub elements: Vec<Element>,
    pub report: Report,
}

/// Appends `nodes` to `parent`, an element of `document`. `images` fetches
/// the bytes of `<img src>`.
pub fn mount(
    document: &Document,
    parent: &Element,
    nodes: &[Node],
    images: Option<ImageLoader<'_>>,
) -> Result<Mounted, Error> {
    let mut mounter = Mounter {
        document: document.clone(),
        images,
        elements: Vec::new(),
        report: Report::default(),
    };
    mounter.children(parent, nodes)?;
    Ok(Mounted {
        elements: mounter.elements,
        report: mounter.report,
    })
}

struct Mounter<'a> {
    document: Document,
    images: Option<ImageLoader<'a>>,
    elements: Vec<Element>,
    report: Report,
}

impl Mounter<'_> {
    fn children(&mut self, parent: &Element, nodes: &[Node]) -> Result<(), Error> {
        for node in nodes {
            match node {
                Node::Text(text) => parent.append_text_node(text)?,
                Node::Element(element) => {
                    let child = self.element(element)?;
                    parent.append_child(&child)?;
                }
            }
        }
        Ok(())
    }

    fn element(&mut self, node: &markdown_tree::Element) -> Result<Element, Error> {
        let created = match node.namespace {
            Namespace::Html => Element::create(&self.document, &node.name),
            Namespace::Svg if node.name == "svg" => Element::create(&self.document, "svg"),
            _ => Err(Error::UnknownTag(node.name.clone())),
        };
        let element = match created {
            Ok(element) => element,
            Err(Error::UnknownTag(_)) => {
                // div-gap: G-TAGS HTML element identity (MD-1). Div has no
                // element for this name; stand in with `span` for phrasing
                // content and `div` otherwise, and count it.
                let (key, stand_in) = match node.namespace {
                    Namespace::Html if PHRASING.contains(&node.name.as_str()) => {
                        (node.name.clone(), "span")
                    }
                    Namespace::Html => (node.name.clone(), "div"),
                    Namespace::Svg => (format!("svg:{}", node.name), "span"),
                    Namespace::MathMl => (format!("math:{}", node.name), "span"),
                };
                self.report.stand_ins.entry(key).or_insert((stand_in, 0)).1 += 1;
                Element::create(&self.document, stand_in)?
            }
            Err(error) => return Err(error),
        };
        self.elements.push(element.clone());
        for (name, value) in &node.attributes {
            element.set_attribute(name, value)?;
        }
        if node.namespace == Namespace::Html && node.name == "img" {
            self.image(&element, node)?;
        }
        self.children(&element, &node.children)?;
        Ok(element)
    }

    /// div-gap: G-IMG-SRC `<img src>` fetching (MD-15). Setting `src` loads
    /// nothing in Div, so the bytes come from the app's loader and are
    /// registered with the document.
    fn image(&mut self, element: &Element, node: &markdown_tree::Element) -> Result<(), Error> {
        let src = node.attribute("src").unwrap_or("");
        let Some(image) = self.images.and_then(|load| load(src)) else {
            self.report.images_not_loaded.push(src.to_owned());
            return Ok(());
        };
        let id = self.document.register_image_resource(
            src,
            image.mime_type,
            image.sha256,
            image.bytes,
        )?;
        // div-gap: G-IMG-SIZE intrinsic size from the image data (MD-15).
        // Div does not decode it, and this crate does not decode images in
        // its place: without a size the box is 300×150.
        element.set_image_resource(id, None)
    }
}

/// The HTML elements that are phrasing content (HTML § 3.2.5.2.5), and the
/// obsolete ones that were, for choosing a stand-in.
const PHRASING: &[&str] = &[
    "a", "abbr", "area", "audio", "b", "bdi", "bdo", "big", "br", "button", "canvas", "cite",
    "code", "data", "datalist", "del", "dfn", "em", "embed", "font", "i", "iframe", "img", "input",
    "ins", "kbd", "label", "map", "mark", "meter", "noscript", "object", "output", "picture",
    "progress", "q", "ruby", "s", "samp", "select", "slot", "small", "span", "strike", "strong",
    "sub", "sup", "textarea", "time", "tt", "u", "var", "video", "wbr",
];

#[cfg(test)]
mod tests {
    use super::*;
    use div::{EngineOptions, ViewportMetrics};

    fn document() -> Document {
        Document::with_options(
            ViewportMetrics::from_logical_size(400.0, 300.0, 1.0).unwrap(),
            EngineOptions::default(),
        )
        .unwrap()
    }

    fn text(nodes: &[Node]) -> String {
        nodes
            .iter()
            .map(|node| match node {
                Node::Text(text) => text.clone(),
                Node::Element(element) => text(&element.children),
            })
            .collect()
    }

    #[test]
    fn mounts_every_element_in_tree_order() {
        let document = document();
        let nodes = markdown_tree::parse("# Title\n\n> a `b` ~~c~~\n\n- [x] d\n");
        let mounted = mount(&document, &document.body(), &nodes, None).unwrap();
        // h1, blockquote, p, code, del, ul, li, input
        assert_eq!(mounted.elements.len(), 8);
        assert_eq!(document.body().text_content().unwrap(), text(&nodes));
        let stand_ins: Vec<_> = mounted
            .report
            .stand_ins
            .iter()
            .map(|(name, (stand_in, count))| (name.as_str(), *stand_in, *count))
            .collect();
        assert_eq!(
            stand_ins,
            [
                ("blockquote", "div", 1),
                ("code", "span", 1),
                ("del", "span", 1)
            ]
        );
        let input = &mounted.elements[7];
        assert_eq!(
            input.get_attribute("type").unwrap().as_deref(),
            Some("checkbox")
        );
    }

    /// div-gap: MD-24. Div keeps a `style` attribute but does not apply its
    /// declarations, so Shiki's colored spans show the inherited color.
    /// Fails when Div applies inline styles.
    #[test]
    fn div_gap_md24_style_attribute_is_not_applied() {
        let document = document();
        let nodes = markdown_tree::parse("<span style=\"color:#ff0000\">a</span><span>b</span>\n");
        let mounted = mount(&document, &document.body(), &nodes, None).unwrap();
        let styled = &mounted.elements[1];
        let plain = &mounted.elements[2];
        assert_eq!(
            styled.get_attribute("style").unwrap().as_deref(),
            Some("color:#ff0000")
        );
        let color = |element: &Element| format!("{:?}", element.computed_style().unwrap().color);
        assert_eq!(color(styled), color(plain));
        // Control: the same declaration through the style API does apply.
        let property = div::StyleProperty::from_css_name("color").unwrap();
        let value = div::typed_style::parse_literal(property, "#ff0000").unwrap();
        plain.set_property(property, value).unwrap();
        assert_ne!(color(styled), color(plain));
    }

    #[test]
    fn reports_images_it_could_not_load() {
        let document = document();
        let nodes = markdown_tree::parse("![a](x.png)\n");
        let mounted = mount(&document, &document.body(), &nodes, Some(&|_| None)).unwrap();
        assert_eq!(mounted.report.images_not_loaded, ["x.png"]);
    }
}
