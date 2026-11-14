// Builds the Chromium reference pages for the Markdown fidelity fixtures.
//
// Each fixtures/<id>.md goes through react-markdown (remark-parse,
// remark-gfm, remark-rehype, rehype-raw) and React's static renderer, the
// pipeline a React app uses to show Markdown. The HTML is wrapped in
// <article class="markdown-body"> with github-markdown-css and the page
// style its readme recommends, once per theme.
//
//     cd reference && npm ci && node build.mjs
//
// The pages are committed; harness/compare.py renders them in pinned
// Chromium and captures the tree that markdown-render rebuilds with Div.

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeRaw from "rehype-raw";
import rehypeSanitize from "rehype-sanitize";
import { bundledLanguages, bundledThemes, createHighlighter } from "shiki";
import { createOnigurumaEngine } from "shiki/engine/oniguruma";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const fixtures = join(root, "fixtures");
const pages = join(here, "pages");
const modules = join(here, "node_modules");
const cssPackage = join(modules, "github-markdown-css");
// Read from node_modules directly: some packages do not export package.json.
const versions = Object.fromEntries(
  ["react-markdown", "remark-gfm", "rehype-raw", "rehype-sanitize", "react", "react-dom", "github-markdown-css"].map(
    (name) => [name, JSON.parse(readFileSync(join(modules, name, "package.json"), "utf8")).version],
  ),
);

const THEMES = ["light", "dark"];

// The page style github-markdown-css's readme recommends, verbatim in
// substance: GitHub's 980px column with 45px padding (15px under 768px).
const PAGE_CSS = `.markdown-body {
  box-sizing: border-box;
  min-width: 200px;
  max-width: 980px;
  margin: 0 auto;
  padding: 45px;
}

@media (max-width: 767px) {
  .markdown-body {
    padding: 15px;
  }
}
`;

// React 19 emits <link rel="preload" as="image"> for each <img>. In an app
// React hoists them into <head>; a static render leaves them in the
// fragment, so they are dropped here.
const PRELOAD = /<link rel="preload" as="image" href="[^"]*"\/>/g;

// `sanitize` adds rehype-sanitize with its default schema (GitHub's) after
// rehype-raw, as for untrusted Markdown. The pages use the default.
function render(source, sanitize = false) {
  const rehypePlugins = sanitize ? [rehypeRaw, rehypeSanitize] : [rehypeRaw];
  return renderToStaticMarkup(
    createElement(Markdown, { remarkPlugins: [remarkGfm], rehypePlugins }, source),
  ).replace(PRELOAD, "");
}

