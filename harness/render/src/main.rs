//! Renders the Markdown fidelity fixtures with Div's public `divui` API (crate `div`).
//!
//! Input is a page tree that `harness/compare.py` captured from pinned
//! Chromium: every element of the reference page's `<body>` with its tag,
//! attributes, text and computed style. Two modes build it with Div:
//!
//! - `semantic`: tags, attributes and text only, as a Markdown component
//!   could build them today. Div has no user-agent or author stylesheet, so
//!   this measures what the missing cascade costs. With `--source`, the
//!   tree comes from the fixture's Markdown instead of the capture:
//!   `markdown-tree` parses it, `markdown-div` mounts it, and the capture
//!   supplies only the viewport. `report --source` checks that this tree
//!   equals the captured one.
//! - `computed`: the same tree, with each element's Chromium computed style
//!   copied onto it as typed declarations (`parse_literal` + `set_property`).
//!   div-gap: G-CASCADE. This is measurement scaffolding, not an app
//!   technique: it takes the cascade out of the comparison so the result
//!   shows what layout, paint and text get wrong once a cascade exists.
//!
//! Commands:
//!
//! ```text
//! markdown-render render   <tree.json> <out.png> [--mode M | --source F.md] [--viewport WxH] [--scale S]
//! markdown-render geometry <tree.json>           [--mode M | --source F.md] [--viewport WxH] [--scale S]
//! markdown-render report   <tree.json>           [--source F.md]
//! ```

use div::typed_style::parse_literal;
use div::{
    Document, Element, EngineOptions, Error, RasterConfiguration, StyleProperty, ViewportMetrics,
};
use markdown_tree::Node;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Semantic,
    Computed,
}

/// Logical longhands, which `getComputedStyle` lists beside the physical
/// ones, and the capture's used sizes: never copied. The capture records
/// `width` and `height` as computed values (see compare.py), so those are.
const NOT_COPIED: &[&str] = &["inline-size", "block-size"];

/// What building the tree ran into: the gap inventory for one fixture.
#[derive(Default)]
struct Findings {
    /// HTML tags `Element::create` rejects, with the stand-in used.
    unknown_tags: BTreeMap<String, (String, usize)>,
    /// CSS property names Div's style schema does not have.
    unknown_properties: BTreeMap<String, usize>,
    /// Values `parse_literal` rejects, keyed by property and value.
    rejected_values: BTreeMap<(String, String), (String, usize)>,
    /// Values accepted only after rewriting (see `style`).
    normalized_values: BTreeMap<(String, String), usize>,
    /// Values parsed but refused by `set_property`.
    refused_values: BTreeMap<(String, String), (String, usize)>,
    /// Declarations applied.
    applied: usize,
    /// Images that could not be loaded.
    image_errors: Vec<String>,
}

struct Builder<'a> {
    document: &'a Document,
    mode: Mode,
    base: PathBuf,
    findings: Findings,
    /// Elements in depth-first order, `None` for skipped nodes, parallel to
    /// the capture's element order.
    elements: Vec<Element>,
}

