//! Shiki 4.6.0's primitive (`@shikijs/primitive`): its language registry,
//! `normalizeTheme` and `codeToTokensBase`.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use serde_json::{json, Map, Value};

use crate::textmate::grammar::{BalancedBracketSelectors, Grammar};
use crate::textmate::metadata;
use crate::textmate::registry::SyncRegistry;
use crate::textmate::scanner::ScannerError;
use crate::textmate::theme::Theme;
use crate::textmate::utils::truthy;

/// Shiki's default `tokenizeTimeLimit`, in milliseconds.
const TOKENIZE_TIME_LIMIT: u64 = 500;

#[derive(Debug)]
pub enum Error {
    /// Shiki: "Language `x` not found, you may need to load it first".
    UnknownLanguage(String),
    /// Shiki: "Theme `x` not found, you may need to load it first".
    UnknownTheme(String),
    /// A grammar pattern Oniguruma rejects.
    Regex(ScannerError),
    /// A part of Shiki this port does not have.
    NotPorted(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownLanguage(name) => write!(f, "Language `{name}` not found"),
            Self::UnknownTheme(name) => write!(f, "Theme `{name}` not found"),
            Self::Regex(error) => error.fmt(f),
            Self::NotPorted(what) => write!(f, "{what} is not ported"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ScannerError> for Error {
    fn from(error: ScannerError) -> Self {
        Self::Regex(error)
    }
}

/// `ThemedToken` from `codeToTokensBase`. A plain language's tokens have
/// no color and no font style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemedToken {
    pub content: String,
    /// In UTF-16 code units from the start of the code, as in JavaScript.
    pub offset: usize,
    pub color: Option<String>,
    pub font_style: Option<u32>,
}

/// A theme after `normalizeTheme`.
pub struct NormalizedTheme {
    pub name: String,
    /// `"light"` or `"dark"`.
    pub kind: String,
    pub fg: String,
    pub bg: String,
    settings: Vec<Value>,
    color_replacements: HashMap<String, String>,
    textmate: Rc<Theme>,
}

fn string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// `normalizeTheme`. The replacements it makes for `colors` (editor and
/// terminal colors that are not hex) do not affect tokens and are left out.
pub fn normalize_theme(raw: &Value) -> NormalizedTheme {
    let mut theme: Map<String, Value> = raw.as_object().cloned().unwrap_or_default();
    if theme.get("tokenColors").is_some_and(truthy) && !theme.get("settings").is_some_and(truthy) {
        let token_colors = theme.remove("tokenColors").expect("present");
        theme.insert("settings".into(), token_colors);
    }
    let kind = string(theme.get("type")).unwrap_or_else(|| "dark".into());
    let mut color_replacements: HashMap<String, String> = theme
        .get("colorReplacements")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let mut settings: Vec<Value> = theme
        .get("settings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut fg = string(theme.get("fg"));
    let mut bg = string(theme.get("bg"));
    if fg.is_none() || bg.is_none() {
        let global = settings
            .iter()
            .find(|s| !s.get("name").is_some_and(truthy) && !s.get("scope").is_some_and(truthy));
        if let Some(global) = global.and_then(|s| s.get("settings")) {
            if let Some(color) = string(global.get("foreground")) {
                fg = Some(color);
            }
            if let Some(color) = string(global.get("background")) {
                bg = Some(color);
            }
        }
        let colors = theme.get("colors");
        if fg.is_none() {
            fg = string(colors.and_then(|c| c.get("editor.foreground")));
        }
        if bg.is_none() {
            bg = string(colors.and_then(|c| c.get("editor.background")));
        }
        let light = kind == "light";
        fg.get_or_insert_with(|| if light { "#333333" } else { "#bbbbbb" }.into());
        bg.get_or_insert_with(|| if light { "#fffffe" } else { "#1e1e1e" }.into());
    }
    let (fg, bg) = (fg.expect("set"), bg.expect("set"));
    let first_is_global = settings.first().is_some_and(|first| {
        first.get("settings").is_some_and(truthy) && !first.get("scope").is_some_and(truthy)
    });
    if !first_is_global {
        settings.insert(
            0,
            json!({ "settings": { "foreground": fg, "background": bg } }),
        );
    }
    // Colors that are not hex (CSS variables, names) become `#0000000N`
    // placeholders, replaced back in the tokens.
    let mut replacements: HashMap<String, String> = HashMap::new();
    for setting in settings.iter_mut() {
        for key in ["foreground", "background"] {
            let Some(value) = setting
                .get("settings")
                .and_then(|s| s.get(key))
                .and_then(Value::as_str)
            else {
                continue;
            };
            if value.is_empty() || value.starts_with('#') {
                continue;
            }
            let value = value.to_owned();
            let count = replacements.len() + 1;
            let hex = replacements
                .entry(value.clone())
                .or_insert_with(|| format!("#{count:08x}"))
                .clone();
            color_replacements.insert(hex.clone(), value);
            setting["settings"][key] = Value::String(hex);
        }
    }
    let textmate = Rc::new(Theme::from_settings(&settings));
    NormalizedTheme {
        name: string(theme.get("name")).unwrap_or_default(),
        kind,
        fg,
        bg,
        settings,
        color_replacements,
        textmate,
    }
}

impl NormalizedTheme {
    /// The settings after normalization, as `Theme.createFromRawTheme`
    /// reads them.
    pub fn settings(&self) -> &[Value] {
        &self.settings
    }

    /// `applyColorReplacements` with the theme's replacements.
    pub fn replace_color(&self, color: &str) -> String {
        self.color_replacements
            .get(&color.to_lowercase())
            .cloned()
            .unwrap_or_else(|| color.to_owned())
    }
}

/// A loaded language: `LanguageRegistration` and its grammar.
struct Language {
    name: String,
    grammar: Grammar,
}

/// `isPlainLang`.
pub(crate) fn is_plain_lang(lang: &str) -> bool {
    matches!(lang, "" | "plaintext" | "txt" | "text" | "plain")
}

fn strings(value: Option<&Value>) -> Option<Vec<String>> {
    value.and_then(Value::as_array).map(|items| {
        items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_owned))
            .collect()
    })
}

/// `splitLines(code)`: each line without its ending and its offset in
/// UTF-16 code units.
fn split_lines(code: &str) -> Vec<(&str, usize)> {
    if code.is_empty() {
        return vec![("", 0)];
    }
    let mut lines = Vec::new();
    let mut offset = 0;
    let mut start = 0;
    let bytes = code.as_bytes();
    for (i, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            let end = if i > start && bytes[i - 1] == b'\r' {
                i - 1
            } else {
                i
            };
            let line = &code[start..end];
            lines.push((line, offset));
            offset += utf16_len(line) + (i + 1 - end);
            start = i + 1;
        }
    }
    lines.push((&code[start..], offset));
    lines
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Shiki's primitive with every language and theme loaded up front.
pub struct Highlighter {
    registry: Rc<SyncRegistry>,
    languages: HashMap<String, Language>,
    aliases: HashMap<String, String>,
    themes: Vec<NormalizedTheme>,
}

impl Highlighter {
    /// `langs` are language registrations (grammars with `name`,
    /// `scopeName` and Shiki's fields) in Shiki's load order.
    pub fn new(langs: &[Value], themes: &[Value]) -> Self {
        let themes: Vec<NormalizedTheme> = themes.iter().map(normalize_theme).collect();
        let initial_theme = themes
            .first()
            .map(|t| t.textmate.clone())
            .unwrap_or_else(|| Rc::new(Theme::from_settings(&[])));
        let entries: Vec<(String, Rc<Value>, Vec<String>)> = langs
            .iter()
            .map(|lang| {
                (
                    string(lang.get("scopeName")).unwrap_or_default(),
                    Rc::new(lang.clone()),
                    strings(lang.get("injectTo")).unwrap_or_default(),
                )
            })
            .collect();
        let registry = Rc::new(SyncRegistry::new(&entries, initial_theme));
        let mut languages = HashMap::new();
        let mut aliases = HashMap::new();
        for lang in langs {
            let name = string(lang.get("name")).unwrap_or_default();
            let scope_name = string(lang.get("scopeName")).unwrap_or_default();
            let balanced =
                strings(lang.get("balancedBracketSelectors")).unwrap_or_else(|| vec!["*".into()]);
            let unbalanced = strings(lang.get("unbalancedBracketSelectors")).unwrap_or_default();
            let grammar = Grammar::new(
                &scope_name,
                lang,
                1,
                BalancedBracketSelectors::new(&balanced, &unbalanced),
                registry.clone(),
            );
            for alias in strings(lang.get("aliases")).unwrap_or_default() {
                aliases.insert(alias, name.clone());
            }
            languages.insert(name.clone(), Language { name, grammar });
        }
        Self {
            registry,
            languages,
            aliases,
            themes,
        }
    }

