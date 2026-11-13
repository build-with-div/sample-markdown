# RFC draft: rendering Markdown exactly as Chromium does

Status: draft from build-with-div/sample-markdown, 13 November 2026, for the
Div tracks to review. It is not a Div RFC yet. If accepted, parts move into
`docs/alpha/rfcs/` in build-with-div/div, or amend RFC 0001 and RFC 0004.
Evidence: [design.md](design.md) and [fidelity-results.md](fidelity-results.md),
measured against Div `565db46a` and Chrome for Testing 147.

## Problem

A Markdown view on the web is react-markdown (or marked or markdown-it)
producing ordinary HTML, styled by github-markdown-css. Div can't render
that HTML today:

- it rejects 7 of the tags (MD-1);
- it has no user-agent or author stylesheet (MD-3, MD-4);
- even with Chromium's computed styles copied in, lists, tables, text
  decoration, rounded corners and checkboxes render wrong (MD-7 to MD-14).

The principle is to port the packages by implementing the platform
features they use, matched to Chromium and measured. This RFC lists the
Div APIs that needs. Most already have a home in RFC 0001 (A1), RFC 0004
(presets) or A2. The new requests are §3, §5, §6, §7 and §8.

## 1. Element identity (amends RFC 0004 prerequisite 1)

`Element::create` and `view!` accept every element GitHub's HTML sanitizer
allows, with the name kept:

```text
a abbr b bdo blockquote br caption cite code dd del details dfn div dl dt em
figcaption figure h1 h2 h3 h4 h5 h6 hr i img input ins kbd li mark ol p
picture pre q rp rt ruby s samp section small source span strike strong sub
summary sup table tbody td tfoot th thead time tr tt ul var wbr
```

```rust
impl Element {
    /// The element's HTML local name ("blockquote", "h2"), as created.
    pub fn local_name(&self) -> Result<LocalName, Error>;
}
```

The phrasing elements map to `ElementTag::Span`, the flow elements to
`ElementTag::Div`, and `pre`, `blockquote` and `figure` are blocks. Their
look then comes only from the preset (§2) and the sheets (§3), as in a
browser. The same names feed type selectors and accessibility roles: heading
levels, `list`/`listitem`, `table` roles, `link`.

**Root.** The document gets an `html` root with `body` inside it, so the UA
rule `body { margin: 8px }` and the propagation of the root and body
backgrounds to the canvas behave as in Chromium (MD-2). `Document::body()`
keeps returning `body`.

## 2. Browser preset: Markdown's parts first (RFC 0004, unchanged API)

Request: in RFC 0004's migration step 6, ship "text and sections" and then
"lists and tables" first, covering the element list above. Also map these
presentational attributes as Chromium does: `align` on `p`, `div`, `img`,
`h1`–`h6`, `td` and `th`; `width` and `height` on `img`; `start` and
`reversed` on `ol`; `value` on `li`. Markdown's raw HTML uses them (MD-18).
GitHub's sanitizer strips `style` attributes, so GitHub parity needs no
runtime CSS text.

Verification as RFC 0004 proposes. The fixtures in this repository also
measure it: semantic mode plus the preset, against pages with no author
CSS.

## 3. Third-party stylesheets

RFC 0001 makes an unsupported selector or at-rule a compile error. That's
right for an app's own CSS. A sheet from npm has to keep its text, though,
and github-markdown-css contains rules no browser applies to Markdown either:

- vendor pseudo-elements: `::-webkit-inner-spin-button`,
  `::-webkit-file-upload-button` and others;
- properties: `-ms-text-size-adjust`, `-webkit-appearance`.

Proposal: a compatibility mode on `stylesheet!`:

```rust
stylesheet!(github_markdown, "github-markdown.css", compat = Browser);
```

- **Selectors.** A selector list with a selector Div can't match is dropped
  as Chromium drops an invalid one: the whole rule, except inside the
  forgiving lists of `:is()` and `:where()`. A selector that is valid CSS
  but not implemented yet is reported separately, because Chromium would
  apply it.
- **Properties and values.** A property or value Chromium ignores (unknown
  property, `-ms-` prefix) is dropped the same way.
- **Report.** The macro writes the dropped rules to a build report.
  `#[deny(div::unsupported_css)]` turns "valid but not implemented" into an
  error, so a port can see what it loses.
- **Default.** Strict mode stays the default.

**Features this one sheet needs from A1:**

| Kind | Needed |
| --- | --- |
| Combinators | descendant, child `>`, adjacent sibling `+` |
| Attribute selectors | `[type=checkbox]`, `[align=center]`, `[data-footnote-ref]`, `[role=…]`, `[hidden]` |
| Pseudo-classes | `:hover`, `:focus`, `:focus-visible`, `:focus-within`, `:not()`, `:has()`, `:first-child`, `:last-child`, `:nth-child()`, `:target`, `:dir()` |
| Pseudo-elements | `::before`, `::after`, `::placeholder` |

## 4. Tokens and media (A2, unchanged plan)

github-markdown-css needs:

- **Custom properties:** 60, with 124 `var()` uses, set per theme on
  `.markdown-body` and `[data-theme]`.
- **Media queries:** `@media (prefers-color-scheme: dark)` and `(… light)`;
  the page style adds `max-width: 767px`.
- **Color scheme:** `Document::set_color_scheme` (RFC 0001), plus a
  framework default that follows the OS theme and updates on change, on
  Windows and Linux.