impl Builder<'_> {
    fn element(&mut self, node: &Value) -> Result<Element, Error> {
        let tag = node["tag"].as_str().unwrap_or("div");
        let element = match Element::create(self.document, tag) {
            Ok(element) => element,
            Err(Error::UnknownTag(_)) => {
                // div-gap: G-TAGS. Stand in with the closest kind Div has.
                let inline = node["style"]["display"]
                    .as_str()
                    .is_some_and(|display| display.starts_with("inline"));
                let stand_in = if inline { "span" } else { "div" };
                let entry = self
                    .findings
                    .unknown_tags
                    .entry(tag.to_owned())
                    .or_insert((stand_in.to_owned(), 0));
                entry.1 += 1;
                Element::create(self.document, stand_in)?
            }
            Err(error) => return Err(error),
        };
        self.decorate(&element, node)?;
        Ok(element)
    }

    fn decorate(&mut self, element: &Element, node: &Value) -> Result<(), Error> {
        self.elements.push(element.clone());
        if let Some(attributes) = node["attrs"].as_object() {
            for (name, value) in attributes {
                let value = value.as_str().unwrap_or("");
                element.set_attribute(name, value)?;
                if name == "checked" {
                    element.set_checked(true)?;
                }
            }
        }
        if let Some(image) = node.get("image").filter(|image| image.is_object()) {
            self.image(element, image)?;
        }
        if self.mode == Mode::Computed {
            if let Some(style) = node["style"].as_object() {
                self.style(element, style)?;
            }
        }
        for child in node["children"].as_array().into_iter().flatten() {
            if let Some(text) = child.get("text").and_then(Value::as_str) {
                element.append_text_node(text)?;
            } else {
                let child_element = self.element(child)?;
                element.append_child(&child_element)?;
            }
        }
        Ok(())
    }

    fn style(&mut self, element: &Element, style: &Map<String, Value>) -> Result<(), Error> {
        for (name, value) in style {
            if NOT_COPIED.contains(&name.as_str()) {
                continue;
            }
            let value = value.as_str().unwrap_or("");
            // div-gap: G-LITERALS. Div's schema names the list-style-type
            // longhand `list-style`; `from_css_name("list-style-type")` is None.
            let alias = (name == "list-style-type")
                .then(|| StyleProperty::from_css_name("list-style"))
                .flatten();
            if alias.is_some() {
                *self
                    .findings
                    .normalized_values
                    .entry((name.clone(), "(name)".to_owned()))
                    .or_default() += 1;
            }
            let Some(property) = alias.or_else(|| StyleProperty::from_css_name(name)) else {
                *self
                    .findings
                    .unknown_properties
                    .entry(name.clone())
                    .or_default() += 1;
                continue;
            };
            let parsed = parse_literal(property, value).or_else(|error| {
                // div-gap: G-LITERALS. `border-*-width` takes a bare integer
                // but not the `1px` every computed style has. Retry with the
                // number so the measurement shows layout and paint; the
                // report still counts it.
                let number = value
                    .strip_suffix("px")
                    .filter(|_| name.ends_with("-width"));
                match number.map(|number| parse_literal(property, number)) {
                    Some(Ok(parsed)) => {
                        *self
                            .findings
                            .normalized_values
                            .entry((name.clone(), value.to_owned()))
                            .or_default() += 1;
                        Ok(parsed)
                    }
                    _ => Err(error),
                }
            });
            match parsed {
                Ok(parsed) => match element.set_property(property, parsed) {
                    Ok(()) => self.findings.applied += 1,
                    Err(error) => {
                        self.findings
                            .refused_values
                            .entry((name.clone(), value.to_owned()))
                            .or_insert((error.to_string(), 0))
                            .1 += 1;
                    }
                },
                Err(error) => {
                    self.findings
                        .rejected_values
                        .entry((name.clone(), value.to_owned()))
                        .or_insert((error.to_string(), 0))
                        .1 += 1;
                }
            }
        }
        Ok(())
    }

    /// `<img src>` does not load by itself in Div: the bytes are registered
    /// as a resource and attached to the element.
    /// div-gap: G-IMG-SRC.
    fn image(&mut self, element: &Element, image: &Value) -> Result<(), Error> {
        let Some(file) = image["file"].as_str() else {
            self.findings
                .image_errors
                .push(format!("{} is not a local file", image["src"]));
            return Ok(());
        };
        let bytes = match std::fs::read(self.base.join(file)) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.findings.image_errors.push(format!("{file}: {error}"));
                return Ok(());
            }
        };
        let source = image["src"].as_str().unwrap_or(file);
        let id = self.document.register_image_resource(
            source,
            image["mime"].as_str().unwrap_or(""),
            image["sha256"].as_str().unwrap_or(""),
            bytes,
        )?;
        // div-gap: G-IMG-SIZE. Div does not take the intrinsic size from the
        // image bytes; without one the box is 300x150. Use the natural size
        // Chromium decoded.
        let natural = image["natural"].as_array().and_then(|size| {
            Some((
                size.first()?.as_f64()? as f32,
                size.get(1)?.as_f64()? as f32,
            ))
        });
        element.set_image_resource(id, natural)
    }

    fn report(&self) -> Value {
        let grouped = |map: &BTreeMap<(String, String), (String, usize)>| -> Vec<Value> {
            map.iter()
                .map(|((property, value), (error, count))| {
                    json!({ "property": property, "value": value, "error": error, "count": count })
                })
                .collect()
        };
        json!({
            "unknown_tags": self.findings.unknown_tags.iter()
                .map(|(tag, (stand_in, count))| json!({ "tag": tag, "stand_in": stand_in, "count": count }))
                .collect::<Vec<_>>(),
            "unknown_properties": self.findings.unknown_properties,
            "rejected_values": grouped(&self.findings.rejected_values),
            "refused_values": grouped(&self.findings.refused_values),
            "normalized_values": self.findings.normalized_values.iter()
                .map(|((property, value), count)| json!({ "property": property, "value": value, "count": count }))
                .collect::<Vec<_>>(),
            "applied_declarations": self.findings.applied,
            "image_errors": self.findings.image_errors,
        })
    }
}

