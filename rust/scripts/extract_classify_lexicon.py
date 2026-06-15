#!/usr/bin/env python3
"""Extrahiert die 8 inline-Keyword-Listen aus _classify_message (api_v2.py) per ast
und emittiert sie als Rust-Consts (exakte Daten-Paritaet, kein Abtippen)."""
import ast

SRC = "/home/naniadm/Documents/Deadlock-Twitch-Bot/bot/analytics/api_v2.py"
OUT = "/home/naniadm/Documents/Deadlock-Twitch-Bot/rust/crates/tb-analytics/src/chat_analytics_lexicon.rs"

# Reihenfolge der Branches in _classify_message (Command/empty haben keine Liste).
LABELS = ["HYPE", "GREETING", "QUESTION", "FEEDBACK", "TECHNICAL", "SOCIAL", "REACTION", "GAME"]

tree = ast.parse(open(SRC, encoding="utf-8").read())
func = next(
    (n for n in ast.walk(tree) if isinstance(n, ast.FunctionDef) and n.name == "_classify_message"),
    None,
)
if func is None:
    raise SystemExit("_classify_message nicht gefunden")

lists = []
for stmt in func.body:
    if not isinstance(stmt, ast.If):
        continue
    for sub in ast.walk(stmt.test):
        if (
            isinstance(sub, ast.Call)
            and isinstance(sub.func, ast.Name)
            and sub.func.id == "any"
            and sub.args
            and isinstance(sub.args[0], ast.GeneratorExp)
            and isinstance(sub.args[0].generators[0].iter, ast.List)
        ):
            it = sub.args[0].generators[0].iter
            lists.append([ast.literal_eval(e) for e in it.elts])
            break

if len(lists) != len(LABELS):
    raise SystemExit(f"Erwartet {len(LABELS)} Listen, gefunden {len(lists)}")


def rstr(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


lines = [
    "//! AUTO-GENERIERT aus bot/analytics/api_v2.py:_classify_message via",
    "//! rust/scripts/extract_classify_lexicon.py. Keyword-Listen fuer classify_message",
    "//! (exakte Daten-Paritaet, kein Abtippen). Bei Aenderung neu generieren.",
    "",
]
for label, items in zip(LABELS, lists):
    body = ", ".join(rstr(x) for x in items)
    lines.append(f"pub const {label}: &[&str] = &[{body}];\n")

open(OUT, "w", encoding="utf-8").write("\n".join(lines) + "\n")
print(f"Geschrieben: {OUT}")
for label, items in zip(LABELS, lists):
    print(f"{label}: {len(items)}")
