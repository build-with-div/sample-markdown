//! The whole pipeline with `@shikijs/rehype` against the npm packages' own
//! output: `markdown_tree::parse_with` and [`RehypeShiki`].
//!
//! - Every reference page's tree equals the fixture's tree, highlighted
//!   with the page's Shiki theme (`highlight` in the pages' manifest).
//! - Over `reference/corpus/rehype-shiki.json` (the fixtures, the Shiki
//!   cases as fenced blocks, the CommonMark spec's examples and the GFM
//!   cases), with github-light, github-dark and github-light after
//!   rehype-sanitize, exactly the examples listed in
//!   `tests/rehype-shiki[-dark|-sanitized]-known-differences.txt` differ.
//!   A fix must remove its number from the list, and a regression fails.
//!
//! Both inputs are built by `reference/build.mjs` from the pinned packages.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use markdown_highlight::{Highlighter, RehypeShiki};
use markdown_tree::{parse_html, parse_with, to_html, Node, Options};
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// The content of the page's `<article>`, without the template's line
/// endings around it.
fn article(page: &str) -> &str {
    let start = page
        .find("<article class=\"markdown-body\">\n")
        .expect("article")
        + 32;
    let end = page.rfind("\n</article>").expect("article end");
    &page[start..end]
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

/// Counts the `<pre class="shiki …">` elements, to make sure the
/// comparison covers highlighted blocks.
fn shiki_blocks(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            Node::Text(_) => 0,
            Node::Element(element) => {
                let own = element.name == "pre"
                    && element
                        .attribute("class")
                        .is_some_and(|class| class.starts_with("shiki "));
                usize::from(own) + shiki_blocks(&element.children)
            }
        })
        .sum()
}

#[test]
fn fixtures_match_the_reference_pages() {
    let highlighter = Highlighter::bundled();
    let manifest: Value =
        serde_json::from_str(&read(&root().join("reference/pages/manifest.json"))).unwrap();
    let mut failures = Vec::new();
    let mut count = 0;
    let mut highlighted = 0;
    for fixture in manifest["fixtures"].as_array().unwrap() {
        let id = fixture["id"].as_str().unwrap();
        let theme = fixture["highlight"].as_str().unwrap();
        let page = read(
            &root()
                .join("reference/pages")
                .join(fixture["page"].as_str().unwrap()),
        );
        let expected = parse_html(article(&page));
        let highlight = RehypeShiki {
            highlighter: &highlighter,
            theme,
        };
        let options = Options {
            highlight: Some(&highlight),
            ..Options::default()
        };
        let actual = parse_with(
            &read(&root().join(fixture["source"].as_str().unwrap())),
            options,
        );
        count += 1;
        highlighted += shiki_blocks(&expected);
        assert!(
            text_length(&expected) > 20,
            "{id}: the reference page parsed to almost nothing"
        );
        if actual != expected {
            failures.push(format!(
                "{id}\n  expected: {}\n  actual:   {}",
                to_html(&expected),
                to_html(&actual)
            ));
        }
    }
    assert!(count >= 24, "found {count} pages");
    assert!(highlighted >= 4, "found {highlighted} highlighted blocks");
    println!("{count} pages equal, with {highlighted} highlighted blocks");
    assert!(
        failures.is_empty(),
        "{} of {count} pages differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn highlighted_corpus_differs_only_where_listed() {
    check_corpus("rehype-shiki", "html", "github-light", false);
}

#[test]
fn dark_highlighted_corpus_differs_only_where_listed() {
    check_corpus("rehype-shiki-dark", "html_dark", "github-dark", false);
}

#[test]
fn sanitized_highlighted_corpus_differs_only_where_listed() {
    check_corpus(
        "rehype-shiki-sanitized",
        "html_sanitized",
        "github-light",
        true,
    );
}

fn check_corpus(name: &str, key: &str, theme: &str, sanitize: bool) {
    let corpus: Value =
        serde_json::from_str(&read(&root().join("reference/corpus/rehype-shiki.json"))).unwrap();
    let highlighter = Highlighter::bundled();
    let highlight = RehypeShiki {
        highlighter: &highlighter,
        theme,
    };
    let options = Options {
        sanitize,
        highlight: Some(&highlight),
    };
    let list =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/{name}-known-differences.txt"));
    let known: BTreeSet<u64> = read(&list)
        .lines()
        .filter_map(|line| line.split('#').next().unwrap().trim().parse().ok())
        .collect();
    let examples = corpus["examples"].as_array().unwrap();
    let mut differ = BTreeSet::new();
    let mut highlighted = 0;
    let mut report = Vec::new();
    for example in examples {
        let number = example["number"].as_u64().unwrap();
        let markdown = example["markdown"].as_str().unwrap();
        let html = example[key]
            .as_str()
            .unwrap_or_else(|| panic!("{name} #{number}: the reference failed to render"));
        let expected = parse_html(html);
        let actual = parse_with(markdown, options);
        highlighted += shiki_blocks(&expected);
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
        highlighted >= 15,
        "{name}: only {highlighted} highlighted blocks in the corpus"
    );
    let fixed: Vec<_> = known.difference(&differ).collect();
    println!(
        "{name} corpus: {} of {} examples equal, {highlighted} highlighted blocks",
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
