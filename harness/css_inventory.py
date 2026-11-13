#!/usr/bin/env python3
"""Inventory the CSS features a stylesheet uses: selectors, at-rules,
properties, custom properties and functions. Prints JSON.

    python3 harness/css_inventory.py reference/node_modules/github-markdown-css/github-markdown.css
"""
import json
import re
import sys
from collections import Counter


def strip_comments(text):
    return re.sub(r"/\*.*?\*/", "", text, flags=re.S)


def blocks(text):
    """(prelude, body) pairs, recursing into @media and @supports."""
    i, n = 0, len(text)
    while i < n:
        j = text.find("{", i)
        if j < 0:
            return
        prelude = text[i:j].strip()
        depth, k = 1, j + 1
        while k < n and depth:
            depth += {"{": 1, "}": -1}.get(text[k], 0)
            k += 1
        body = text[j + 1:k - 1]
        if prelude.startswith("@media") or prelude.startswith("@supports"):
            yield prelude, None
            yield from blocks(body)
        else:
            yield prelude, body
        i = k


def main(path):
    text = strip_comments(open(path, encoding="utf-8").read())
    at_rules, pseudo_classes, pseudo_elements = Counter(), Counter(), Counter()
    combinators, attributes, properties, custom_defined = Counter(), Counter(), Counter(), Counter()
    functions, var_uses, rules, selectors = Counter(), Counter(), 0, 0
    for prelude, body in blocks(text):
        if body is None or prelude.startswith("@"):
            at_rules[prelude.split("(")[0].split()[0] if prelude.startswith("@") else prelude] += 1
            if prelude.startswith("@media"):
                at_rules[prelude] += 0
                at_rules["media: " + prelude[6:].strip()] += 1
            continue
        rules += 1
        for selector in prelude.split(","):
            selectors += 1
            s = selector.strip()
            for m in re.finditer(r"::?[-\w]+(\()?", s):
                token = m.group(0).rstrip("(")
                (pseudo_elements if token.startswith("::") else pseudo_classes)[token] += 1
            for m in re.finditer(r"\[([-\w]+)([~|^$*]?=)?", s):
                attributes[m.group(1) + (m.group(2) or "")] += 1
            for c in (">", "+", "~"):
                if c in re.sub(r"\[.*?\]|\(.*?\)", "", s):
                    combinators[c] += 1
            if re.search(r"\S\s+[^>+~\s]", re.sub(r"\s*([>+~])\s*", r"\1", re.sub(r"\(.*?\)", "", s))):
                combinators["descendant"] += 1
        for declaration in body.split(";"):
            if ":" not in declaration:
                continue
            name, value = declaration.split(":", 1)
            name = name.strip()
            if name.startswith("--"):
                custom_defined[name] += 1
                continue
            properties[name] += 1
            for m in re.finditer(r"([-\w]+)\(", value):
                functions[m.group(1)] += 1
            for m in re.finditer(r"var\((--[-\w]+)", value):
                var_uses[m.group(1)] += 1
    print(json.dumps({
        "file": path, "rules": rules, "selectors": selectors,
        "at_rules": dict(at_rules), "pseudo_classes": dict(pseudo_classes.most_common()),
        "pseudo_elements": dict(pseudo_elements.most_common()),
        "combinators": dict(combinators), "attribute_selectors": dict(attributes),
        "properties": dict(properties.most_common()),
        "custom_properties_defined": len(custom_defined),
        "custom_properties_used": len(var_uses), "var_uses": sum(var_uses.values()),
        "functions": dict(functions.most_common()),
    }, indent=1))


if __name__ == "__main__":
    main(sys.argv[1])
