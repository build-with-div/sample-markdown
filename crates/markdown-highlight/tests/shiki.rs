//! The port against Shiki's own output.
//!
//! `reference/corpus/shiki.json` holds `codeToTokensBase` for each case of
//! `fixtures/shiki-cases.json` and each code block of the fixtures, in both
//! themes, from Shiki 4.6.0 with the Oniguruma engine. Exactly the cases in
//! `shiki-known-differences.txt` may differ (`<theme> <case name>` per
//! line): a fix must remove its line, and a regression fails.
//!
//! Both the corpus and `data/` are built by `reference/build.mjs`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use markdown_highlight::{Highlighter, ThemedToken};
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn expected_tokens(value: &Value) -> Vec<Vec<ThemedToken>> {
    value
        .as_array()
        .expect("lines")
        .iter()
        .map(|line| {
            line.as_array()
                .expect("tokens")
                .iter()
                .map(|token| ThemedToken {
                    content: token["content"].as_str().expect("content").to_owned(),
                    offset: token["offset"].as_u64().expect("offset") as usize,
                    color: token
                        .get("color")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    font_style: token
                        .get("fontStyle")
                        .and_then(Value::as_u64)
                        .map(|n| n as u32),
                })
                .collect()
        })
        .collect()
}

fn describe(tokens: &[Vec<ThemedToken>]) -> String {
    tokens
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let tokens: Vec<String> = line
                .iter()
                .map(|t| {
                    format!(
                        "{:?}@{} {} {}",
                        t.content,
                        t.offset,
                        t.color.as_deref().unwrap_or("-"),
                        t.font_style.map_or("-".into(), |s| s.to_string())
                    )
                })
                .collect();
            format!("    {}: {}", i + 1, tokens.join(" | "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The first line that differs, both ways.
fn first_difference(expected: &[Vec<ThemedToken>], actual: &[Vec<ThemedToken>]) -> String {
    let lines = expected.len().max(actual.len());
    for i in 0..lines {
        let (e, a) = (expected.get(i), actual.get(i));
        if e != a {
            let one =
                |line: Option<&Vec<ThemedToken>>| describe(&[line.cloned().unwrap_or_default()]);
            return format!(
                "line {}\n  expected:\n{}\n  actual:\n{}",
                i + 1,
                one(e),
                one(a)
            );
        }
    }
    String::new()
}

#[test]
fn bundled_data_follows_the_manifest() {
    let manifest = read_json(&root().join("crates/markdown-highlight/data/manifest.json"));
    let langs: Vec<&str> = manifest["langs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let themes: Vec<&str> = manifest["themes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        markdown_highlight::bundled_languages().collect::<Vec<_>>(),
        langs
    );
    assert_eq!(
        markdown_highlight::bundled_themes().collect::<Vec<_>>(),
        themes
    );
}

#[test]
fn tokens_and_colors_equal_shiki_except_where_listed() {
    let corpus = read_json(&root().join("reference/corpus/shiki.json"));
    let highlighter = Highlighter::bundled();
    for (name, colors) in corpus["themes"].as_object().unwrap() {
        let theme = highlighter.theme(name).expect("theme");
        assert_eq!(theme.fg, colors["fg"].as_str().unwrap(), "{name} fg");
        assert_eq!(theme.bg, colors["bg"].as_str().unwrap(), "{name} bg");
    }
    let known: BTreeSet<String> = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/shiki-known-differences.txt"),
    )
    .expect("known differences")
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty() && !line.starts_with('#'))
    .map(str::to_owned)
    .collect();
    let mut differing = BTreeSet::new();
    let mut reports = Vec::new();
    let mut count = 0;
    let mut token_count = 0;
    let mut colors = BTreeSet::new();
    for case in corpus["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let code = case["code"].as_str().unwrap();
        let lang = case["lang"].as_str().unwrap();
        for (theme, tokens) in case["tokens"].as_object().unwrap() {
            count += 1;
            let expected = expected_tokens(tokens);
            token_count += expected.iter().map(Vec::len).sum::<usize>();
            colors.extend(expected.iter().flatten().filter_map(|t| t.color.clone()));
            let actual = highlighter
                .code_to_tokens_base(code, lang, theme)
                .unwrap_or_else(|e| panic!("{name} ({theme}): {e}"));
            if actual != expected {
                let key = format!("{theme} {name}");
                reports.push(format!("{key}: {}", first_difference(&expected, &actual)));
                differing.insert(key);
            }
        }
    }
    assert!(count >= 38, "found {count} case-theme pairs");
    // Not a comparison of uncolored text: both themes' palettes appear.
    assert!(
        colors.len() >= 16,
        "the corpus uses {} colors",
        colors.len()
    );
    eprintln!(
        "{} of {count} case-theme pairs equal Shiki ({token_count} tokens expected)",
        count - differing.len()
    );
    let unexpected: Vec<&String> = differing.difference(&known).collect();
    let fixed: Vec<&String> = known.difference(&differing).collect();
    assert!(
        unexpected.is_empty() && fixed.is_empty(),
        "unexpected differences: {unexpected:?}\nlisted but now equal: {fixed:?}\n{}",
        reports.join("\n")
    );
}
