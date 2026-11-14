//! `src/utils.ts`, and the JavaScript string behavior the port relies on.

use std::cmp::Ordering;

use serde_json::Value;

/// A capture's span in the line, in bytes.
///
/// vscode-oniguruma reports a group that did not participate as an empty
/// span past the line (`-1` read as unsigned, then clamped to the line's
/// length when the line is not ASCII). The scanner uses `start == end ==`
/// the line's length, which every consumer treats the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureIndex {
    pub start: usize,
    pub end: usize,
}

impl CaptureIndex {
    pub fn length(&self) -> usize {
        self.end - self.start
    }
}

/// One match of `CAPTURING_REGEX_SOURCE`, `/\$(\d+)|\${(\d+):\/(downcase|upcase)}/g`.
struct CaptureReference<'a> {
    start: usize,
    end: usize,
    index: &'a str,
    command: Option<&'a str>,
}

fn capture_references(source: &str) -> Vec<CaptureReference<'_>> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let digits = bytes[i + 1..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
            if digits > 0 {
                found.push(CaptureReference {
                    start: i,
                    end: i + 1 + digits,
                    index: &source[i + 1..i + 1 + digits],
                    command: None,
                });
                i += 1 + digits;
                continue;
            }
            if bytes.get(i + 1) == Some(&b'{') {
                let digits = bytes[i + 2..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                if digits > 0 {
                    let after = i + 2 + digits;
                    for command in ["downcase", "upcase"] {
                        let tail = format!(":/{command}}}");
                        if source[after..].starts_with(&tail) {
                            found.push(CaptureReference {
                                start: i,
                                end: after + tail.len(),
                                index: &source[i + 2..after],
                                command: Some(command),
                            });
                            break;
                        }
                    }
                    if found.last().is_some_and(|r| r.start == i) {
                        i = found.last().unwrap().end;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    found
}

/// `RegexSource.hasCaptures`.
pub fn has_captures(source: Option<&str>) -> bool {
    source.is_some_and(|source| !capture_references(source).is_empty())
}

/// `RegexSource.replaceCaptures`.
pub fn replace_captures(source: &str, capture_source: &str, captures: &[CaptureIndex]) -> String {
    let mut out = String::new();
    let mut last = 0;
    for reference in capture_references(source) {
        out.push_str(&source[last..reference.start]);
        last = reference.end;
        let index: usize = reference.index.parse().unwrap_or(usize::MAX);
        match captures.get(index) {
            // `if (capture)`: every group of the pattern has an entry. An
            // unmatched one is an empty span, so it is replaced by "".
            Some(capture) => {
                let mut result = &capture_source[capture.start..capture.end];
                while result.starts_with('.') {
                    result = &result[1..];
                }
                match reference.command {
                    Some("downcase") => out.push_str(&result.to_lowercase()),
                    Some("upcase") => out.push_str(&result.to_uppercase()),
                    _ => out.push_str(result),
                }
            }
            _ => out.push_str(&source[reference.start..reference.end]),
        }
    }
    out.push_str(&source[last..]);
    out
}

/// `escapeRegExpCharacters`: `/[\-\\\{\}\*\+\?\|\^\$\.\,\[\]\(\)\#\s]/g`.
pub fn escape_regexp_characters(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        if "-\\{}*+?|^$.,[]()#".contains(c) || is_js_whitespace(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// JavaScript's `\s`.
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// JavaScript's `String.prototype.trim`.
pub fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// JavaScript truthiness of a JSON value.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// JavaScript's `<` on strings: UTF-16 code unit order.
pub fn js_strcmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `strArrCmp`.
pub fn str_arr_cmp(a: Option<&[String]>, b: Option<&[String]>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Less,
        (_, None) => Ordering::Greater,
        (Some(a), Some(b)) => {
            if a.len() == b.len() {
                for (x, y) in a.iter().zip(b) {
                    let r = js_strcmp(x, y);
                    if r != Ordering::Equal {
                        return r;
                    }
                }
                Ordering::Equal
            } else {
                a.len().cmp(&b.len())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_captures_like_the_regex() {
        let captures = [
            CaptureIndex { start: 0, end: 5 },
            CaptureIndex { start: 0, end: 3 },
            CaptureIndex { start: 3, end: 5 },
        ];
        assert_eq!(replace_captures("a.$1.$2", "..b.c", &captures), "a.b.c");
        assert_eq!(
            replace_captures("${2:/upcase}x$9", "ab.cd", &captures),
            "CDx$9"
        );
        assert_eq!(
            replace_captures("${2:/bad}", "ab.cd", &captures),
            "${2:/bad}"
        );
        assert!(has_captures(Some("x$1")));
        assert!(!has_captures(Some("x$")));
    }

    #[test]
    fn escapes_like_the_regex() {
        assert_eq!(escape_regexp_characters("a.b c#"), "a\\.b\\ c\\#");
    }
}
