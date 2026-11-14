# Sample Markdown

Markdown rendered by [Div](https://github.com/build-with-div/div), a native
Rust UI framework, measured against the same Markdown rendered in Chromium.
It is a sample: it uses only Div's public API, and its job is to find what
Div still lacks to show Markdown exactly as a browser does.

The target is the web's usual stack: react-markdown with remark-gfm for the
HTML, github-markdown-css for GitHub's look, and later Shiki or Prism for
code, KaTeX for math and Mermaid for diagrams. Div should render that HTML
and CSS the way Chromium does, so a Markdown view is a port of those
packages rather than a re-creation of their look.

Status: phase 2. `crates/markdown-tree` ports react-markdown with
remark-gfm and rehype-raw to Rust. No viewer app yet. See
[docs/design.md](docs/design.md) for the findings and plan, and
[docs/rfc-markdown.md](docs/rfc-markdown.md) for the Div APIs it proposes.

## Layout

| Path | What |
| --- | --- |
| `fixtures/*.md` | Small GFM documents, one feature each, plus a README-like page |
| `fixtures/gfm-cases.json` | Short GFM and raw-HTML cases for the tree test |
| `reference/` | `build.mjs` turns each fixture into a Chromium reference page (react-markdown, remark-gfm, rehype-raw, github-markdown-css), light and dark; the pages are committed |
| `reference/corpus/` | The CommonMark spec's examples and the GFM cases rendered by the same packages |
| `crates/markdown-tree` | The port: Markdown to the DOM Chromium builds for react-markdown, checked against `reference/` |
| `harness/render` | `markdown-render`: rebuilds a captured page tree with Div's public API and renders it |
| `harness/compare.py` | Captures each page in pinned Chromium, renders it with Div, compares pixels and element boxes |
| `harness/css_inventory.py` | Lists the selectors, at-rules, properties and functions a stylesheet uses |

## Reproduce the comparison

The comparison runs in Linux (WSL here), with Chrome for Testing
147.0.7727.50 and a checkout of Div at the commit pinned in `Cargo.toml`,
which provides the census font environment.

```sh
cd reference && npm ci && node build.mjs && cd ..   # only after changing fixtures
cargo build --release -p markdown-render            # with Div's Skia build variables
python3 harness/compare.py --div ~/div \
    --renderer target/release/markdown-render --output out/fidelity/results.json
```

Div compiles Skia from source; see sample-git's README for the toolchain
(Rust 1.94.1, Python 3, Ninja, LLVM; on Windows the VS 2022 Build Tools).
`.cargo/config.toml` sets the Skia build variables of Div's CI.

## License

Apache-2.0. The reference pages include github-markdown-css (MIT,
Sindre Sorhus), copied unmodified from the npm package.
