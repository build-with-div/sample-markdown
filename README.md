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
remark-gfm, rehype-raw and, as an option, rehype-sanitize to Rust;
`crates/markdown-div` mounts the result in Div, and `examples/viewer` is
a viewer app built on them (screenshots in
[docs/design.md](docs/design.md#p2-3-result)). `crates/markdown-highlight`
ports Shiki's code highlighting and `@shikijs/rehype`; its tokens equal
Shiki's, and its highlighted blocks equal the reference pages'
([P2-4](docs/design.md#p2-4-result), [P2-4b](docs/design.md#p2-4b-result)).
Div does not apply their colors yet (MD-24). See
[docs/design.md](docs/design.md) for the findings and plan, and
[docs/rfc-markdown.md](docs/rfc-markdown.md) for the Div APIs it proposes.

## Layout

| Path | What |
| --- | --- |
| `fixtures/*.md` | Small GFM documents, one feature each, plus a README-like page |
| `fixtures/gfm-cases.json` | Short GFM and raw-HTML cases for the tree test |
| `fixtures/shiki-cases.json` | Code in 11 languages for the highlighting test |
| `reference/` | `build.mjs` turns each fixture into a Chromium reference page (react-markdown, remark-gfm, rehype-raw, `@shikijs/rehype`, github-markdown-css), light and dark; the pages are committed |
| `reference/corpus/` | The fixtures, the CommonMark spec's examples and the GFM cases rendered by the same packages, without and with rehype-sanitize; `shiki.json`, Shiki's tokens for the highlighting cases; `rehype-shiki.json`, all of these with `@shikijs/rehype` |
| `crates/markdown-tree` | The port: Markdown to the DOM Chromium builds for react-markdown, checked against `reference/corpus/` |
| `crates/markdown-highlight` | A port of Shiki's highlighting (vscode-textmate on Oniguruma) with Shiki's grammars and themes in `data/`, and `codeToHast` for `markdown-tree`; checked against `reference/corpus/shiki.json`, `rehype-shiki.json` and the reference pages |
| `crates/markdown-div` | Mounts that tree in a Div document through the public API, with a report of what Div cannot take |
| `examples/viewer` | `markdown-viewer`: open a `.md` file, scroll it, switch the theme; code blocks are highlighted with the theme's Shiki theme. What Div cannot do yet is labeled `div-gap` |
| `harness/render` | `markdown-render`: builds a page with Div's public API, from a fixture's Markdown or a captured tree, and renders it |
| `harness/compare.py` | Captures each page in pinned Chromium, renders it with Div, compares pixels and element boxes |
| `harness/css_inventory.py` | Lists the selectors, at-rules, properties and functions a stylesheet uses |

## Run the viewer

```sh
cargo run -p markdown-viewer --features windows -- fixtures/readme.md   # or --features linux
cargo run -p markdown-viewer -- --png viewer.png fixtures/readme.md     # headless, to a PNG
```

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
`crates/markdown-highlight/data` holds TextMate grammars and themes copied
unchanged from `@shikijs/langs` and `@shikijs/themes` (MIT, Shiki); each
keeps its upstream license, listed by Shiki's tm-grammars and tm-themes
([data/README.md](crates/markdown-highlight/data/README.md)).
