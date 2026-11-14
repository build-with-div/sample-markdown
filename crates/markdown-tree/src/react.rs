//! From rehype-raw's parse to the props React renders, then React's
//! static markup.
//!
//! Each attribute goes through the same steps as in the reference:
//! hast-util-from-parse5 and hastscript turn it into a typed property
//! (property-information 7), react-markdown filters URLs,
//! hast-util-to-jsx-runtime turns `align` and `style` into a style object,
//! and react-dom 19's `renderToStaticMarkup` decides what it writes.

use markup5ever_rcdom::{Handle, NodeData};

use crate::dom::{self, attribute_name};
use crate::uri::url_transform;
use crate::{Element, Namespace, Node};

/// A hast property value, as hastscript parses an attribute.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    Str(String),
    True,
    Num(f64),
    Spaces(Vec<String>),
    Commas(Vec<Value>),
}

const SPACE_SEPARATED: &[&str] = &[
    "accept-charset",
    "accesskey",
    "autocomplete",
    "blocking",
    "class",
    "controlslist",
    "headers",
    "for",
    "http-equiv",
    "itemprop",
    "itemref",
    "itemtype",
    "ping",
    "rel",
    "sandbox",
    "archive",
    "part",
    "aria-controls",
    "aria-describedby",
    "aria-dropeffect",
    "aria-flowto",
    "aria-labelledby",
    "aria-owns",
    "aria-roledescription",
];
const COMMA_SEPARATED: &[&str] = &["accept", "coords", "exportparts"];
const NUMBER: &[&str] = &[
    "cols",
    "colspan",
    "coords",
    "height",
    "high",
    "low",
    "maxlength",
    "minlength",
    "optimum",
    "rows",
    "rowspan",
    "size",
    "span",
    "start",
    "tabindex",
    "width",
    "border",
    "bottommargin",
    "hspace",
    "leftmargin",
    "marginheight",
    "marginwidth",
    "rightmargin",
    "topmargin",
    "vspace",
    "results",
    "aria-colcount",
    "aria-colindex",
    "aria-colspan",
    "aria-level",
    "aria-posinset",
    "aria-rowcount",
    "aria-rowindex",
    "aria-rowspan",
    "aria-setsize",
    "aria-valuemax",
    "aria-valuemin",
    "aria-valuenow",
];
const BOOLEAN: &[&str] = &[
    "allowfullscreen",
    "allowpaymentrequest",
    "allowusermedia",
    "alpha",
    "async",
    "autofocus",
    "autoplay",
    "checked",
    "controls",
    "default",
    "defer",
    "disabled",
    "formnovalidate",
    "inert",
    "ismap",
    "itemscope",
    "loop",
    "multiple",
    "muted",
    "nomodule",
    "novalidate",
    "open",
    "playsinline",
    "readonly",
    "required",
    "reversed",
    "scoped",
    "seamless",
    "selected",
    "shadowrootclonable",
    "shadowrootcustomelementregistry",
    "shadowrootdelegatesfocus",
    "shadowrootserializable",
    "typemustmatch",
    "compact",
    "declare",
    "noresize",
    "nohref",
    "noshade",
    "nowrap",
    "credentialless",
    "disablepictureinpicture",
    "disableremoteplayback",
    "download",
    "hidden",
];
/// Props `pushAttribute` writes as `name=""` when truthy.
const REACT_BOOLEAN: &[&str] = &[
    "inert",
    "allowfullscreen",
    "async",
    "autoplay",
    "controls",
    "credentialless",
    "default",
    "defer",
    "disabled",
    "disablepictureinpicture",
    "disableremoteplayback",
    "formnovalidate",
    "hidden",
    "loop",
    "nomodule",
    "novalidate",
    "open",
    "playsinline",
    "readonly",
    "required",
    "reversed",
    "scoped",
    "seamless",
    "itemscope",
    "autofocus",
    "multiple",
    "muted",
];
const TABLE: &[&str] = &["table", "tbody", "thead", "tfoot", "tr"];

