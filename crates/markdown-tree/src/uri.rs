//! URL handling from the reference pipeline.

/// `normalizeUri` from micromark-util-sanitize-uri 2, which
/// mdast-util-to-hast applies to link and image URLs: percent-encode what
/// is not allowed in a URL, keeping valid `%XX` escapes.
pub(crate) fn normalize_uri(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        let c = value[index..]
            .chars()
            .next()
            .expect("index is on a char boundary");
        if c == '%'
            && bytes.get(index + 1).is_some_and(u8::is_ascii_alphanumeric)
            && bytes.get(index + 2).is_some_and(u8::is_ascii_alphanumeric)
        {
            out.push_str(&value[index..index + 3]);
            index += 3;
            continue;
        }
        if c.is_ascii() && allowed(c) {
            out.push(c);
        } else {
            let mut buffer = [0; 4];
            for byte in c.encode_utf8(&mut buffer).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
        index += c.len_utf8();
    }
    out
}

/// The ASCII characters `normalizeUri` keeps: `[!#$&-;=?-Z_a-z~]`.
fn allowed(c: char) -> bool {
    matches!(c, '!' | '#' | '$' | '&'..=';' | '=' | '?'..='Z' | '_' | 'a'..='z' | '~')
}

/// `defaultUrlTransform` from react-markdown 10: keep relative URLs and the
/// safe protocols, replace anything else with the empty string.
pub(crate) fn url_transform(value: &str) -> String {
    let position = |c| value.find(c);
    let Some(colon) = position(':') else {
        return value.to_owned();
    };
    let before = |other: Option<usize>| other.is_some_and(|i| colon > i);
    if before(position('/')) || before(position('?')) || before(position('#')) {
        return value.to_owned();
    }
    let protocol = value[..colon].to_ascii_lowercase();
    if matches!(
        protocol.as_str(),
        "http" | "https" | "irc" | "ircs" | "mailto" | "xmpp"
    ) {
        value.to_owned()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_like_micromark() {
        assert_eq!(normalize_uri("/x y"), "/x%20y");
        assert_eq!(normalize_uri("a%20b%zz%"), "a%20b%zz%25");
        assert_eq!(normalize_uri("ä[]"), "%C3%A4%5B%5D");
        assert_eq!(
            normalize_uri("https://a.b/?q=1&r=2#f"),
            "https://a.b/?q=1&r=2#f"
        );
    }

    #[test]
    fn transforms_like_react_markdown() {
        assert_eq!(url_transform("javascript:alert(1)"), "");
        assert_eq!(url_transform("HTTPS://x"), "HTTPS://x");
        assert_eq!(url_transform("/a:b"), "/a:b");
        assert_eq!(url_transform("#x:y"), "#x:y");
        assert_eq!(url_transform("relative"), "relative");
    }
}