struct Surface {
    width: f64,
    height: f64,
    scale: f64,
}

fn build<'a>(
    document: &'a Document,
    tree_path: &Path,
    tree: &Value,
    mode: Mode,
) -> Result<Builder<'a>, Box<dyn std::error::Error>> {
    let mut builder = Builder {
        document,
        mode,
        base: tree_path.parent().unwrap_or(Path::new(".")).to_owned(),
        findings: Findings::default(),
        elements: Vec::new(),
    };
    // div-gap: G-ROOT. Div's `Document::body()` is the root element: it
    // takes the place of `<html>`, and there is no `<html>`/`<body>` pair.
    // Given Chromium's body style (margin 8px) it still measured 0,0 800x600,
    // so the captured `<body>` is built as an ordinary block inside it.
    let body = Element::create(document, "div")?;
    builder.decorate(&body, &tree["body"])?;
    document.body().append_child(&body)?;
    Ok(builder)
}

fn document(surface: &Surface) -> Result<Document, Box<dyn std::error::Error>> {
    Ok(Document::with_options(
        ViewportMetrics::from_logical_size(surface.width, surface.height, surface.scale)?,
        EngineOptions {
            raster_configuration: RasterConfiguration::chromium_linux_lcd(),
        },
    )?)
}

fn load(path: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

/// Builds the captured tree, or with `--source` the fixture's Markdown.
/// Returns the elements in the capture's order and the build report.
fn build_any(
    document: &Document,
    tree_path: &Path,
    tree: &Value,
    config: &Config,
) -> Result<(Vec<Element>, Value), Box<dyn std::error::Error>> {
    let Some(source) = &config.source else {
        let builder = build(document, tree_path, tree, config.mode)?;
        let report = builder.report();
        return Ok((builder.elements, report));
    };
    let (elements, mut report, body) = build_from_source(document, source)?;
    let difference = first_difference(&captured_lines(&tree["body"]), &tree_lines(&body));
    report["tree_equals_capture"] = json!(difference.is_none());
    report["tree_difference"] = json!(difference);
    Ok((elements, report))
}

/// The `<body>` of the page `reference/build.mjs` writes around
/// react-markdown's output, as Chromium parses it. The body holds "\n", the
/// article and "\n\n\n" (the line endings after `</article>`, `</body>` and
/// `</html>`); the article holds "\n", the Markdown's nodes and "\n".
fn page_body(markdown: &str) -> Node {
    let mut article = vec![Node::Text("\n".to_owned())];
    article.extend(markdown_tree::parse(markdown));
    article.push(Node::Text("\n".to_owned()));
    let element = |name: &str, attributes: Vec<(String, String)>, children: Vec<Node>| {
        Node::Element(markdown_tree::Element {
            namespace: markdown_tree::Namespace::Html,
            name: name.to_owned(),
            attributes,
            children: merge_text(children),
        })
    };
    element(
        "body",
        Vec::new(),
        vec![
            Node::Text("\n".to_owned()),
            element(
                "article",
                vec![("class".to_owned(), "markdown-body".to_owned())],
                article,
            ),
            Node::Text("\n\n\n".to_owned()),
        ],
    )
}

fn merge_text(nodes: Vec<Node>) -> Vec<Node> {
    let mut merged: Vec<Node> = Vec::new();
    for node in nodes {
        match (merged.last_mut(), node) {
            (Some(Node::Text(last)), Node::Text(text)) => last.push_str(&text),
            (_, node) => merged.push(node),
        }
    }
    merged
}

/// Semantic mode from source: `markdown-tree` parses the fixture and
/// `markdown-div` mounts it. Images load from the fixture's directory, as
/// a page served from there fetches them, with the MIME type a static
/// server gives the file's extension.
fn build_from_source(
    document: &Document,
    source: &Path,
) -> Result<(Vec<Element>, Value, Node), Box<dyn std::error::Error>> {
    let markdown = std::fs::read_to_string(source)?.replace("\r\n", "\n");
    let body = page_body(&markdown);
    let Node::Element(body_node) = &body else {
        unreachable!("page_body returns an element")
    };
    let load = markdown_div::directory_loader(source.parent().unwrap_or(Path::new(".")).to_owned());
    // div-gap: G-ROOT. As in `build`, the page's `<body>` is a `div` inside
    // Div's root element.
    let body_element = Element::create(document, "div")?;
    document.body().append_child(&body_element)?;
    let mounted = markdown_div::mount(document, &body_element, &body_node.children, Some(&load))?;
    let mut elements = vec![body_element];
    elements.extend(mounted.elements);
    let report = json!({
        "stand_ins": mounted.report.stand_ins.iter()
            .map(|(tag, (stand_in, count))| json!({ "tag": tag, "stand_in": stand_in, "count": count }))
            .collect::<Vec<_>>(),
        "images_not_loaded": mounted.report.images_not_loaded,
    });
    Ok((elements, report, body))
}

/// A tree as lines: start tags with their attributes, text, end tags.
fn tree_lines(node: &Node) -> Vec<String> {
    fn walk(node: &Node, lines: &mut Vec<String>) {
        match node {
            Node::Text(text) => lines.push(format!("text {text:?}")),
            Node::Element(element) => {
                lines.push(format!("<{} {:?}>", element.name, element.attributes));
                for child in &element.children {
                    walk(child, lines);
                }
                lines.push(format!("</{}>", element.name));
            }
        }
    }
    let mut lines = Vec::new();
    walk(node, &mut lines);
    lines
}

/// The captured tree as [`tree_lines`] writes a tree.
fn captured_lines(node: &Value) -> Vec<String> {
    fn convert(node: &Value) -> Node {
        if let Some(text) = node.get("text").and_then(Value::as_str) {
            return Node::Text(text.to_owned());
        }
        Node::Element(markdown_tree::Element {
            namespace: markdown_tree::Namespace::Html,
            name: node["tag"].as_str().unwrap_or("").to_owned(),
            attributes: node["attrs"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, value)| (name.clone(), value.as_str().unwrap_or("").to_owned()))
                .collect(),
            children: node["children"]
                .as_array()
                .into_iter()
                .flatten()
                .map(convert)
                .collect(),
        })
    }
    tree_lines(&convert(node))
}

