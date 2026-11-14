//! The port against the npm packages' own output.
//!
//! Over each corpus (the fixtures, the CommonMark spec's examples and the
//! GFM cases in `fixtures/gfm-cases.json`), without and with
//! rehype-sanitize, exactly the examples listed in
//! `<corpus>[-sanitized]-known-differences.txt` differ. A fix must remove
//! its number from the list, and a regression fails.
//!
//! The corpora are built by `reference/build.mjs` from the pinned packages.
//! The reference pages highlight code with `@shikijs/rehype`, so the test
//! of the pages is in `markdown-highlight` (`tests/rehype.rs`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use markdown_tree::{parse_html, parse_with, to_html, Node, Options};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// The length of all text in the tree, to make sure a comparison is not
/// between two empty trees.
fn text_length(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Node::Text(text) => text.len(),
            Node::Element(element) => element.name.len() + text_length(&element.children),
        })
        .sum()
}

#[test]
fn commonmark_corpus_differs_only_where_listed() {
    check_corpus("commonmark", false, 10_000);
}

#[test]
fn gfm_corpus_differs_only_where_listed() {
    check_corpus("gfm", false, 1_000);
}

#[test]
fn fixtures_corpus_differs_only_where_listed() {
    check_corpus("fixtures", false, 1_000);
}

#[test]
fn sanitized_commonmark_corpus_differs_only_where_listed() {
    check_corpus("commonmark", true, 10_000);
}

#[test]
fn sanitized_gfm_corpus_differs_only_where_listed() {
    check_corpus("gfm", true, 1_000);
}

#[test]
fn sanitized_fixtures_corpus_differs_only_where_listed() {
    check_corpus("fixtures", true, 1_000);
}

/// Checks one corpus against react-markdown's output, without or with
/// rehype-sanitize (`html` or `html_sanitized`).
fn check_corpus(corpus_name: &str, sanitize: bool, minimum_text: usize) {
    let corpus: serde_json::Value = serde_json::from_str(&read(
        &root().join(format!("reference/corpus/{corpus_name}.json")),
    ))
    .unwrap();
    let (name, key) = if sanitize {
        (format!("{corpus_name}-sanitized"), "html_sanitized")
    } else {
        (corpus_name.to_owned(), "html")
    };
    let options = Options {
        sanitize,
        ..Options::default()
    };
    let list =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/{name}-known-differences.txt"));
    let known: BTreeSet<u64> = read(&list)
        .lines()
        .filter_map(|line| line.split('#').next().unwrap().trim().parse().ok())
        .collect();
    let examples = corpus["examples"].as_array().unwrap();
    let mut differ = BTreeSet::new();
    let mut total_text = 0;
    let mut report = Vec::new();
    for example in examples {
        let number = example["number"].as_u64().unwrap();
        let markdown = example["markdown"].as_str().unwrap();
        let html = example[key]
            .as_str()
            .unwrap_or_else(|| panic!("{name} #{number}: the reference failed to render"));
        let expected = parse_html(html);
        let actual = parse_with(markdown, options);
        total_text += text_length(&expected);
        if actual != expected {
            differ.insert(number);
            if !known.contains(&number) {
                report.push(format!(
                    "#{number} ({})\n  markdown: {markdown:?}\n  expected: {:?}\n  actual:   {:?}",
                    example["section"].as_str().unwrap_or(""),
                    to_html(&expected),
                    to_html(&actual)
                ));
            }
        }
    }
    assert!(
        total_text > minimum_text,
        "{name}: the corpus parsed to almost no text ({total_text} bytes)"
    );
    let fixed: Vec<_> = known.difference(&differ).collect();
    println!(
        "{name} corpus: {} of {} examples equal",
        examples.len() - differ.len(),
        examples.len()
    );
    assert!(
        report.is_empty() && fixed.is_empty(),
        "{name}: {} unexpected differences, {} listed examples now equal ({fixed:?}):\n{}",
        report.len(),
        fixed.len(),
        report.join("\n")
    );
}
