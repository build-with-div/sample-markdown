#!/usr/bin/env python3
"""Compare Div's rendering of the Markdown fixtures with pinned Chromium.

For each reference page in reference/pages (react-markdown + remark-gfm
HTML with github-markdown-css):

1. Chromium loads the page and the harness captures a tree of the
   `<body>`: every element's tag, attributes, text, bounding box and computed
   style (the longhands that differ from their initial values, plus
   `display`; `width` and `height` as computed, not used, values), and which
   pseudo-elements generate boxes. The viewport height
   is the page's scroll height, so the whole document is compared.
2. Chromium rasterizes the page at each profile, in the same session
   (two identical frames, as Div's census capture requires).
3. `markdown-render` builds the page with Div's public API twice:
   `semantic` from the fixture's Markdown (`markdown-tree` parses it and
   `markdown-div` mounts tags, attributes and text, which is all Div can
   take today without a cascade; the report checks that this tree equals
   the captured one) and `computed` from the capture (each element also
   gets Chromium's computed style as typed declarations, which removes the
   cascade from the comparison).
4. Pixels are compared exactly: the whole page, and the box of each
   top-level block of the article. Element boxes are compared within
   1/128 px (half a LayoutUnit).

    python3 harness/compare.py --div ~/div --renderer ~/sample-markdown-target/release/markdown-render

Run it in Linux (WSL) with Chrome for Testing 147.0.7727.50 and a Div
checkout at the pinned commit, which provides the census font environment
(tools/accountability/data/fonts) and the CDP helpers.
"""

from __future__ import annotations

import argparse
import base64
import concurrent.futures
import hashlib
import json
import math
import mimetypes
import os
import subprocess
import sys
import tempfile
import urllib.parse
import urllib.request
from contextlib import ExitStack, contextmanager
from pathlib import Path

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parents[1]
PAGES = ROOT / "reference/pages"
DEFAULT_CHROME = Path.home() / "openui-oracle/chrome/linux-147.0.7727.50/chrome-linux64/chrome"
EXPECTED_CHROMIUM = "147.0.7727.50"
WIDTH = 800
SCALES = (1.0, 2.0)
MODES = ("semantic", "computed")
GEOMETRY_TOLERANCE = 1 / 128
SCHEMA = "sample-markdown-fidelity-v1"

pixel_runner = None  # Div's tools/accountability/run_all_pixel_comparisons


