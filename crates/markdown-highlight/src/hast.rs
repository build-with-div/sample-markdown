//! Shiki's `codeToHast` (`@shikijs/core` 4.6.0) with one theme and its
//! default options, and the [`markdown_tree::Highlight`] that
//! `@shikijs/rehype` calls.
//!
//! The defaults: the classic structure (`pre.shiki` > `code` > one
//! `span.line` per line, separated by `"\n"`), `tabindex="0"`, whitespace
//! tokens merged into the next token, no transformers. The `ansi`
//! language (`tokenizeAnsiWithTheme`) is not ported.

use markdown_tree::{Element, Namespace, Node};

use crate::shiki::{is_plain_lang, Error, Highlighter, ThemedToken};
use crate::textmate::theme::font_style;
use crate::textmate::utils::is_js_whitespace;

/// `RE_WHITESPACE_ONLY`, `/^\s+$/`.
fn whitespace_only(text: &str) -> bool {
    !text.is_empty() && text.chars().all(is_js_whitespace)
}

fn decorated(token: &ThemedToken) -> bool {
    token.font_style.is_some_and(|style| {
        style as i32 & (font_style::UNDERLINE | font_style::STRIKETHROUGH) != 0
    })
}

/// `mergeWhitespaceTokens`: a whitespace-only token joins the next token,
/// unless that one is underlined or struck through.
fn merge_whitespace_tokens(lines: Vec<Vec<ThemedToken>>) -> Vec<Vec<ThemedToken>> {
    lines
        .into_iter()
        .map(|line| {
            let mut merged = Vec::new();
            let mut carry = String::new();
            let mut first_offset = None;
            let count = line.len();
            for (index, token) in line.into_iter().enumerate() {
                let could_merge = !decorated(&token);
                if could_merge && whitespace_only(&token.content) && index + 1 < count {
                    first_offset.get_or_insert(token.offset);
                    carry.push_str(&token.content);
                } else if !carry.is_empty() {
                    let offset = first_offset.take().unwrap_or(token.offset);
                    let carried = std::mem::take(&mut carry);
                    if could_merge {
                        merged.push(ThemedToken {
                            content: carried + &token.content,
                            offset,
                            ..token
                        });
                    } else {
                        merged.push(ThemedToken {
                            content: carried,
                            offset,
                            color: None,
                            font_style: None,
                        });
                        merged.push(token);
                    }
                } else {
                    merged.push(token);
                }
            }
            merged
        })
        .collect()
}

/// `getTokenStyleObject` and `stringifyTokenStyle`.
fn token_style(token: &ThemedToken) -> String {
    let mut styles = Vec::new();
    if let Some(color) = token.color.as_deref().filter(|c| !c.is_empty()) {
        styles.push(format!("color:{color}"));
    }
    if let Some(style) = token.font_style.map(|s| s as i32).filter(|s| *s != 0) {
        if style & font_style::ITALIC != 0 {
            styles.push("font-style:italic".to_owned());
        }
        if style & font_style::BOLD != 0 {
            styles.push("font-weight:bold".to_owned());
        }
        let mut decorations = Vec::new();
        if style & font_style::UNDERLINE != 0 {
            decorations.push("underline");
        }
        if style & font_style::STRIKETHROUGH != 0 {
            decorations.push("line-through");
        }
        if !decorations.is_empty() {
            styles.push(format!("text-decoration:{}", decorations.join(" ")));
        }
    }
    styles.join(";")
}

fn element(name: &str, attributes: Vec<(&str, String)>, children: Vec<Node>) -> Node {
    Node::Element(Element {
        namespace: Namespace::Html,
        name: name.to_owned(),
        attributes: attributes
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect(),
        children,
    })
}

impl Highlighter {
    /// `codeToHast(code, { lang, theme })`: the root's children.
    pub fn code_to_hast(&self, code: &str, lang: &str, theme: &str) -> Result<Vec<Node>, Error> {
        if lang == "ansi" && theme != "none" {
            return Err(Error::NotPorted("the ansi language"));
        }
        let tokens = merge_whitespace_tokens(self.code_to_tokens_base(code, lang, theme)?);
        let theme = self
            .theme(theme)
            .ok_or_else(|| Error::UnknownTheme(theme.to_owned()))?;
        let mut lines = Vec::new();
        for (index, line) in tokens.into_iter().enumerate() {
            if index > 0 {
                lines.push(Node::Text("\n".to_owned()));
            }
            let spans = line
                .into_iter()
                .map(|token| {
                    let style = token_style(&token);
                    let attributes = if style.is_empty() {
                        vec![]
                    } else {
                        vec![("style", style)]
                    };
                    element("span", attributes, vec![Node::Text(token.content)])
                })
                .collect();
            lines.push(element("span", vec![("class", "line".to_owned())], spans));
        }
        let code = element("code", vec![], lines);
        Ok(vec![element(
            "pre",
            vec![
                ("class", format!("shiki {}", theme.name)),
                (
                    "style",
                    format!(
                        "background-color:{};color:{}",
                        theme.replace_color(&theme.bg),
                        theme.replace_color(&theme.fg)
                    ),
                ),
                ("tabindex", "0".to_owned()),
            ],
            vec![code],
        )])
    }

    /// `getLoadedLanguages().includes(lang) || isSpecialLang(lang)`.
    pub fn has_language(&self, lang: &str) -> bool {
        is_plain_lang(lang) || lang == "ansi" || self.is_loaded(lang)
    }
}

/// `@shikijs/rehype`'s highlighter, with the theme a page uses.
pub struct RehypeShiki<'a> {
    pub highlighter: &'a Highlighter,
    pub theme: &'a str,
}

impl markdown_tree::Highlight for RehypeShiki<'_> {
    fn has_language(&self, lang: &str) -> bool {
        self.highlighter.has_language(lang)
    }

    fn code_to_hast(&self, code: &str, lang: &str) -> Option<Vec<Node>> {
        self.highlighter.code_to_hast(code, lang, self.theme).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(content: &str, color: Option<&str>, style: u32) -> ThemedToken {
        ThemedToken {
            content: content.into(),
            offset: 0,
            color: color.map(str::to_owned),
            font_style: Some(style),
        }
    }

    #[test]
    fn whitespace_joins_the_next_token_unless_decorated() {
        let merged = merge_whitespace_tokens(vec![vec![
            token("  ", Some("#1"), 0),
            token("a", Some("#2"), 0),
            token(" ", Some("#1"), 0),
            token("b", Some("#3"), font_style::UNDERLINE as u32),
            token(" ", Some("#1"), 0),
        ]]);
        let contents: Vec<(&str, Option<&str>)> = merged[0]
            .iter()
            .map(|t| (t.content.as_str(), t.color.as_deref()))
            .collect();
        assert_eq!(
            contents,
            [
                ("  a", Some("#2")),
                (" ", None),
                ("b", Some("#3")),
                (" ", Some("#1"))
            ]
        );
        assert_eq!(
            token_style(&merged[0][2]),
            "color:#3;text-decoration:underline"
        );
    }
}