    /// The grammars and themes in `data/`, copied from `@shikijs/langs` and
    /// `@shikijs/themes` 4.6.0 by `reference/build.mjs`.
    pub fn bundled() -> Self {
        let parse = |text: &str| serde_json::from_str::<Value>(text).expect("bundled JSON");
        let langs: Vec<Value> = crate::data::LANGS
            .iter()
            .map(|(_, text)| parse(text))
            .collect();
        let themes: Vec<Value> = crate::data::THEMES
            .iter()
            .map(|(_, text)| parse(text))
            .collect();
        Self::new(&langs, &themes)
    }

    pub fn theme(&self, name: &str) -> Option<&NormalizedTheme> {
        self.themes.iter().find(|theme| theme.name == name)
    }

    /// Whether `getLoadedLanguages()` lists `name`: a language's name or
    /// one of its aliases.
    pub(crate) fn is_loaded(&self, name: &str) -> bool {
        self.languages.contains_key(name) || self.aliases.contains_key(name)
    }

    /// `resolveLangAlias` over the languages' aliases.
    fn language(&self, name: &str) -> Option<&Language> {
        let mut name = name;
        let mut seen = vec![name];
        while let Some(target) = self.aliases.get(name) {
            if seen.contains(&target.as_str()) {
                break;
            }
            name = target;
            seen.push(name);
        }
        self.languages.get(name)
    }

