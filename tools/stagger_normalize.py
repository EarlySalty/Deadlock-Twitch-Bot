#!/usr/bin/env python3
"""Bringt gerechnete Staffelungen auf das Fenster, das der Standard vorgibt.

Uebrig geblieben sind die Stellen, die framer-motion behalten muessen (exit,
whileInView). Ihre Verzoegerung wird trotzdem gerechnet, und zwar zu grosszuegig:
`delay: i * 0.1` heisst bei zehn Eintraegen, dass der letzte nach einer Sekunde
kommt. 40ms pro Stufe, gedeckelt bei 240ms — dieselben Werte wie in
`src/motion/rise.ts`, damit beide Motoren gleich schnell staffeln.

Aufruf: python3 tools/stagger_normalize.py
"""

from __future__ import annotations

import re
from pathlib import Path

SRC = Path(__file__).resolve().parent.parent / "bot" / "dashboard_v2" / "src"

STEP = 0.04
CAP = 0.24
MAX_OFFSET = 0.1

# delay: 0.3 + i * 0.1   |   delay: i * 0.1   |   delay: 0.05 * i
CALC = re.compile(
    r"delay:\s*(?:(?P<offset>[\d.]+)\s*\+\s*)?"
    r"(?:(?P<var1>[A-Za-z_$][\w$]*)\s*\*\s*(?P<f1>[\d.]+)"
    r"|(?P<f2>[\d.]+)\s*\*\s*(?P<var2>[A-Za-z_$][\w$]*))"
)


def fmt(value: float) -> str:
    return f"{value:g}"


def replace(m: re.Match[str]) -> str:
    var = m.group("var1") or m.group("var2")
    factor = float(m.group("f1") or m.group("f2"))
    offset = float(m.group("offset") or 0)

    factor = min(factor, STEP)
    offset = min(offset, MAX_OFFSET)
    term = f"{var} * {fmt(factor)}"
    inner = f"{fmt(offset)} + {term}" if offset else term
    return f"delay: Math.min({inner}, {fmt(CAP)})"


def main() -> int:
    touched = 0
    for path in sorted(SRC.rglob("*.tsx")):
        src = path.read_text(encoding="utf-8")
        # Schon gedeckelte Ausdruecke nicht doppelt einpacken.
        out = CALC.sub(lambda m: m.group(0) if "Math.min" in m.group(0) else replace(m), src)
        out = re.sub(r"delay:\s*Math\.min\(([^,]+),\s*[\d.]+\)", lambda m: normalize_capped(m), out)
        if out != src:
            path.write_text(out, encoding="utf-8")
            print(path.relative_to(SRC))
            touched += 1
    print(f"angepasst: {touched}")
    return 0


def normalize_capped(m: re.Match[str]) -> str:
    """Bestehende `Math.min(...)`-Deckel auf denselben Wert ziehen."""
    inner = m.group(1).strip()
    parts = CALC.match(f"delay: {inner}")
    if not parts:
        return f"delay: Math.min({inner}, {fmt(CAP)})"
    return replace(parts)


if __name__ == "__main__":
    raise SystemExit(main())