CAPTURE_SCRIPT = r"""
new Promise(resolve => {
  const LAYOUT_RESULTS = new Set(['width', 'height', 'inline-size', 'block-size',
    'perspective-origin', 'transform-origin']);
  const collect = async () => {
    for (const image of document.images) { try { await image.decode(); } catch (e) {} }
    const body = document.body;
    const all = [body, ...body.querySelectorAll('*')];
    const styles = all.map(element => getComputedStyle(element));
    // Attributes as parsed, before the size probe below rewrites `style`.
    const attributes = all.map(element =>
      Array.from(element.attributes, attribute => [attribute.name, attribute.value]));
    // Authored sizes: getComputedStyle reports the used width and height of
    // a rendered box, but the computed value (auto, 4px, max-content, 100%)
    // of a box with display: none. Read them that way, one element at a time.
    const authored = all.map(element => {
      const saved = element.style.getPropertyValue('display');
      const priority = element.style.getPropertyPriority('display');
      element.style.setProperty('display', 'none', 'important');
      const s = getComputedStyle(element);
      const sizes = {};
      for (const name of ['width', 'height']) sizes[name] = s.getPropertyValue(name);
      element.style.setProperty('display', saved, priority);
      if (!element.getAttribute('style')) element.removeAttribute('style');
      return sizes;
    });
    const records = all.map((element, index) => {
      const r = element.getBoundingClientRect();
      const style = styles[index];
      const values = {};
      for (let i = 0; i < style.length; i++) {
        const name = style[i];
        values[name] = style.getPropertyValue(name);
      }
      const pseudo = {};
      for (const which of ['::before', '::after', '::marker']) {
        const p = getComputedStyle(element, which);
        if (which === '::marker' ? style.display === 'list-item' : p.content !== 'none') {
          pseudo[which] = {content: p.content, display: p.display,
                           'list-style-type': style.listStyleType};
        }
      }
      return {rect: [r.x, r.y, r.width, r.height], values, pseudo};
    });
    // Initial values, read from an element with `all: initial`, after the
    // real boxes were measured.
    const probe = document.createElement('div');
    probe.style.all = 'initial';
    body.appendChild(probe);
    const initialStyle = getComputedStyle(probe);
    const initial = {};
    for (let i = 0; i < initialStyle.length; i++) {
      initial[initialStyle[i]] = initialStyle.getPropertyValue(initialStyle[i]);
    }
    probe.remove();
    let index = 0;
    const walk = element => {
      const record = records[index++];
      const sizes = authored[index - 1];
      const style = {display: record.values.display};
      for (const [name, value] of Object.entries(record.values)) {
        if (value !== initial[name] && !LAYOUT_RESULTS.has(name)) style[name] = value;
      }
      for (const [name, value] of Object.entries(sizes)) {
        if (value !== 'auto') style[name] = value;
      }
      const node = {tag: element.localName, attrs: {}, style, rect: record.rect,
                    children: []};
      for (const [name, value] of attributes[index - 1]) node.attrs[name] = value;
      if (Object.keys(record.pseudo).length) node.pseudo = record.pseudo;
      if (element.localName === 'img') node.image = {src: element.getAttribute('src'),
                                                     url: element.currentSrc,
                                                     natural: [element.naturalWidth, element.naturalHeight]};
      for (const child of element.childNodes) {
        if (child.nodeType === Node.TEXT_NODE) node.children.push({text: child.data});
        else if (child.nodeType === Node.ELEMENT_NODE) node.children.push(walk(child));
      }
      return node;
    };
    resolve({body: walk(body),
             scroll_height: document.documentElement.scrollHeight,
             initial});
  };
  const done = () => document.fonts.ready.then(collect);
  if (document.readyState === 'complete') done();
  else addEventListener('load', done, {once: true});
})
"""


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def load_pixel_runner(div_root: Path) -> None:
    global pixel_runner
    sys.path.insert(0, str(div_root / "tools/accountability"))
    import run_all_pixel_comparisons  # noqa: E402

    pixel_runner = run_all_pixel_comparisons


def chromium_version(chrome: Path) -> str:
    output = subprocess.run([str(chrome), "--version"], capture_output=True, text=True).stdout
    return output.strip().split()[-1] if output.strip() else ""


@contextmanager
def chromium_page(chrome: Path, window=(WIDTH, 600)):
    """A headless pinned Chromium with one page, driven over CDP.

    The window is at least as large as the largest physical surface the
    page is captured at, as Div's census renderer allocates it.
    """
    env = pixel_runner.chrome_environment(str(chrome.parent), use_real_font=True)
    with tempfile.TemporaryDirectory(prefix="sample-markdown-") as profile_dir, ExitStack() as stack:
        process = subprocess.Popen(
            [
                str(chrome), "--headless", "--disable-gpu", "--no-sandbox", "--no-first-run",
                "--no-default-browser-check",
                "--enable-features=FontDataServiceLinux:typeface/Freetype",
                "--remote-debugging-port=0", f"--user-data-dir={profile_dir}",
                f"--window-size={window[0]},{window[1] + 87}", "about:blank",
            ],
            env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True,
        )
        stack.callback(pixel_runner._stop_chromium_process, process)
        port = pixel_runner._wait_for_devtools_endpoint(profile_dir, process, timeout=90)
        client = pixel_runner._CdpWebSocket(pixel_runner._page_websocket_url(port))
        stack.callback(pixel_runner._close_chromium_client, client)
        client.command("Page.enable")
        yield client