    /// `codeToTokensBase(code, { lang, theme })`.
    pub fn code_to_tokens_base(
        &self,
        code: &str,
        lang: &str,
        theme: &str,
    ) -> Result<Vec<Vec<ThemedToken>>, Error> {
        if is_plain_lang(lang) || theme == "none" {
            return Ok(split_lines(code)
                .into_iter()
                .map(|(line, offset)| {
                    vec![ThemedToken {
                        content: line.to_owned(),
                        offset,
                        color: None,
                        font_style: None,
                    }]
                })
                .collect());
        }
        let theme = self
            .theme(theme)
            .ok_or_else(|| Error::UnknownTheme(theme.to_owned()))?;
        self.registry.set_theme(theme.textmate.clone());
        let language = self
            .language(lang)
            .ok_or_else(|| Error::UnknownLanguage(lang.to_owned()))?;
        debug_assert!(!language.name.is_empty());
        let color_map = theme.textmate.color_map();
        let mut state = None;
        let mut result = Vec::new();
        for (line, line_offset) in split_lines(code) {
            if line.is_empty() {
                result.push(Vec::new());
                continue;
            }
            let tokenized =
                language
                    .grammar
                    .tokenize_line2(line, state.as_ref(), TOKENIZE_TIME_LIMIT)?;
            let tokens = &tokenized.tokens;
            let count = tokens.len() / 2;
            let mut actual = Vec::new();
            for j in 0..count {
                let start = tokens[2 * j] as usize;
                let next = if j + 1 < count {
                    tokens[2 * j + 2] as usize
                } else {
                    line.len()
                };
                if start == next {
                    continue;
                }
                let metadata = tokens[2 * j + 1];
                let color = color_map
                    .get(metadata::foreground(metadata) as usize)
                    .filter(|c| !c.is_empty())
                    .map(|color| {
                        theme
                            .color_replacements
                            .get(&color.to_lowercase())
                            .cloned()
                            .unwrap_or_else(|| color.clone())
                    });
                actual.push(ThemedToken {
                    content: line[start..next].to_owned(),
                    offset: line_offset + utf16_len(&line[..start]),
                    color,
                    font_style: Some(metadata::font_style(metadata)),
                });
            }
            result.push(actual);
            state = Some(tokenized.rule_stack);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_lines_like_shiki() {
        assert_eq!(
            split_lines("a\r\nb\n\né"),
            vec![("a", 0), ("b", 3), ("", 5), ("é", 6)]
        );
        assert_eq!(split_lines(""), vec![("", 0)]);
        assert_eq!(split_lines("🚀\nx"), vec![("🚀", 0), ("x", 3)]);
    }
}