function page(id, theme, body) {
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>${id} (${theme})</title>
<link rel="stylesheet" href="css/github-markdown-${theme}.css">
<link rel="stylesheet" href="css/page.css">
</head>
<body>
<article class="markdown-body">
${body}
</article>
</body>
</html>
`;
}

rmSync(pages, { recursive: true, force: true });
mkdirSync(join(pages, "css"), { recursive: true });
mkdirSync(join(pages, "assets"), { recursive: true });
for (const theme of THEMES) {
  copyFileSync(
    join(cssPackage, `github-markdown-${theme}.css`),
    join(pages, "css", `github-markdown-${theme}.css`),
  );
}
writeFileSync(join(pages, "css", "page.css"), PAGE_CSS);
for (const asset of readdirSync(join(fixtures, "assets"))) {
  copyFileSync(join(fixtures, "assets", asset), join(pages, "assets", asset));
}

const sha256 = (text) => createHash("sha256").update(text).digest("hex");
const manifest = { schema: "sample-markdown-reference-v1", versions, fixtures: [] };
for (const file of readdirSync(fixtures).filter((name) => name.endsWith(".md")).sort()) {
  const id = file.replace(/\.md$/, "");
  const source = readFileSync(join(fixtures, file), "utf8").replace(/\r\n/g, "\n");
  const html = render(source);
  for (const theme of THEMES) {
    const name = `${id}-${theme}.html`;
    writeFileSync(join(pages, name), page(id, theme, html));
    manifest.fixtures.push({
      id: `${id}-${theme}`,
      source: `fixtures/${file}`,
      source_sha256: sha256(source),
      theme,
      page: name,
    });
  }
}
writeFileSync(join(pages, "manifest.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`wrote ${manifest.fixtures.length} pages`);

// Corpora for the Rust port's tree test (crates/markdown-tree): the
// fixtures, the CommonMark spec's examples and this repository's GFM cases
// (fixtures/gfm-cases.json), through the same pipeline, without and with
// rehype-sanitize. The expected HTML is react-markdown's output, not the
// spec's, since remark-gfm and rehype-raw are on.
function writeCorpus(name, extraVersions, cases) {
  const examples = cases.map(({ number, section, markdown }) => {
    const example = { number, section, markdown };
    for (const [key, sanitize] of [["html", false], ["html_sanitized", true]]) {
      try {
        example[key] = render(markdown, sanitize);
      } catch (error) {
        example[`${key}_error`] = String(error.message || error).split("\n")[0];
      }
    }
    return example;
  });
  mkdirSync(join(here, "corpus"), { recursive: true });
  writeFileSync(
    join(here, "corpus", `${name}.json`),
    JSON.stringify(
      { schema: "sample-markdown-corpus-v1", versions: { ...versions, ...extraVersions }, examples },
      null,
      1,
    ) + "\n",
  );
  console.log(`wrote ${examples.length} ${name} examples`);
}

// The spec writes tabs as "→".
const require = createRequire(import.meta.url);
const spec = require("commonmark-spec");
const specVersion = JSON.parse(
  readFileSync(join(modules, "commonmark-spec", "package.json"), "utf8"),
).version;
writeCorpus(
  "commonmark",
  { "commonmark-spec": specVersion },
  spec.tests.map((test) => ({
    number: test.number,
    section: test.section,
    markdown: test.markdown.replace(/→/g, "\t"),
  })),
);
writeCorpus(
  "fixtures",
  {},
  readdirSync(fixtures)
    .filter((name) => name.endsWith(".md"))
    .sort()
    .map((file, index) => ({
      number: index + 1,
      section: file.replace(/\.md$/, ""),
      markdown: readFileSync(join(fixtures, file), "utf8").replace(/\r\n/g, "\n"),
    })),
);
writeCorpus(
  "gfm",
  {},
  JSON.parse(readFileSync(join(fixtures, "gfm-cases.json"), "utf8")).map((test, index) => ({
    number: index + 1,
    section: test.name,
    markdown: test.markdown,
  })),
);

// Highlighting (crates/markdown-highlight): Shiki's grammars and themes,
// copied verbatim for the Rust port, and the tokens Shiki makes from them.
// The languages are loaded in Shiki's order: each bundled language module
// lists its embedded languages first, and the list is deduplicated.
const HIGHLIGHT_LANGS = [
  "rust", "shellscript", "javascript", "typescript", "json", "python",
  "toml", "yaml", "css", "diff", "html",
];
const HIGHLIGHT_THEMES = ["github-light", "github-dark"];
const highlightData = join(root, "crates", "markdown-highlight", "data");
const langs = [
  ...new Set((await Promise.all(HIGHLIGHT_LANGS.map(async (id) => (await bundledLanguages[id]()).default))).flat()),
];
const themes = await Promise.all(HIGHLIGHT_THEMES.map(async (id) => (await bundledThemes[id]()).default));
for (const dir of ["langs", "themes"]) {
  rmSync(join(highlightData, dir), { recursive: true, force: true });
  mkdirSync(join(highlightData, dir), { recursive: true });
}
for (const lang of langs) {
  writeFileSync(join(highlightData, "langs", `${lang.name}.json`), JSON.stringify(lang) + "\n");
}
for (const theme of themes) {
  writeFileSync(join(highlightData, "themes", `${theme.name}.json`), JSON.stringify(theme) + "\n");
}
const shikiVersions = Object.fromEntries(
  ["shiki", "@shikijs/langs", "@shikijs/themes", "@shikijs/vscode-textmate", "@shikijs/engine-oniguruma"].map(
    (name) => [name, JSON.parse(readFileSync(join(modules, name, "package.json"), "utf8")).version],
  ),
);
writeFileSync(
  join(highlightData, "manifest.json"),
  JSON.stringify(
    {
      schema: "sample-markdown-highlight-data-v1",
      versions: shikiVersions,
      langs: langs.map((lang) => lang.name),
      themes: themes.map((theme) => theme.name),
    },
    null,
    2,
  ) + "\n",
);

// The token corpus: fixtures/shiki-cases.json and the fixtures' code
// blocks (their text as react-markdown renders it, the input a highlighter
// plugin receives; no language is Shiki's plain "text"), through
// codeToTokensBase with the Oniguruma engine, once per theme.
const highlighter = await createHighlighter({
  themes,
  langs,
  engine: createOnigurumaEngine(import("shiki/wasm")),
});
const unescapeHtml = (text) =>
  text
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#x27;/g, "'")
    .replace(/&amp;/g, "&");
const highlightCases = JSON.parse(readFileSync(join(fixtures, "shiki-cases.json"), "utf8")).map((test) => ({
  ...test,
  source: "fixtures/shiki-cases.json",
}));
for (const file of readdirSync(fixtures).filter((name) => name.endsWith(".md")).sort()) {
  const html = render(readFileSync(join(fixtures, file), "utf8").replace(/\r\n/g, "\n"));
  let index = 0;
  for (const [, lang, code] of html.matchAll(/<pre><code(?: class="language-([^"]+)")?>([\s\S]*?)<\/code><\/pre>/g)) {
    index += 1;
    highlightCases.push({
      name: `${file} block ${index}`,
      lang: lang || "text",
      code: unescapeHtml(code),
      source: `fixtures/${file}`,
    });
  }
}
const highlightCorpus = {
  schema: "sample-markdown-shiki-corpus-v1",
  versions: shikiVersions,
  themes: Object.fromEntries(
    HIGHLIGHT_THEMES.map((id) => {
      const theme = highlighter.getTheme(id);
      return [id, { fg: theme.fg, bg: theme.bg }];
    }),
  ),
  cases: highlightCases.map((test) => ({
    ...test,
    tokens: Object.fromEntries(
      HIGHLIGHT_THEMES.map((theme) => [theme, highlighter.codeToTokensBase(test.code, { lang: test.lang, theme })]),
    ),
  })),
};
writeFileSync(join(here, "corpus", "shiki.json"), JSON.stringify(highlightCorpus, null, 1) + "\n");
console.log(`wrote ${langs.length} grammars, ${themes.length} themes, ${highlightCases.length} highlighting cases`);
highlighter.dispose();
