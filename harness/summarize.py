#!/usr/bin/env python3
"""Summarize harness/compare.py results as Markdown, and make side-by-side
screenshots (Chromium | Div semantic | Div computed) for the docs.

    python3 harness/summarize.py out/fidelity/results.json \
        --markdown docs/fidelity-results.md --screenshots docs/screenshots
"""

from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path

from PIL import Image, ImageDraw

SHOTS = ["readme-light", "readme-dark", "lists-light", "table-light", "code-light",
         "tasklist-light", "html-light", "image-light"]
# Chromium lists both the physical and the logical longhand; in horizontal
# left-to-right text the logical ones duplicate the physical ones.
LOGICAL = ("-block-", "-inline-", "inline-size", "block-size")


def pct(exact: int, total: int) -> str:
    return f"{exact}/{total}" if total else "-"


def fixture_table(results: dict) -> list[str]:
    lines = ["| Fixture | Size | Semantic 1x | Semantic 2x | Computed 1x | Computed 2x | Blocks exact (computed 1x / 2x) | Boxes exact (computed) |",
             "| --- | --- | --- | --- | --- | --- | --- | --- |"]
    for row in results["fixtures"]:
        cells = [row["fixture"], "{}x{}".format(*row["viewport"])]
        blocks = []
        for mode in ("semantic", "computed"):
            for profile, prow in row["profiles"].items():
                r = prow.get(mode, {})
                if r.get("status") == "exact":
                    cells.append("exact")
                elif r.get("status") == "different":
                    cells.append(f"{r['mismatched_pixels']:,} px")
                else:
                    cells.append(r.get("status") or prow.get("error", "?"))
                if mode == "computed":
                    b = r.get("blocks", [])
                    blocks.append(pct(sum(x["status"] == "exact" for x in b), len(b)))
        geometry = row["geometry"].get("computed", {})
        cells.append(" / ".join(blocks))
        cells.append(pct(geometry.get("exact", 0), geometry.get("total", 0)))
        lines.append("| " + " | ".join(cells) + " |")
    return lines


def tag_tables(results: dict) -> list[str]:
    boxes: dict = defaultdict(lambda: [0, 0])
    blocks: dict = defaultdict(lambda: [0, 0, 0, 0])
    for row in results["fixtures"]:
        for tag, tally in row["geometry"].get("computed", {}).get("per_tag", {}).items():
            boxes[tag][0] += tally["exact"]
            boxes[tag][1] += tally["total"]
        for profile, prow in row["profiles"].items():
            offset = 0 if profile.endswith("@1x") else 2
            for block in prow.get("computed", {}).get("blocks", []):
                blocks[block["tag"]][offset] += block["status"] == "exact"
                blocks[block["tag"]][offset + 1] += 1
    lines = ["| Element | Boxes within 1/128 px | Top-level blocks pixel-exact at 1x | at 2x |",
             "| --- | --- | --- | --- |"]
    for tag in sorted(set(boxes) | set(blocks)):
        b = blocks.get(tag, [0, 0, 0, 0])
        lines.append(f"| `{tag}` | {pct(*boxes.get(tag, [0, 0]))} | {pct(b[0], b[1])} | {pct(b[2], b[3])} |")
    total = [sum(v[0] for v in boxes.values()), sum(v[1] for v in boxes.values())]
    lines.append(f"| all | {pct(*total)} | {pct(sum(v[0] for v in blocks.values()), sum(v[1] for v in blocks.values()))} | "
                 f"{pct(sum(v[2] for v in blocks.values()), sum(v[3] for v in blocks.values()))} |")
    return lines


