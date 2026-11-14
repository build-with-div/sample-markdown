# Div gaps found by this sample

Each `// div-gap:` label in the code names an entry here. The MD numbers
point to the [gap inventory](design.md#gap-inventory), which has the
evidence, owner and size. Every workaround below is temporary scaffolding,
to be removed when Div fixes the gap. None is a technique for apps.

Checked against Div `565db46a`; the gaps are unchanged at `92ea25a0` (same measured values).

| Label | Gap | Workaround here | Where | Inventory |
| --- | --- | --- | --- | --- |
| G-CASCADE | No UA stylesheet and no author stylesheets, so github-markdown-css cannot apply | Computed mode copies Chromium's computed style onto each element. It is used only to measure layout, paint and text. The fix is Div's UA stylesheet (S5) and A1 with the browser-compatible ingestion mode adopted on 13 November 2026, not this copy | `harness/render/src/main.rs` (`Builder::style`) | MD-3, MD-4, MD-5, MD-6 |
| G-TAGS | `Element::create` rejects `blockquote`, `pre`, `code`, `del`, `kbd`, `sub`, `sup` | `span` or `div` stand-in. The captured tree picks it by Chromium's `display`; `markdown-div` picks it by HTML's phrasing content category, and both count every stand-in | `Builder::element`; `crates/markdown-div` (`Mounter::element`) | MD-1 |
| G-ROOT | `Document::body()` is the root, with no `html`/`body` pair, and given an 8px margin it still fills the viewport | The page's `<body>` is built as a `div` inside Div's body | `build`, `build_from_source` | MD-2 |
| G-LITERALS | `border-*-width` takes `1` but not `1px`; the `list-style-type` longhand is named `list-style` in `StyleProperty::from_css_name` | Retry with the bare number or the other name, counted as "normalized" in the report | `Builder::style` | MD-10 |
| G-IMG-SRC | `<img src>` loads nothing, and `register_image_resource` needs the caller to supply the MIME type and the SHA-256 that Div keys its image cache on | The app's loader supplies the bytes, the MIME type (a static server's, from the extension) and the SHA-256. Then `register_image_resource` and `set_image_resource` | `Builder::image`; `markdown-div` (`Mounter::image`) with the loader in `build_from_source` | MD-15 |
| G-IMG-SIZE | No intrinsic size from the image bytes (300×150 without one) | The captured tree passes the natural size Chromium decoded. `markdown-div` passes none and does not decode images itself, so from source each image is 300×150 (measured) | `Builder::image`; `Mounter::image` | MD-15 |
| G-PKG | A fresh dependency resolve picks a `zbus_macros` that does not build on Linux (sample-git gap 24) | `Cargo.lock` seeded from Div's lock | `Cargo.toml` | (sample-git 24) |

## Not worked around (they show in the results)

- **MD-7 List markers.** `list-style-type` takes only `none` and `disc`, so
  ordered lists show discs, and `counter-increment` is rejected.
- **MD-8 Tables.** Table row groups don't share columns, and `width:
  max-content` is rejected.
- **MD-9 Text decoration.** `text-decoration-line` paints no line.
- **MD-10 Border radius.** `border-*-radius` with one value is rejected.
- **MD-11 Checkboxes.** A checkbox has a 0×0 box.
- **MD-12 Text metrics.** Line and glyph positions differ.
- **MD-13 Pseudo-elements.** There is no `::before` or `::after` in the
  public API.
- **MD-14 Disclosure markers.** `disclosure-open` and `disclosure-closed`
  are rejected.
- **MD-6 Theme.** The viewer's theme switch changes its own toolbar and the
  article's `data-theme`, which nothing in Div reads. It cannot start in the
  system's theme (`examples/viewer`, `Viewer::set_theme`).
- **MD-17 Links.** Clicking a footnote reference does not scroll to the
  footnote; links do nothing (`div_gap_md17_fragment_links_do_not_navigate`).
- **MD-23 Keyboard scrolling.** No key scrolls the viewer's document,
  focused or not (`div_gap_md23_keys_do_not_scroll`).
- **MD-24 Inline `style` attribute.** Div keeps a `style` attribute but
  does not apply its declarations, so Shiki's highlighted code shows the
  inherited color in semantic mode and in the viewer
  (`div_gap_md24_style_attribute_is_not_applied` in `crates/markdown-div`,
  `div_gap_md24_highlighted_code_shows_no_color` in `examples/viewer`).