/// hastscript's `parsePrimitive` and list parsing for an HTML attribute.
fn property(name: &str, value: &str) -> Value {
    if SPACE_SEPARATED.contains(&name) {
        return Value::Spaces(
            value
                .split(is_html_space)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        );
    }
    if COMMA_SEPARATED.contains(&name) {
        return Value::Commas(
            parse_commas(value)
                .into_iter()
                .map(|item| primitive(name, &item))
                .collect(),
        );
    }
    primitive(name, value)
}

fn primitive(name: &str, value: &str) -> Value {
    if NUMBER.contains(&name) && !value.is_empty() {
        if let Some(number) = js_number(value) {
            return Value::Num(number);
        }
    }
    if BOOLEAN.contains(&name) && (value.is_empty() || value.eq_ignore_ascii_case(name)) {
        return Value::True;
    }
    Value::Str(value.to_owned())
}

/// comma-separated-tokens' `parse`.
fn parse_commas(value: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let parts: Vec<&str> = value.split(',').collect();
    let last = parts.len() - 1;
    for (index, part) in parts.into_iter().enumerate() {
        let token = part.trim_matches(is_js_space);
        if !token.is_empty() || index != last {
            tokens.push(token.to_owned());
        }
    }
    tokens
}

fn is_html_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// JavaScript's `Number(string)`, or `None` for `NaN`.
fn js_number(value: &str) -> Option<f64> {
    let s = value.trim_matches(is_js_space);
    if s.is_empty() {
        return Some(0.0);
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = s.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix).ok().map(|n| n as f64);
        }
    }
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    if unsigned == "Infinity" {
        return Some(if s.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        });
    }
    let valid = unsigned
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        && unsigned
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_digit() || b == b'.');
    if !valid {
        return None;
    }
    unsigned
        .parse::<f64>()
        .ok()
        .map(|n| if s.starts_with('-') { -n } else { n })
}

/// JavaScript's `String(number)` for the values attributes produce.
fn js_string(number: f64) -> String {
    if number.is_nan() {
        "NaN".into()
    } else if number.is_infinite() {
        if number > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .into()
    } else if number == number.trunc() && number.abs() < 1e21 {
        format!("{number:.0}")
    } else {
        format!("{number}")
    }
}

impl Value {
    /// The value React writes for a prop it writes as a string.
    pub(crate) fn string(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::True => "true".into(),
            Value::Num(n) => js_string(*n),
            Value::Spaces(items) => items.join(" ").trim().to_owned(),
            Value::Commas(items) => {
                let mut items: Vec<String> = items.iter().map(Value::string).collect();
                if items.last().is_some_and(String::is_empty) {
                    items.push(String::new());
                }
                items.join(", ").trim().to_owned()
            }
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Value::Str(s) => !s.is_empty(),
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            _ => true,
        }
    }

    /// JavaScript's `isNaN(value)`, which coerces.
    fn is_nan(&self) -> bool {
        match self {
            Value::Str(s) => js_number(s).is_none(),
            Value::Num(n) => n.is_nan(),
            Value::True => false,
            Value::Spaces(_) | Value::Commas(_) => js_number(&self.string()).is_none(),
        }
    }

    fn number(&self) -> f64 {
        match self {
            Value::Num(n) => *n,
            Value::True => 1.0,
            other => js_number(&other.string()).unwrap_or(f64::NAN),
        }
    }
}

/// react-markdown's URL attributes (html-url-attributes): the attribute and
/// the elements it is a URL on (`None`: all elements).
fn is_url_attribute(element: &str, name: &str) -> bool {
    let elements: Option<&[&str]> = match name {
        "action" => Some(&["form"]),
        "cite" => Some(&["blockquote", "del", "ins", "q"]),
        "data" => Some(&["object"]),
        "formaction" => Some(&["button", "input"]),
        "href" => Some(&["a", "area", "base", "link"]),
        "icon" => Some(&["menuitem"]),
        "itemid" => None,
        "manifest" => Some(&["html"]),
        "ping" => Some(&["a", "area"]),
        "poster" => Some(&["video"]),
        "src" => Some(&[
            "audio", "embed", "iframe", "img", "input", "script", "source", "track", "video",
        ]),
        _ => return false,
    };
    elements.is_none_or(|list| list.contains(&element))
}

