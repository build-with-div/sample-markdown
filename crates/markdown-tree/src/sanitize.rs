//! rehype-sanitize 6 (hast-util-sanitize 5.0.2) with its default schema,
//! which follows GitHub's sanitizer.
//!
//! It runs on rehype-raw's tree, between hastscript's typed properties and
//! react-markdown's URL filter, as in `rehypePlugins: [rehypeRaw,
//! rehypeSanitize]`. The schema's property names are written here as the
//! HTML attribute names they come from (`className` is `class`).

use crate::react::Value;

/// `tagNames`.
const TAG_NAMES: &[&str] = &[
    "a",
    "b",
    "blockquote",
    "br",
    "code",
    "dd",
    "del",
    "details",
    "div",
    "dl",
    "dt",
    "em",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "input",
    "ins",
    "kbd",
    "li",
    "ol",
    "p",
    "picture",
    "pre",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "section",
    "source",
    "span",
    "strike",
    "strong",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "tt",
    "ul",
    "var",
];

/// `attributes['*']`.
const DEFAULTS: &[&str] = &[
    "abbr",
    "accept",
    "accept-charset",
    "accesskey",
    "action",
    "align",
    "alt",
    "axis",
    "border",
    "cellpadding",
    "cellspacing",
    "char",
    "charoff",
    "charset",
    "checked",
    "clear",
    "colspan",
    "color",
    "cols",
    "compact",
    "coords",
    "datetime",
    "dir",
    "enctype",
    "frame",
    "hspace",
    "headers",
    "height",
    "hreflang",
    "for",
    "id",
    "ismap",
    "itemprop",
    "label",
    "lang",
    "maxlength",
    "media",
    "method",
    "multiple",
    "name",
    "nohref",
    "noshade",
    "nowrap",
    "open",
    "prompt",
    "readonly",
    "rev",
    "rowspan",
    "rows",
    "rules",
    "scope",
    "selected",
    "shape",
    "size",
    "span",
    "start",
    "summary",
    "tabindex",
    "title",
    "usemap",
    "valign",
    "value",
    "width",
];

/// A value a definition allows.
#[derive(Clone, Copy)]
enum Allowed {
    Str(&'static str),
    True,
    /// `/^language-./`
    LanguageClass,
}

/// A schema definition: the property, and the values it allows (all values
/// when empty).
type Definition = (&'static str, &'static [Allowed]);

const ARIA: [Definition; 3] = [
    ("aria-describedby", &[]),
    ("aria-label", &[]),
    ("aria-labelledby", &[]),
];

/// `attributes[tagName]`.
fn specific(element: &str) -> Vec<Definition> {
    let mut list: Vec<Definition> = Vec::new();
    match element {
        "a" => {
            list.extend(ARIA);
            list.extend([
                ("data-footnote-backref", &[][..]),
                ("data-footnote-ref", &[]),
                ("class", &[Allowed::Str("data-footnote-backref")]),
                ("href", &[]),
            ]);
        }
        "blockquote" | "del" | "ins" | "q" => list.push(("cite", &[])),
        "code" => list.push(("class", &[Allowed::LanguageClass])),
        "div" => list.extend([("itemscope", &[][..]), ("itemtype", &[])]),
        "dl" | "summary" | "table" => list.extend(ARIA),
        "h2" => list.push(("class", &[Allowed::Str("sr-only")])),
        "img" => {
            list.extend(ARIA);
            list.extend([("longdesc", &[][..]), ("src", &[])]);
        }
        "input" => list.extend([
            ("disabled", &[Allowed::True][..]),
            ("type", &[Allowed::Str("checkbox")]),
        ]),
        "li" => list.push(("class", &[Allowed::Str("task-list-item")])),
        "ol" | "ul" => {
            list.extend(ARIA);
            list.push(("class", &[Allowed::Str("contains-task-list")]));
        }
        "section" => list.extend([
            ("data-footnotes", &[][..]),
            ("class", &[Allowed::Str("footnotes")]),
        ]),
        "source" => list.push(("srcset", &[])),
        _ => {}
    }
    list
}

/// `clobber`, with `clobberPrefix` `user-content-`.
const CLOBBER: &[&str] = &["aria-describedby", "aria-labelledby", "id", "name"];

/// `protocols`.
fn protocols(name: &str) -> &'static [&'static str] {
    match name {
        "cite" | "longdesc" | "src" => &["http", "https"],
        "href" => &["http", "https", "irc", "ircs", "mailto", "xmpp"],
        _ => &[],
    }
}