def load(client, page: Path, height: int, scale: float = 1.0) -> None:
    client.command("Emulation.setDeviceMetricsOverride", {
        "width": WIDTH, "height": height, "deviceScaleFactor": scale, "mobile": False,
        "screenWidth": WIDTH, "screenHeight": height,
    })
    client.command("Page.navigate", {"url": "file:" + urllib.request.pathname2url(str(page))})


SETTLE_SCRIPT = (
    "new Promise(resolve => {"
    "const done = () => document.fonts.ready.then(() => "
    "requestAnimationFrame(() => requestAnimationFrame(resolve)));"
    "if (document.readyState === 'complete') done();"
    "else addEventListener('load', done, {once:true});"
    "}).then(() => [innerWidth, innerHeight, devicePixelRatio])"
)


def screenshot(client, page: Path, height: int, scale: float, output: Path) -> None:
    """Capture the viewport as Div's census does: two identical frames."""
    load(client, page, height, scale)
    metrics = client.command("Runtime.evaluate", {
        "expression": SETTLE_SCRIPT, "awaitPromise": True, "returnByValue": True,
    }).get("result", {}).get("value")
    if metrics != [WIDTH, height, scale]:
        raise RuntimeError(f"viewport {metrics}, expected {[WIDTH, height, scale]}")
    options = {"format": "png", "fromSurface": True, "captureBeyondViewport": False}
    frames = [client.command("Page.captureScreenshot", options)["data"] for _ in range(2)]
    while frames[-1] != frames[-2]:
        if len(frames) > 6:
            raise RuntimeError(f"{page.name} did not settle")
        frames.append(client.command("Page.captureScreenshot", options)["data"])
    output.write_bytes(base64.b64decode(frames[-1], validate=True))


def capture(client, page: Path, height: int) -> dict:
    load(client, page, height)
    result = client.command(
        "Runtime.evaluate",
        {"expression": CAPTURE_SCRIPT, "awaitPromise": True, "returnByValue": True},
    ).get("result", {}).get("value")
    if not isinstance(result, dict):
        raise RuntimeError(f"capture failed for {page.name}: {result!r}")
    return result


def attach_images(node: dict, page_dir: Path) -> None:
    image = node.get("image")
    if image:
        url = urllib.parse.urlparse(image.get("url") or "")
        if url.scheme == "file":
            path = Path(urllib.request.url2pathname(url.path))
            data = path.read_bytes()
            image.update(file=str(path), sha256=sha256_bytes(data),
                         mime=mimetypes.guess_type(path.name)[0] or "application/octet-stream")
    for child in node["children"]:
        if "tag" in child:
            attach_images(child, page_dir)


def elements(node: dict) -> list[dict]:
    """The tree's elements in depth-first order, as markdown-render lists them."""
    found = [node]
    for child in node["children"]:
        if "tag" in child:
            found.extend(elements(child))
    return found


def compare_images(expected_path: Path, actual_path: Path, box=None) -> dict:
    with Image.open(expected_path) as e, Image.open(actual_path) as a:
        expected, actual = e.convert("RGBA"), a.convert("RGBA")
    if expected.size != actual.size:
        return {"status": "error", "error": f"size {actual.size} != {expected.size}"}
    if box is not None:
        expected, actual = expected.crop(box), actual.crop(box)
    difference = ImageChops.difference(expected, actual)
    mask = difference.split()[0]
    for channel in difference.split()[1:]:
        mask = ImageChops.lighter(mask, channel)
    mismatched = sum(mask.point(lambda v: 255 if v else 0).histogram()[255:])
    result = {"status": "exact" if mismatched == 0 else "different",
              "mismatched_pixels": mismatched,
              "pixels": expected.size[0] * expected.size[1]}
    if mismatched:
        result["max_channel_delta"] = max(c.getextrema()[1] for c in difference.split())
    return result


