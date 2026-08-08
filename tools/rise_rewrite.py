#!/usr/bin/env python3
"""Ersetzt reine Auftritts-Animationen von framer-motion durch <Rise>.

Erfasst wird ausschliesslich das Muster, das im Dashboard 130 Mal steht:

    <motion.div initial={{ opacity: 0, y: 20 }} animate={{ opacity: 1, y: 0 }}
                transition={{ delay: 0.15 }} className="...">

Alles andere bleibt unangetastet — insbesondere jedes Element mit `exit`,
`whileHover`, `layout`, `variants` oder einem Wert, der nicht konstant ist.
Solche Faelle brauchen framer-motion weiterhin.

Aufruf: python3 tools/rise_rewrite.py <datei> [...]
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Ausser div stehen im Dashboard section, aside, li, tr und span. Das Element
# bleibt erhalten (`as`-Prop), nur die Bewegung wechselt den Motor.
TAG_NAMES = ("div", "section", "aside", "li", "tr", "span")

# initial={{ opacity: 0, y: 20 }} — auch mit x, auch ohne opacity.
INITIAL = re.compile(
    r"initial=\{\{\s*(?:opacity:\s*0\s*,\s*)?([xy]):\s*-?\d+\s*(?:,\s*opacity:\s*0\s*)?\}\}"
)
ANIMATE = re.compile(
    r"animate=\{\{\s*(?:opacity:\s*1\s*,\s*)?[xy]:\s*0\s*(?:,\s*opacity:\s*1\s*)?\}\}"
)
# transition={{ delay: 0.15 }} / {{ duration: 0.24 }} / {{ duration: 0.24, delay: 0.06 }}
# Die Dauer faellt weg: `.rise-in` normiert alle Auftritte auf 260ms. Uebernommen
# wird nur, was gestaffelt ist. Laengere Dauern als 0.35s bleiben liegen — dort
# ist die Bewegung Absicht und keine Standard-Einblendung.
DELAY = re.compile(
    r"transition=\{\{\s*(?:duration:\s*(?P<dur>0?\.\d+)\s*)?"
    r"(?:,?\s*delay:\s*(?P<delay>[\d.]+)\s*)?\}\}"
)
TRANSITION_ANY = re.compile(r"transition=\{\{")
MAX_DURATION = 0.35

# Props, die ohne framer-motion nicht funktionieren.
BLOCKERS = ("exit=", "whileHover", "whileTap", "whileInView", "variants=", "layout", "drag")


def find_tag_end(src: str, start: int) -> int:
    """Index hinter dem `>` des Oeffnungs-Tags, das bei `start` beginnt."""
    depth = 0
    i = start
    while i < len(src):
        ch = src[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
        elif ch == '"' and depth == 0:
            i = src.index('"', i + 1)
        elif ch == ">" and depth == 0:
            return i
        i += 1
    raise ValueError("Oeffnungs-Tag ohne Ende")


def find_matching_close(src: str, after_open: int, open_tag: str, close_tag: str) -> int:
    """Index des schliessenden Tags, der zum Oeffner bei `after_open` gehoert.

    Gleichnamige verschachtelte Elemente zaehlen mit, selbstschliessende nicht —
    die haben kein Gegenstueck und wuerden die Zaehlung verschieben.
    """
    depth = 1
    i = after_open
    while i < len(src):
        nxt_open = src.find(open_tag, i)
        nxt_close = src.find(close_tag, i)
        if nxt_close == -1:
            raise ValueError(f"Kein schliessendes {open_tag[1:]}")
        if nxt_open != -1 and nxt_open < nxt_close:
            inner_end = find_tag_end(src, nxt_open)
            if not src[nxt_open : inner_end + 1].rstrip().endswith("/>"):
                depth += 1
            i = inner_end + 1
            continue
        depth -= 1
        if depth == 0:
            return nxt_close
        i = nxt_close + len(close_tag)
    raise ValueError(f"Kein schliessendes {open_tag[1:]}")


def rebuild_tag(tag: str, step: str, open_tag: str, element: str) -> str:
    """Baut den Oeffnungs-Tag ohne die abgeloesten Props neu auf.

    Zeilenweise, damit keine leeren Zeilen und keine verwaisten `>` stehen
    bleiben — im Projekt laeuft kein Formatter, der das hinterher glaettet.
    """
    body = tag[len(open_tag) : -1]  # ohne `<motion.xy` und ohne `>`
    self_closing = body.rstrip().endswith("/")
    if self_closing:
        body = body.rstrip()[:-1]

    for pattern in (INITIAL, ANIMATE, DELAY):
        body = pattern.sub("", body)

    raw_lines = body.split("\n")
    # Einrueckung des schliessenden `>` — vor dem rstrip ablesen, sonst ist sie weg.
    indent = re.match(r"[ \t]*", raw_lines[-1]).group(0) if len(raw_lines) > 1 else ""
    lines = [ln.rstrip() for ln in raw_lines]
    kept = [ln for ln in lines if ln.strip()]
    # `div` ist die Vorgabe von <Rise> und braucht kein `as`.
    props_first = [p for p in (step, "" if element == "div" else f'as="{element}"') if p]
    head = f"<Rise {' '.join(props_first)}" if props_first else "<Rise"
    tail = " />" if self_closing else ">"

    if not kept:
        return f"{head}{tail}"
    if len(kept) == 1 and "\n" not in body:
        return f"{head} {kept[0].strip()}{tail}"
    # Mehrzeiliger Tag: die neuen Props gehoeren in die Liste, nicht hinter den Namen.
    for prop in reversed(props_first):
        kept.insert(0, f"{indent}  {prop}")
    props = "\n".join(ln if ln.startswith((" ", "\t")) else f"{indent}  {ln}" for ln in kept)
    return f"<Rise\n{props}\n{indent}{tail.lstrip()}"


def rewrite(src: str) -> tuple[str, int]:
    total = 0
    for element in TAG_NAMES:
        src, n = rewrite_tag(src, element)
        total += n
    return src, total


def rewrite_tag(src: str, element: str) -> tuple[str, int]:
    open_tag = f"<motion.{element}"
    close_tag = f"</motion.{element}>"
    count = 0
    pos = 0
    while True:
        start = src.find(open_tag, pos)
        if start == -1:
            break
        # `<motion.div` darf nicht auf `<motion.divider` matchen.
        if src[start + len(open_tag)] not in " \t\n>/":
            pos = start + len(open_tag)
            continue
        tag_end = find_tag_end(src, start)
        tag = src[start : tag_end + 1]

        if tag.rstrip().endswith("/>") or not INITIAL.search(tag) or not ANIMATE.search(tag):
            pos = tag_end + 1
            continue
        if any(b in tag for b in BLOCKERS):
            pos = tag_end + 1
            continue

        delay = DELAY.search(tag)
        if delay and delay.group("dur") and float(delay.group("dur")) > MAX_DURATION:
            # Bewusst langsame Bewegung — die gehoert nicht in den Standardauftritt.
            pos = tag_end + 1
            continue
        if TRANSITION_ANY.search(DELAY.sub("", tag)):
            # Feder, Kurve oder gerechneter Delay — nicht mechanisch uebertragbar.
            pos = tag_end + 1
            continue

        close = find_matching_close(src, tag_end + 1, open_tag, close_tag)

        seconds = delay.group("delay") if delay else None
        step = f"step={{{{ seconds: {seconds} }}}}" if seconds else ""
        new_tag = rebuild_tag(tag, step, open_tag, element)

        src = src[:close] + "</Rise>" + src[close + len(close_tag) :]
        src = src[:start] + new_tag + src[tag_end + 1 :]
        count += 1
        pos = start + len(new_tag)
    return src, count


def ensure_import(src: str) -> str:
    if "from '../motion/Rise'" in src or "from '../../motion/Rise'" in src:
        return src
    return src


def main() -> int:
    total = 0
    for arg in sys.argv[1:]:
        path = Path(arg)
        src = path.read_text(encoding="utf-8")
        out, n = rewrite(src)
        if n:
            path.write_text(ensure_import(out), encoding="utf-8")
            print(f"{path}: {n}")
            total += n
    print(f"gesamt: {total}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