/// inline-style-parser through hast-util-to-jsx-runtime's `parseStyle`, with
/// `ignoreInvalidStyle`: declarations in order, or nothing if invalid.
fn parse_style(value: &str) -> Vec<(String, String)> {
    let mut source = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("/*") {
        source.push_str(&rest[..start]);
        rest = rest[start + 2..]
            .find("*/")
            .map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    source.push_str(rest);
    let mut declarations: Vec<(String, String)> = Vec::new();
    for declaration in split_declarations(&source) {
        let declaration = declaration.trim();
        if declaration.is_empty() {
            continue;
        }
        let Some((name, value)) = declaration.split_once(':') else {
            return Vec::new();
        };
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let name = if name.starts_with("--") {
            name.to_owned()
        } else {
            name.to_lowercase()
        };
        match declarations.iter_mut().find(|(known, _)| *known == name) {
            Some(existing) => existing.1 = value.to_owned(),
            None => declarations.push((name, value.to_owned())),
        }
    }
    declarations
}

/// Splits on `;` outside parentheses and quotes.
fn split_declarations(source: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut start) = (0usize, None, 0);
    for (index, c) in source.char_indices() {
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            ('(', None) => depth += 1,
            (')', None) => depth = depth.saturating_sub(1),
            (';', None) if depth == 0 => {
                parts.push(&source[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&source[start..]);
    parts
}

/// React's `isAttributeNameSafe`.
fn attribute_name_safe(name: &str) -> bool {
    fn start(c: char) -> bool {
        matches!(c, ':' | 'A'..='Z' | '_' | 'a'..='z' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
            | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}'
            | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}')
    }
    let mut chars = name.chars();
    chars.next().is_some_and(start)
        && chars.all(|c| start(c) || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}'))
}

/// React's `pushAttribute` for one prop: the value it writes, if any.
fn react_attribute(element: &str, name: &str, value: &Value) -> Option<String> {
    let string_attribute = || (*value != Value::True).then(|| value.string());
    match name {
        "class" | "tabindex" | "dir" | "role" | "width" | "height" => string_attribute(),
        "src" | "href" if *value == Value::Str(String::new()) => {
            (element == "a" && name == "href").then(String::new)
        }
        "src" | "href" | "action" | "formaction" => string_attribute(),
        "contenteditable" | "spellcheck" | "draggable" | "value" => Some(value.string()),
        "capture" | "download" => Some(if *value == Value::True {
            String::new()
        } else {
            value.string()
        }),
        "cols" | "rows" | "size" | "span" => {
            (!value.is_nan() && value.number() >= 1.0).then(|| value.string())
        }
        "rowspan" | "start" => (!value.is_nan()).then(|| value.string()),
        _ if REACT_BOOLEAN.contains(&name) => value.truthy().then(String::new),
        _ => {
            let on = name.len() > 2 && name[..2].eq_ignore_ascii_case("on");
            if on || !attribute_name_safe(name) {
                return None;
            }
            if *value == Value::True {
                let prefix = name.get(..5).map(str::to_ascii_lowercase);
                return matches!(prefix.as_deref(), Some("data-" | "aria-")).then(|| "true".into());
            }
            Some(value.string())
        }
    }
}

/// hast-util-from-parse5 and hastscript: an element's attributes as typed
/// properties. Foreign elements keep strings.
fn properties(namespace: Namespace, parsed: Vec<(String, String)>) -> Vec<(String, Value)> {
    parsed
        .into_iter()
        .map(|(name, raw)| {
            let value = if namespace == Namespace::Html {
                property(&name, &raw)
            } else {
                Value::Str(raw)
            };
            (name, value)
        })
        .collect()
}

/// The attributes React writes for an element's properties, after
/// react-markdown's URL filter.
fn attributes(
    element: &str,
    namespace: Namespace,
    properties: Vec<(String, Value)>,
) -> Vec<(String, String)> {
    if namespace != Namespace::Html {
        return properties
            .into_iter()
            .filter(|(name, _)| !(name.len() > 2 && name[..2].eq_ignore_ascii_case("on")))
            .map(|(name, value)| (name, value.string()))
            .collect();
    }
    // react-markdown.
    let mut props: Vec<(String, Value)> = Vec::new();
    for (name, mut value) in properties {
        if is_url_attribute(element, &name) {
            let url = match &value {
                Value::Num(n) if *n == 0.0 || n.is_nan() => String::new(),
                Value::Spaces(items) => items.join(","),
                other => other.string(),
            };
            value = Value::Str(url_transform(&url));
        }
        props.push((name, value));
    }
    // hast-util-to-jsx-runtime: `align` on cells becomes `text-align`.
    let mut style: Option<Vec<(String, String)>> = None;
    let mut style_index = None;
    let mut align = None;
    let mut kept = Vec::new();
    for (name, value) in props {
        if name == "style" {
            style_index = Some(kept.len());
            style = Some(parse_style(&value.string()));
            kept.push((name, Value::True));
        } else if name == "align"
            && matches!(element, "td" | "th")
            && matches!(value, Value::Str(_))
        {
            align = Some(value.string());
        } else {
            kept.push((name, value));
        }
    }
    if let Some(align) = align {
        let declarations = style.get_or_insert_with(Vec::new);
        match declarations
            .iter_mut()
            .find(|(name, _)| name == "text-align")
        {
            Some(existing) => existing.1 = align,
            None => declarations.push(("text-align".into(), align)),
        }
        if style_index.is_none() {
            style_index = Some(kept.len());
            kept.push(("style".into(), Value::True));
        }
    }
    // React's `pushStartInstance` and `pushAttribute`.
    let mut out = Vec::new();
    let mut deferred: Vec<(String, Value)> = Vec::new();
    let deferred_names: &[&str] = match element {
        "input" => &[
            "name",
            "formaction",
            "formenctype",
            "formmethod",
            "formtarget",
            "checked",
            "value",
        ],
        "option" => &["selected"],
        _ => &[],
    };
    for (index, (name, value)) in kept.into_iter().enumerate() {
        if Some(index) == style_index {
            let declarations = style.take().unwrap_or_default();
            let text: Vec<String> = declarations
                .into_iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(n, v)| format!("{n}:{}", v.trim()))
                .collect();
            if !text.is_empty() {
                out.push(("style".to_owned(), text.join(";")));
            }
        } else if deferred_names.contains(&name.as_str()) {
            deferred.push((name, value));
        } else if matches!(element, "select" | "textarea") && name == "value" {
            // Taken as the control's value, not written.
        } else if let Some(written) = react_attribute(element, &name, &value) {
            out.push((name, written));
        }
    }
    for wanted in deferred_names {
        if let Some((name, value)) = deferred.iter().find(|(name, _)| name == wanted) {
            let written = match *wanted {
                "checked" | "selected" => value.truthy().then(String::new),
                _ => react_attribute(element, name, value),
            };
            if let Some(written) = written {
                out.push((name.clone(), written));
            }
        }
    }
    out
}

/// rehype-raw's tree to React's: typed props, rehype-sanitize when asked,
/// and the table whitespace hast-util-to-jsx-runtime drops.
pub(crate) fn nodes(handles: &[Handle], sanitize: bool) -> Vec<Node> {
    let mut out = Vec::new();
    convert(handles, sanitize, &mut Vec::new(), &mut out);
    out
}

fn convert(handles: &[Handle], sanitize: bool, stack: &mut Vec<String>, out: &mut Vec<Node>) {
    for handle in handles {
        match &handle.data {
            NodeData::Text { contents } => dom::push_text(out, &contents.borrow()),
            NodeData::Element { name, attrs, .. } => {
                let namespace = dom::namespace(&name.ns);
                let element = name.local.to_string();
                stack.push(element.clone());
                let mut children = Vec::new();
                convert(&dom::children(handle), sanitize, stack, &mut children);
                stack.pop();
                let parsed = attrs
                    .borrow()
                    .iter()
                    .map(|a| (attribute_name(&a.name), a.value.to_string()))
                    .collect();
                let mut properties = properties(namespace, parsed);
                if sanitize {
                    if !crate::sanitize::allowed(&element, stack) {
                        if !crate::sanitize::removed(&element) {
                            for child in children {
                                match child {
                                    Node::Text(text) => dom::push_text(out, &text),
                                    element => out.push(element),
                                }
                            }
                        }
                        continue;
                    }
                    properties = crate::sanitize::properties(&element, properties);
                }
                if namespace == Namespace::Html && TABLE.contains(&element.as_str()) {
                    children.retain(
                        |child| !matches!(child, Node::Text(t) if t.chars().all(is_html_space)),
                    );
                }
                out.push(Node::Element(Element {
                    attributes: attributes(&element, namespace, properties),
                    namespace,
                    name: element,
                    children,
                }));
            }
            _ => {}
        }
    }
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "keygen", "link", "meta", "param",
    "source", "track", "wbr",
];

/// `renderToStaticMarkup`'s output for the tree.
pub(crate) fn write_markup(nodes: &[Node], raw_text: bool, out: &mut String) {
    for node in nodes {
        match node {
            Node::Text(value) if raw_text => out.push_str(value),
            Node::Text(value) => escape(value, out),
            Node::Element(e) => {
                out.push('<');
                out.push_str(&e.name);
                for (name, value) in &e.attributes {
                    out.push(' ');
                    out.push_str(name);
                    out.push_str("=\"");
                    escape(value, out);
                    out.push('"');
                }
                let html = e.namespace == Namespace::Html;
                if html && VOID.contains(&e.name.as_str()) {
                    out.push_str("/>");
                    continue;
                }
                out.push('>');
                // React writes a leading newline before a single text child
                // that starts with one, which the parser then drops.
                if html && matches!(e.name.as_str(), "pre" | "textarea" | "listing") {
                    if let [Node::Text(value)] = e.children.as_slice() {
                        if value.starts_with('\n') {
                            out.push('\n');
                        }
                    }
                }
                let raw = html && matches!(e.name.as_str(), "script" | "style");
                write_markup(&e.children, raw, out);
                out.push_str("</");
                out.push_str(&e.name);
                out.push('>');
            }
        }
    }
}

/// React's `escapeTextForBrowser`.
fn escape(value: &str, out: &mut String) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(element: &str, list: &[(&str, &str)]) -> Vec<(String, String)> {
        let parsed = list
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect();
        attributes(
            element,
            Namespace::Html,
            properties(Namespace::Html, parsed),
        )
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn input_writes_checked_and_value_last() {
        let got = attrs(
            "input",
            &[
                ("checked", ""),
                ("value", "v"),
                ("type", "text"),
                ("disabled", ""),
            ],
        );
        assert_eq!(
            got,
            pairs(&[
                ("type", "text"),
                ("disabled", ""),
                ("checked", ""),
                ("value", "v")
            ])
        );
    }

    #[test]
    fn cell_align_joins_the_style() {
        let got = attrs("td", &[("align", "center"), ("style", "color: red; ")]);
        assert_eq!(got, pairs(&[("style", "color:red;text-align:center")]));
    }

    #[test]
    fn react_drops_unknown_booleans_and_small_sizes() {
        assert_eq!(
            attrs("hr", &[("noshade", ""), ("size", "0"), ("width", "50%")]),
            pairs(&[("width", "50%")])
        );
    }

    #[test]
    fn unsafe_urls_become_empty() {
        assert_eq!(
            attrs("a", &[("href", "javascript:x")]),
            pairs(&[("href", "")])
        );
        assert_eq!(
            attrs("img", &[("src", "javascript:x"), ("alt", "b")]),
            pairs(&[("alt", "b")])
        );
    }
}
