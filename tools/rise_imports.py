#!/usr/bin/env python3
"""Zieht die Import-Zeilen hinter dem <Rise>-Umbau nach.

Fuegt `import { Rise } from '<relativ>/motion/Rise';` ein, wo `<Rise` steht,
und entfernt den framer-motion-Import wieder, wo kein `motion.`, `AnimatePresence`
oder Hook der Bibliothek mehr uebrig ist.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / "bot" / "dashboard_v2" / "src"
FRAMER_IMPORT = re.compile(r"^import\s*\{([^}]*)\}\s*from\s*'framer-motion';\s*\n", re.M)
# Alles, was framer-motion weiterhin braucht.
STILL_USED = re.compile(r"\bmotion\.|<AnimatePresence|\buse(Spring|Scroll|Transform|InView|Reduced|Animate|MotionValue)")


def rel_import(path: Path) -> str:
    depth = len(path.relative_to(SRC).parts) - 1
    prefix = "../" * depth if depth else "./"
    return f"import {{ Rise }} from '{prefix}motion/Rise';\n"


def fix(path: Path) -> bool:
    src = path.read_text(encoding="utf-8")
    if "<Rise" not in src:
        return False
    changed = False

    if "motion/Rise'" not in src:
        line = rel_import(path)
        m = FRAMER_IMPORT.search(src)
        if m:
            src = src[: m.end()] + line + src[m.end() :]
        else:
            # Hinter den letzten Import am Dateikopf.
            imports = list(re.finditer(r"^import .*?;\s*\n", src, re.M))
            at = imports[-1].end() if imports else 0
            src = src[:at] + line + src[at:]
        changed = True

    m = FRAMER_IMPORT.search(src)
    if m:
        names = [n.strip() for n in m.group(1).split(",") if n.strip()]
        body = src[: m.start()] + src[m.end() :]
        if not STILL_USED.search(body):
            src = body
            changed = True
        else:
            keep = [n for n in names if re.search(rf"\b{re.escape(n)}\b", body)]
            if keep != names:
                src = (
                    src[: m.start()]
                    + f"import {{ {', '.join(keep)} }} from 'framer-motion';\n"
                    + src[m.end() :]
                )
                changed = True

    if changed:
        path.write_text(src, encoding="utf-8")
    return changed


def main() -> int:
    n = 0
    for path in sorted(SRC.rglob("*.tsx")):
        if path.name == "Rise.tsx":
            continue
        if fix(path):
            print(path.relative_to(SRC))
            n += 1
    print(f"angepasst: {n}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
