# Div gaps found by this sample

Each `// div-gap:` label in the code names an entry here. The MD numbers
point to the [gap inventory](design.md#gap-inventory), which has the
evidence, owner and size. Every workaround below is temporary scaffolding,
to be removed when Div fixes the gap. None is a technique for apps.

Checked against Div `565db46a`.

| Label | Gap | Workaround here | Where | Inventory |
| --- | --- | --- | --- | --- |
| G-CASCADE | No UA stylesheet and no author stylesheets, so github-markdown-css cannot apply | Computed mode copies Chromium's computed style onto each element. It is used only to measure layout, paint and text | `harness/render/src/main.rs` (`Builder::style`) | MD-3, MD-4, MD-5, MD-6 |
| G-TAGS | `Element::create` rejects `blockquote`, `pre`, `code`, `del`, `kbd`, `sub`, `sup` | `span` or `div` stand-in, chosen by Chromium's `display` | `Builder::element` | MD-1 |
| G-ROOT | `Document::body()` is the root, with no `html`/`body` pair, and given an 8px margin it still fills the viewport | The captured `<body>` is built as a `div` inside Div's body | `build` | MD-2 |
| G-LITERALS | `border-*-width` takes `1` but not `1px`; the `list-style-type` longhand is named `list-style` in `StyleProperty::from_css_name` | Retry with the bare number or the other name, counted as "normalized" in the report | `Builder::style` | MD-10 |
| G-IMG-SRC | `<img src>` loads nothing | Read the file, `register_image_resource`, `set_image_resource` | `Builder::image` | MD-15 |
| G-IMG-SIZE | No intrinsic size from the image bytes (300×150 without one) | Pass the natural size Chromium decoded | `Builder::image` | MD-15 |
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