fn first_difference(capture: &[String], source: &[String]) -> Option<Value> {
    let line = (0..capture.len().max(source.len())).find(|&i| capture.get(i) != source.get(i))?;
    Some(json!({ "line": line, "capture": capture.get(line), "source": source.get(line) }))
}

struct Config {
    mode: Mode,
    surface: Surface,
    /// The fixture's Markdown, for semantic mode from source.
    source: Option<PathBuf>,
}

fn options(args: &[String], tree: &Value) -> Result<Config, Box<dyn std::error::Error>> {
    let mut mode = None;
    let mut source = None;
    let viewport = &tree["viewport"];
    let mut surface = Surface {
        width: viewport[0].as_f64().unwrap_or(800.0),
        height: viewport[1].as_f64().unwrap_or(600.0),
        scale: 1.0,
    };
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--mode" => {
                mode = Some(match value.as_str() {
                    "semantic" => Mode::Semantic,
                    "computed" => Mode::Computed,
                    other => return Err(format!("unknown mode {other}").into()),
                })
            }
            "--source" => source = Some(PathBuf::from(value)),
            "--viewport" => {
                let (w, h) = value.split_once('x').ok_or("--viewport is WxH")?;
                surface.width = w.parse()?;
                surface.height = h.parse()?;
            }
            "--scale" => surface.scale = value.parse()?,
            other => return Err(format!("unknown option {other}").into()),
        }
    }
    // The source gives tags, attributes and text, not computed styles.
    let mode = match (mode, &source) {
        (Some(Mode::Computed), Some(_)) => return Err("--source builds semantic mode".into()),
        (_, Some(_)) => Mode::Semantic,
        (mode, None) => mode.unwrap_or(Mode::Computed),
    };
    Ok(Config {
        mode,
        surface,
        source,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: markdown-render render <tree.json> <out.png> [--mode semantic|computed] \
                 [--source F.md] [--viewport WxH] [--scale S] | geometry <tree.json> [options] \
                 | report <tree.json> [--source F.md]";
    match args.first().map(String::as_str) {
        Some("render") => {
            let path = args.get(1).ok_or(usage)?;
            let output = args.get(2).ok_or(usage)?;
            let tree = load(path)?;
            let config = options(&args[3..], &tree)?;
            let document = document(&config.surface)?;
            build_any(&document, Path::new(path), &tree, &config)?;
            document.render_to_png(output)?;
            Ok(())
        }
        Some("geometry") => {
            let path = args.get(1).ok_or(usage)?;
            let tree = load(path)?;
            let config = options(&args[2..], &tree)?;
            let document = document(&config.surface)?;
            let (elements, _) = build_any(&document, Path::new(path), &tree, &config)?;
            document.update_all()?;
            let mut boxes = Vec::new();
            for element in &elements {
                let rect = element.bounding_rect()?;
                boxes.push(json!(rect.map(|r| [r.x, r.y, r.width, r.height])));
            }
            let output = json!({
                "mode": format!("{:?}", config.mode),
                "source": config.source.is_some(),
                "boxes": boxes,
            });
            println!("{output}");
            Ok(())
        }
        Some("report") => {
            // Without a source, the report covers computed mode, which
            // meets every gap semantic mode meets and more.
            let path = args.get(1).ok_or(usage)?;
            let tree = load(path)?;
            let mut config = options(&args[2..], &tree)?;
            if config.source.is_none() {
                config.mode = Mode::Computed;
            }
            let document = document(&config.surface)?;
            let (_, report) = build_any(&document, Path::new(path), &tree, &config)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        _ => Err(usage.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Value {
        json!({
            "viewport": [200, 100],
            "body": {
                "tag": "body",
                "style": { "margin-top": "8px", "display": "block", "width": "184px" },
                "children": [
                    { "tag": "h1", "style": { "display": "block", "font-size": "32px" },
                      "children": [{ "text": "Title" }] },
                    { "tag": "blockquote", "style": { "display": "block", "margin-left": "40px" },
                      "children": [{ "text": "Quote" }] },
                    { "tag": "code", "style": { "display": "inline", "no-such-property": "1" },
                      "children": [{ "text": "x" }] }
                ]
            }
        })
    }

    #[test]
    fn builds_every_captured_element_in_order() {
        let surface = Surface {
            width: 200.0,
            height: 100.0,
            scale: 1.0,
        };
        let document = document(&surface).unwrap();
        let builder = build(&document, Path::new("tree.json"), &tree(), Mode::Computed).unwrap();
        assert_eq!(builder.elements.len(), 4);
        let report = builder.report();
        let tags: Vec<&str> = report["unknown_tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["tag"].as_str().unwrap())
            .collect();
        assert_eq!(tags, ["blockquote", "code"]);
        assert_eq!(report["unknown_properties"]["no-such-property"], 1);
    }

    #[test]
    fn semantic_mode_applies_no_style() {
        let surface = Surface {
            width: 200.0,
            height: 100.0,
            scale: 1.0,
        };
        let document = document(&surface).unwrap();
        let builder = build(&document, Path::new("tree.json"), &tree(), Mode::Semantic).unwrap();
        assert_eq!(builder.report()["applied_declarations"], 0);
    }

    #[test]
    fn source_mode_mounts_every_fixture_element_and_loads_images() {
        let surface = Surface {
            width: 800.0,
            height: 600.0,
            scale: 1.0,
        };
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        for name in ["image.md", "readme.md"] {
            let document = document(&surface).unwrap();
            let (elements, report, body) =
                build_from_source(&document, &fixtures.join(name)).unwrap();
            let starts = tree_lines(&body)
                .iter()
                .filter(|line| line.starts_with('<') && !line.starts_with("</"))
                .count();
            assert_eq!(elements.len(), starts, "{name}");
            assert_eq!(report["images_not_loaded"], json!([]), "{name}");
        }
    }
}