def gap_lists(results: dict) -> list[str]:
    tags, rejected, normalized, unknown, logical = Counter(), {}, Counter(), Counter(), Counter()
    images = []
    for row in results["fixtures"]:
        report = row.get("build_report", {})
        for entry in report.get("unknown_tags", []):
            tags[(entry["tag"], entry["stand_in"])] += entry["count"]
        for entry in report.get("rejected_values", []) + report.get("refused_values", []):
            key = (entry["property"], entry["value"])
            rejected.setdefault(key, [entry["error"], 0])[1] += entry["count"]
        for entry in report.get("normalized_values", []):
            normalized[(entry["property"], entry["value"])] += entry["count"]
        for name, count in report.get("unknown_properties", {}).items():
            (logical if any(part in name for part in LOGICAL) else unknown)[name] += count
        images += report.get("image_errors", [])
    pseudo = Counter()
    for row in results["fixtures"]:
        for key, count in row.get("pseudo_elements", {}).items():
            element, content = key.split(" ", 1)
            tag, which = element.split("::", 1)
            pseudo[(tag, "::" + which, content)] += count
    lines = ["### Tags `Element::create` rejects", "",
             "| Tag | Stand-in used | Elements |", "| --- | --- | --- |"]
    lines += [f"| `{t}` | `{s}` | {n} |" for (t, s), n in sorted(tags.items())]
    lines += ["", "### Computed values `parse_literal` or `set_property` rejects", "",
              "| Property | Value | Error | Declarations |", "| --- | --- | --- | --- |"]
    lines += [f"| `{p}` | `{v}` | {e} | {n} |" for (p, v), (e, n) in sorted(rejected.items())]
    lines += ["", "### Values accepted only after rewriting `Npx` to `N`", "",
              "| Property | Value | Declarations |", "| --- | --- | --- |"]
    lines += [f"| `{p}` | `{v}` | {n} |" for (p, v), n in sorted(normalized.items())]
    lines += ["", "### Properties missing from Div's style schema", "",
              "Non-initial computed values whose property Div does not know (logical "
              "longhands listed separately, since the physical ones carry the same value here).", "",
              "| Property | Declarations |", "| --- | --- |"]
    lines += [f"| `{p}` | {n} |" for p, n in sorted(unknown.items())]
    lines += ["", "Logical longhands: " + ", ".join(f"`{p}`" for p in sorted(logical)) + ".", ""]
    lines += ["### Pseudo-elements Chromium generates (not copied in either mode)", "",
              "| Element | Pseudo-element | content | Count |", "| --- | --- | --- | --- |"]
    lines += [f"| `{t}` | `{w}` | `{c}` | {n} |" for (t, w, c), n in sorted(pseudo.items())]
    if images:
        lines += ["", "Image errors: " + "; ".join(images)]
    return lines


def screenshots(results_dir: Path, output: Path) -> None:
    output.mkdir(parents=True, exist_ok=True)
    for fixture in SHOTS:
        directory = results_dir / fixture
        names = ["chromium@1x.png", "div-semantic@1x.png", "div-computed@1x.png"]
        if not all((directory / n).exists() for n in names):
            continue
        images = [Image.open(directory / n).convert("RGB") for n in names]
        w, h = images[0].size
        label = 24
        sheet = Image.new("RGB", (w * 3 + 16, h + label), (128, 128, 128))
        draw = ImageDraw.Draw(sheet)
        for i, (image, title) in enumerate(zip(images, ["Chromium 147", "Div: semantic", "Div: computed"])):
            sheet.paste(image, (i * (w + 8), label))
            draw.text((i * (w + 8) + 6, 6), title, fill=(255, 255, 255))
        sheet.save(output / f"{fixture}.png", optimize=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("results", type=Path)
    parser.add_argument("--markdown", type=Path, required=True)
    parser.add_argument("--screenshots", type=Path)
    args = parser.parse_args()
    results = json.loads(args.results.read_text())
    results_dir = args.results.parent
    lines = [
        "# Fidelity results",
        "",
        "Generated by `harness/summarize.py` from `harness/compare.py` output ([evidence/fidelity-v1.json](evidence/fidelity-v1.json)). Do not edit.",
        "",
        f"- Chromium {results['chromium']} (Chrome for Testing, Linux, census real-font fontconfig).",
        f"- Div {results['div_commit']}, `markdown-render`, raster configuration `chromium_linux_lcd`, pinned FreeType.",
        "- Reference: " + ", ".join(f"{k} {v}" for k, v in results["reference_versions"].items()) + ".",
        "- Semantic: the fixture's Markdown, parsed by `markdown-tree`, its code blocks "
        "highlighted by `markdown-highlight` with the page's Shiki theme, and mounted by "
        "`markdown-div` (tags, attributes and text only; images without an intrinsic size). "
        "Computed: the captured tree, plus each element's Chromium computed style as typed "
        "declarations.",
        "- Source tree equal to the captured DOM: "
        + pct(sum(1 for row in results["fixtures"]
                  if row.get("source_report", {}).get("tree_equals_capture")),
              len(results["fixtures"])) + " pages.",
        "- Pixels compared exactly (any channel difference counts). "
        "Boxes: `getBoundingClientRect` against `Element::bounding_rect`, tolerance 1/128 px.",
        "",
        "## Per fixture",
        "",
        "Mismatched pixels of the whole viewport (the page's full scroll height).",
        "",
        *fixture_table(results),
        "",
        "## Per element (computed mode)",
        "",
        *tag_tables(results),
        "",
        "## What building the tree ran into",
        "",
        *gap_lists(results),
        "",
    ]
    args.markdown.write_text("\n".join(lines), newline="\n")
    if args.screenshots:
        screenshots(results_dir, args.screenshots)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
