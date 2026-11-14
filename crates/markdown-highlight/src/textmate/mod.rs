//! A port of `@shikijs/vscode-textmate` 10.1.0, Shiki's fork of
//! vscode-textmate: TextMate grammars tokenized line by line, with scopes
//! styled by a TextMate theme. Module names follow its `src/` files.

pub mod grammar;
pub mod matcher;
pub mod metadata;
pub mod raw;
pub mod registry;
pub mod rule;
pub mod scanner;
pub mod theme;
pub mod utils;
