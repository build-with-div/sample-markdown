//! The port against the npm packages' own output.
//!
//! - Every fixture's tree equals the DOM of its reference page.
//! - Over each corpus (the CommonMark spec's examples and the GFM cases in
//!   `fixtures/gfm-cases.json`), exactly the examples listed in
//!   `<corpus>-known-differences.txt` differ. A fix must remove its number
//!   from the list, and a regression fails.
//!
//! Both inputs are built by `reference/build.mjs` from the pinned packages.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use markdown_tree::{parse, parse_html, to_html, Node};

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

#[test]
fn fixtures_match_the_reference_pages() {
    let mut failures = Vec::new();
    let mut count = 0;
    for entry in fs::read_dir(root().join("fixtures")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let id = path.file_stem().unwrap().to_string_lossy().into_owned();
        let page = read(&root().join(format!("reference/pages/{id}-light.html")));
        let expected = parse_html(article(&page));
        let actual = parse(&read(&path));
        count += 1;
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
    assert!(count >= 12, "found {count} fixtures");
    assert!(
        failures.is_empty(),
        "{} of {count} fixtures differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn commonmark_corpus_differs_only_where_listed() {
    check_corpus("commonmark", 10_000);
}

#[test]
fn gfm_corpus_differs_only_where_listed() {
    check_corpus("gfm", 1_000);
}

fn check_corpus(name: &str, minimum_text: usize) {
    let corpus: serde_json::Value =
        serde_json::from_str(&read(&root().join(format!("reference/corpus/{name}.json")))).unwrap();
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
        let Some(html) = example["html"].as_str() else {
            continue;
        };
        let expected = parse_html(html);
        let actual = parse(markdown);
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
        "{} unexpected differences, {} listed examples now equal ({fixed:?}):\n{}",
        report.len(),
        fixed.len(),
        report.join("\n")
    );
}
