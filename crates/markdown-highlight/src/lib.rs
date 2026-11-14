//! Code highlighting as Shiki 4.6.0 does it: `codeToTokensBase` with the
//! Oniguruma engine.
//!
//! Shiki runs TextMate grammars with `@shikijs/vscode-textmate` (Shiki's
//! fork of vscode-textmate) on Oniguruma compiled to WebAssembly. The
//! [`textmate`] module ports that tokenizer to Rust on the `onig` crate's
//! Oniguruma, and [`Highlighter`] ports Shiki's primitive: its language
//! registry, `normalizeTheme` and the per-line token loop. The grammars and
//! themes are Shiki's own files (`data/`), so the tokens and colors are
//! Shiki's: `tests/shiki.rs` compares them with Shiki's output
//! (`reference/corpus/shiki.json`).
//!
//! Nothing here depends on Div. Turning the tokens into styled elements in
//! a Div document is a separate step.

mod data;
pub mod shiki;
pub mod textmate;

pub use shiki::{normalize_theme, Error, Highlighter, NormalizedTheme, ThemedToken};

/// The bundled languages, in Shiki's load order, and themes.
pub fn bundled_languages() -> impl Iterator<Item = &'static str> {
    data::LANGS.iter().map(|(name, _)| *name)
}

pub fn bundled_themes() -> impl Iterator<Item = &'static str> {
    data::THEMES.iter().map(|(name, _)| *name)
}
