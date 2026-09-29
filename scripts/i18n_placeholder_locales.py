#!/usr/bin/env python3
"""Replace every locale string with a tiny placeholder, in place.

This is a measurement harness, not a tool to run on a branch you intend to
keep: it rewrites `ultros-frontend/ultros-i18n/locales/*.json` destructively.
Restore with `git checkout -- ultros-frontend/ultros-i18n/locales` afterwards.

Why it exists
-------------
Asking "how many bytes of the shipped bundle are the seven translated
languages?" cannot be answered by deleting locales — the `Locale` enum is
matched exhaustively across the app, so a build with fewer locales does not
compile, and one with fewer *keys* changes which code leptos_i18n generates.

This keeps the key set, the nesting and every `{{variable}}` marker exactly
where it was, and replaces only the surrounding literal text with a short
token that is unique per locale (unique so that identical strings are not
folded together by the compiler, which would understate the real cost). The
generated module therefore keeps the same shape — same accessors, same
per-locale match arms, same interpolation builders — and the size difference
between a build of this tree and a build of the real one is the cost of the
translated text itself.

Caveat: leptos_i18n de-duplicates identical string segments within a locale,
and real translations repeat far more than unique placeholders do (en: 2270
stored segments versus 2705 here). The placeholder build therefore carries a
few hundred extra slots, which makes the measured delta a slight
*under*-estimate of what the text costs.

See docs/i18n-locale-bundle-size.md for the numbers this produced.
"""

from __future__ import annotations

import json
import os
import re
import sys

INTERPOLATION = re.compile(r"(\{\{[^}]*\}\})")

DEFAULT_DIR = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "ultros-frontend",
    "ultros-i18n",
    "locales",
)


def convert(node, prefix: str, counter: list[int]):
    """Rebuild `node` with every literal text run replaced by a placeholder."""
    if isinstance(node, dict):
        return {key: convert(value, prefix, counter) for key, value in node.items()}
    if isinstance(node, list):
        return [convert(value, prefix, counter) for value in node]
    if not isinstance(node, str):
        return node

    pieces = []
    for part in INTERPOLATION.split(node):
        if not part:
            continue
        if INTERPOLATION.fullmatch(part):
            pieces.append(part)
        else:
            counter[0] += 1
            pieces.append(f"{prefix}{counter[0]}")
    if not pieces:
        counter[0] += 1
        pieces.append(f"{prefix}{counter[0]}")
    return "".join(pieces)


def main(directory: str) -> int:
    names = sorted(name for name in os.listdir(directory) if name.endswith(".json"))
    if not names:
        print(f"no locale files in {directory}", file=sys.stderr)
        return 1
    for name in names:
        path = os.path.join(directory, name)
        with open(path, encoding="utf-8") as handle:
            data = json.load(handle)
        converted = convert(data, f"{name[:-5]}-", [0])
        with open(path, "w", encoding="utf-8") as handle:
            json.dump(converted, handle, ensure_ascii=False, indent=2)
        print(f"{name}: {os.path.getsize(path)} bytes")
    print("\nrestore with: git checkout -- ultros-frontend/ultros-i18n/locales")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else DEFAULT_DIR))