/// Whether an element is kept: in `tagNames`, and inside a `table` when
/// `ancestors` asks for one. `stack` holds the element's ancestors.
pub(crate) fn allowed(element: &str, stack: &[String]) -> bool {
    if !TAG_NAMES.contains(&element) {
        return false;
    }
    match element {
        "tbody" | "td" | "th" | "thead" | "tfoot" | "tr" => stack.iter().any(|a| a == "table"),
        _ => true,
    }
}

/// Whether an element that is not allowed goes with its content (`strip`),
/// rather than being replaced by it.
pub(crate) fn removed(element: &str) -> bool {
    element == "script"
}

/// The element's properties the schema keeps, then its `required` ones.
pub(crate) fn properties(element: &str, properties: Vec<(String, Value)>) -> Vec<(String, Value)> {
    let specific = specific(element);
    let find = |name: &str, list: &[Definition]| list.iter().find(|(n, _)| *n == name).copied();
    let mut result: Vec<(String, Value)> = Vec::new();
    for (name, value) in properties {
        let defaults = DEFAULTS
            .iter()
            .find(|known| **known == name)
            .map(|known| (*known, &[][..]));
        let safe = find(&name, &specific)
            .and_then(|definition| check(definition, &name, &value))
            .or_else(|| defaults.and_then(|definition| check(definition, &name, &value)));
        if let Some(safe) = safe {
            result.push((name, safe));
        }
    }
    if element == "input" {
        for (name, value) in [
            ("disabled", Value::True),
            ("type", Value::Str("checkbox".into())),
        ] {
            if !result.iter().any(|(n, _)| n == name) {
                result.push((name.to_owned(), value));
            }
        }
    }
    result
}

/// `propertyValue`: a list keeps its allowed items (possibly none).
fn check(definition: Definition, name: &str, value: &Value) -> Option<Value> {
    match value {
        Value::Spaces(items) => Some(Value::Spaces(
            items
                .iter()
                .filter_map(
                    |item| match primitive(definition, name, &Value::Str(item.clone())) {
                        Some(Value::Str(s)) => Some(s),
                        _ => None,
                    },
                )
                .collect(),
        )),
        Value::Commas(items) => Some(Value::Commas(
            items
                .iter()
                .filter_map(|item| primitive(definition, name, item))
                .collect(),
        )),
        other => primitive(definition, name, other),
    }
}

/// `propertyValuePrimitive`.
fn primitive(definition: Definition, name: &str, value: &Value) -> Option<Value> {
    if !safe_protocol(name, &value.string()) {
        return None;
    }
    let (_, allowed) = definition;
    if !allowed.is_empty() {
        let ok = allowed.iter().any(|allowed| match (allowed, value) {
            (Allowed::Str(s), Value::Str(v)) => s == v,
            (Allowed::True, Value::True) => true,
            (Allowed::LanguageClass, v) => {
                let v = v.string();
                v.strip_prefix("language-")
                    .and_then(|rest| rest.chars().next())
                    .is_some_and(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
            }
            _ => false,
        });
        if !ok {
            return None;
        }
    }
    if CLOBBER.contains(&name) {
        return Some(Value::Str(format!("user-content-{}", value.string())));
    }
    Some(value.clone())
}

/// `safeProtocol`: relative URLs, or one of the listed protocols, matched
/// case-sensitively.
fn safe_protocol(name: &str, url: &str) -> bool {
    let protocols = protocols(name);
    if protocols.is_empty() {
        return true;
    }
    let Some(colon) = url.find(':') else {
        return true;
    };
    let before = |c| url.find(c).is_some_and(|i| colon > i);
    if before('/') || before('?') || before('#') {
        return true;
    }
    protocols
        .iter()
        .any(|p| colon == p.len() && url.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Value {
        Value::Str(v.into())
    }

    #[test]
    fn input_gets_its_required_properties_last() {
        let got = properties(
            "input",
            vec![("type".into(), s("text")), ("value".into(), s("v"))],
        );
        assert_eq!(
            got,
            vec![
                ("value".into(), s("v")),
                ("disabled".into(), Value::True),
                ("type".into(), s("checkbox"))
            ]
        );
    }

    #[test]
    fn classes_are_filtered_and_ids_prefixed() {
        let classes = Value::Spaces(vec!["task-list-item".into(), "x".into()]);
        let got = properties("li", vec![("class".into(), classes), ("id".into(), s("a"))]);
        assert_eq!(
            got,
            vec![
                ("class".into(), Value::Spaces(vec!["task-list-item".into()])),
                ("id".into(), s("user-content-a"))
            ]
        );
    }

    #[test]
    fn protocols_are_case_sensitive() {
        assert!(safe_protocol("href", "https://x"));
        assert!(!safe_protocol("href", "HTTPS://x"));
        assert!(safe_protocol("src", "/a:b"));
        assert!(!safe_protocol("src", "data:x"));
    }
}