Shiki's dual themes need the same mechanism: a `--shiki-dark` variable
under a media query.

## 5. Images: `<img src>`

```rust
impl Document {
    /// Base for relative `src` and `href` (a file: or https: URL).
    pub fn set_base_url(&self, url: &str) -> Result<(), Error>;
}
```

- **Loading.** Setting `src` on an `img` asks the document's resource
  provider (`set_resource_provider`, which exists) for the resolved URL. An
  async provider form serves `https:` through the `http` feature. The
  element shows its alt text or nothing until the bytes arrive, as
  Chromium does.
- **Size.** The intrinsic size and aspect ratio come from the decoded bytes:
  PNG, JPEG, GIF, WebP, and SVG `width`/`height`/`viewBox`. Today the app
  must pass a size, and without one the box is 300×150.
- **Errors.** A load error fires `error` and lays out as a broken image.
- **SVG.** SVG images must draw paths, strokes and text: README badges are
  SVG with text. That is engine paint work, shared with the components
  track's icon need.

## 6. Links

- **Element.** `a[href]` is focusable, activates on click and Enter, and
  matches `:any-link`. Its cursor (`pointer`) comes from the preset.
- **Navigation.** Activation calls a document-level handler:

  ```rust
  pub enum Navigation<'a> { Fragment(&'a str), Url(&'a str) }
  impl Document {
      pub fn on_navigate(&self, f: impl Fn(Navigation) -> NavigationResult + 'static);
  }
  ```

  The default for `Fragment` scrolls to the element whose `id` matches
  (`scroll_into_view`, which exists) and sets `:target`. The default for
  `Url` does nothing. The app decides.
- **Opening.** `div::open_url(url)` opens in the user's browser:
  `ShellExecuteW` on Windows, the XDG portal `OpenURI` or `xdg-open` on
  Linux.

## 7. Document text selection

Reading apps need it, Markdown views first among them.

- **API.** A selection over the document, not only inside form controls:

  ```rust
  impl Document {
      pub fn selection(&self) -> Result<Option<SelectionRange>, Error>;
      pub fn selected_text(&self) -> Result<String, Error>;  // innerText rules
      pub fn select_all(&self) -> Result<(), Error>;
  }
  ```

- **Input:** drag to select, double-click for a word, triple-click for a
  paragraph, Shift+click, Ctrl+A.
- **CSS:** `user-select` and `::selection` painting, with Chromium's
  default colors.
- **Copy:** Ctrl+C and the Copy menu role copy `selected_text()` as plain
  text (HTML later). Today `copy_selection` reads only the focused control.

## 8. The Markdown component

A `div-markdown` crate, owned by the components track, after §1–§4.

**API:**

```rust
#[component]
pub fn Markdown(
    source: Signal<String>,
    #[prop(default = MarkdownTheme::System)] theme: MarkdownTheme, // Light, Dark, System
    #[prop(optional)] base_url: Option<String>,
    #[prop(optional)] highlight: Option<Rc<dyn Highlighter>>,
    #[prop(optional)] options: MarkdownOptions, // GFM extensions, raw HTML policy
) -> ViewNode;

pub trait Highlighter {
    /// Colored runs for one code block; the component wraps them in spans.
    fn highlight(&self, language: Option<&str>, code: &str) -> Vec<Vec<(Color, Range<usize>)>>;
}
```

**Behavior:**

- **Parsing:** comrak with GFM extensions (tables, task lists,
  strikethrough, autolinks, footnotes). It builds the element tree
  react-markdown builds, with the same tags, attributes and classes, and
  raw HTML through GitHub's sanitizer allow-list. A test compares the tree
  with `reference/pages` for every fixture.
- **Styling:** `github-markdown-css` compiled with
  `stylesheet!(…, compat = Browser)` and adopted on mount. The theme sets
  `data-theme` or the color scheme.
- **Highlighting:** a syntect implementation, with Shiki's grammars and
  themes, ships behind a feature.
- **Updates:** a changed source re-renders, keyed by block, so a large
  document does not rebuild on each keystroke.

## 9. Small engine fixes

Measured by the computed mode here; each is S:

- **Text decoration.** `text-decoration-line` must paint (MD-9). Right now
  `line-through` and `underline` paint nothing.
- **Literals** (MD-10):
  - `border-*-radius: <length>` with one value;
  - `border-*-width: 1px`;
  - `list-style-type` under its own name;
  - `overflow-clip-margin: content-box`;
  - `width: max-content` (also `min-content` and `fit-content`).
- **Missing from the schema** (MD-10): `appearance`, `color-scheme`,
  `caret-color`, logical longhands, `text-size-adjust`.
- **Disclosure markers.** `list-style-type: disclosure-open` and
  `disclosure-closed` must parse (MD-14).
- **Counters** (MD-7): list counter styles (`decimal`, `lower-roman`,
  `circle`, `square`), the `list-item` counter, `ol start`, `li value`, and
  `list-style-position: inside`.

## Not proposed

- **Runtime CSS text for content.** GitHub strips `style` attributes, and
  everything else is static sheets compiled at build time (D1 holds).
- **Mermaid.** It depends on JavaScript layout and text measurement. A Rust
  port of its layout is a separate project, and Div would need SVG text,
  strokes and markers first.
- **MathML.** KaTeX's HTML output needs only §3, `@font-face` in sheets,
  WOFF2 on Linux and SVG paths.
