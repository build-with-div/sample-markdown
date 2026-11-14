# Shiki's grammars and themes

`reference/build.mjs` writes these files from `@shikijs/langs` and
`@shikijs/themes` 4.6.0 (MIT, Shiki): each language registration and
theme object as JSON, keys in their order, nothing changed. Do not edit
them; rerun `node build.mjs` in `reference/` after a version change.

`manifest.json` lists the languages in the order Shiki loads them (each
bundled language module lists its embedded languages first, deduplicated)
and the package versions. `src/data.rs` must list them in the same order;
`tests/shiki.rs` checks it.

The grammars and themes come from their upstream projects through Shiki's
tm-grammars and tm-themes (https://github.com/shikijs/textmate-grammars-themes),
which record each one's source repository and license.