def write_diff(expected_path: Path, actual_path: Path, output: Path) -> None:
    with Image.open(expected_path) as e, Image.open(actual_path) as a:
        if e.size != a.size:
            return
        difference = ImageChops.difference(e.convert("RGB"), a.convert("RGB"))
    mask = difference.convert("L").point(lambda v: 255 if v else 0)
    Image.composite(Image.new("RGB", mask.size, (255, 0, 0)),
                    Image.new("RGB", mask.size, (255, 255, 255)), mask).save(output)


def run_renderer(renderer: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(renderer), *args],
        env=pixel_runner.openui_environment(use_real_font=True),
        capture_output=True, text=True, timeout=300,
    )


def compare_fixture(fixture: dict, chrome: Path, renderer: Path, results: Path) -> dict:
    page = PAGES / fixture["page"]
    directory = results / fixture["id"]
    directory.mkdir(parents=True, exist_ok=True)
    with chromium_page(chrome) as client:
        first = capture(client, page, 600)
    height = max(1, math.ceil(first["scroll_height"]))
    scale_max = max(SCALES)
    chromium_pngs = {}
    with chromium_page(chrome, (int(WIDTH * scale_max), int(height * scale_max))) as client:
        tree = capture(client, page, height) if height != 600 else first
        for scale in SCALES:
            chromium_pngs[scale] = directory / f"chromium@{scale:g}x.png"
            screenshot(client, page, height, scale, chromium_pngs[scale])
    tree = {"fixture": fixture["id"], "viewport": [WIDTH, height], "body": tree["body"]}
    attach_images(tree["body"], page.parent)
    pseudo: dict = {}
    for node in elements(tree["body"]):
        for which, info in node.get("pseudo", {}).items():
            key = f'{node["tag"]}{which} {info.get("content", "")}'
            pseudo[key] = pseudo.get(key, 0) + 1
    tree_path = directory / "tree.json"
    tree_path.write_text(json.dumps(tree, indent=1) + "\n")
    chromium_boxes = [node["rect"] for node in elements(tree["body"])]

    row: dict = {"fixture": fixture["id"], "viewport": [WIDTH, height], "profiles": {},
                 "pseudo_elements": pseudo}
    source = str(ROOT / fixture["source"])
    mode_args = {"semantic": ["--mode", "semantic", "--source", source],
                 "computed": ["--mode", "computed"]}
    for key, args, name in (("build_report", [], "report.json"),
                            ("source_report", ["--source", source], "source-report.json")):
        report = run_renderer(renderer, "report", str(tree_path), *args)
        if report.returncode == 0:
            row[key] = json.loads(report.stdout)
            (directory / name).write_text(report.stdout)
        else:
            row[key] = {"error": report.stderr.strip()}

    # Top-level blocks of the article, by element index in the tree.
    nodes = elements(tree["body"])
    article = next(i for i, n in enumerate(nodes) if n["tag"] == "article")
    blocks = [(i, n) for i, n in enumerate(nodes)
              if i > article and n in nodes[article]["children"]]

    for scale in SCALES:
        profile = f"{WIDTH}x{height}@{scale:g}x"
        prow: dict = {}
        chromium_png = chromium_pngs[scale]
        for mode in MODES:
            div_png = directory / f"div-{mode}@{scale:g}x.png"
            done = run_renderer(renderer, "render", str(tree_path), str(div_png),
                                *mode_args[mode], "--scale", f"{scale:g}")
            if done.returncode != 0:
                prow[mode] = {"status": "error", "error": done.stderr.strip()[-2000:]}
                continue
            page_result = compare_images(chromium_png, div_png)
            if page_result["status"] == "different":
                write_diff(chromium_png, div_png, directory / f"diff-{mode}@{scale:g}x.png")
            block_results = []
            for index, node in blocks:
                x, y, w, h = node["rect"]
                box = (math.floor(x * scale), math.floor(y * scale),
                       math.ceil((x + w) * scale), math.ceil((y + h) * scale))
                if box[2] <= box[0] or box[3] <= box[1]:
                    continue
                result = compare_images(chromium_png, div_png, box)
                block_results.append({"index": index, "tag": node["tag"],
                                      "status": result["status"],
                                      "mismatched_pixels": result.get("mismatched_pixels")})
            prow[mode] = {**page_result, "blocks": block_results}
        row["profiles"][profile] = prow

    # Geometry at 1x for both modes.
    row["geometry"] = {}
    for mode in MODES:
        done = run_renderer(renderer, "geometry", str(tree_path), *mode_args[mode])
        if done.returncode != 0:
            row["geometry"][mode] = {"error": done.stderr.strip()[-2000:]}
            continue
        div_boxes = json.loads(done.stdout)["boxes"]
        if len(div_boxes) != len(nodes):
            row["geometry"][mode] = {"error": f"{len(div_boxes)} Div elements for "
                                              f"{len(nodes)} captured ones"}
            continue
        per_tag: dict = {}
        differences = []
        for index, (node, expected, actual) in enumerate(zip(nodes, chromium_boxes, div_boxes)):
            tally = per_tag.setdefault(node["tag"], {"exact": 0, "total": 0})
            tally["total"] += 1
            if actual is None:
                if not any(expected):
                    tally["exact"] += 1
                else:
                    differences.append({"index": index, "tag": node["tag"],
                                        "chromium": expected, "div": None})
                continue
            delta = max(abs(a - e) for a, e in zip(actual, expected))
            if delta <= GEOMETRY_TOLERANCE:
                tally["exact"] += 1
            else:
                differences.append({"index": index, "tag": node["tag"], "chromium": expected,
                                    "div": actual, "max_delta": round(delta, 4)})
        row["geometry"][mode] = {"per_tag": per_tag,
                                 "exact": sum(t["exact"] for t in per_tag.values()),
                                 "total": sum(t["total"] for t in per_tag.values()),
                                 "differences": differences}
    return row


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--div", type=Path, required=True,
                        help="a Div checkout at the pinned commit (for the font environment)")
    parser.add_argument("--renderer", type=Path, required=True, help="markdown-render binary")
    parser.add_argument("--chrome", type=Path, default=DEFAULT_CHROME)
    parser.add_argument("--results", type=Path, default=ROOT / "out/fidelity")
    parser.add_argument("--output", type=Path, help="write the results JSON here")
    parser.add_argument("--fixture", action="append", help="only these fixture ids")
    parser.add_argument("--jobs", type=int, default=6)
    args = parser.parse_args(argv)

    load_pixel_runner(args.div.resolve())
    version = chromium_version(args.chrome)
    if version != EXPECTED_CHROMIUM:
        print(f"expected Chromium {EXPECTED_CHROMIUM}, found {version!r}", file=sys.stderr)
        return 2
    manifest = json.loads((PAGES / "manifest.json").read_text())
    fixtures = [f for f in manifest["fixtures"] if not args.fixture or f["id"] in args.fixture]
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        rows = list(pool.map(
            lambda f: compare_fixture(f, args.chrome, args.renderer.resolve(), args.results),
            fixtures))
    div_commit = subprocess.run(["git", "-C", str(args.div), "rev-parse", "HEAD"],
                                capture_output=True, text=True).stdout.strip()
    results = {"schema": SCHEMA, "chromium": version, "div_commit": div_commit,
               "reference_versions": manifest["versions"], "fixtures": rows}
    output = args.output or args.results / "results.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(results, indent=1) + "\n")
    for row in rows:
        cells = []
        for profile, prow in row["profiles"].items():
            for mode in MODES:
                r = prow.get(mode, {})
                cells.append(f"{mode[:4]}@{profile.split('@')[1]}={r.get('status')}"
                             f"({r.get('mismatched_pixels', '-')})")
        boxes = " ".join(f"{mode[:4]}-boxes={g.get('exact')}/{g.get('total')}"
                         for mode, g in row["geometry"].items())
        tree = row["source_report"].get("tree_equals_capture")
        print(f"{row['fixture']:22} {' '.join(cells)} {boxes} source-tree-equal={tree}")
    print(f"wrote {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
