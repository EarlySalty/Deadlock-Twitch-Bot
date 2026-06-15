#!/usr/bin/env python3
"""Extrahiert die Chat-Content-Keyword-Listen exakt per ast aus api_chat_deep.py
und emittiert sie als Rust-Consts (keine Abtipp-Fehler)."""
import ast

SRC = "/home/naniadm/Documents/Deadlock-Twitch-Bot/bot/analytics/api_chat_deep.py"
OUT = "/home/naniadm/Documents/Deadlock-Twitch-Bot/rust/crates/tb-analytics/src/chat_content_lexicon.rs"

SET_TARGETS = [
    "REACTION_TOKENS", "SMALLTALK_TOKENS", "GREETING_TOKENS",
    "POSITIVE_WORDS", "NEGATIVE_WORDS", "SHORT_POSITIVE", "SHORT_NEGATIVE",
]
TUPLE_TARGETS = [
    "BACKSEAT_PHRASES", "SOCIAL_MARKERS", "REACTION_PHRASES",
    "EMOTE_PREFIXES", "EMOTE_SUFFIXES", "GREETING_PHRASES",
    "POSITIVE_PHRASES", "NEGATIVE_PHRASES",
]
ALL = SET_TARGETS + TUPLE_TARGETS + ["DEADLOCK_HEROES", "TOPIC_KEYWORDS"]

tree = ast.parse(open(SRC, encoding="utf-8").read())
vals = {}


def consider(name, value):
    if name not in ALL or value is None:
        return
    v = value
    if isinstance(v, ast.Call) and isinstance(v.func, ast.Name) and v.func.id in ("frozenset", "set"):
        v = v.args[0]
    vals[name] = ast.literal_eval(v)


for node in tree.body:
    if isinstance(node, ast.Assign):
        for t in node.targets:
            if isinstance(t, ast.Name):
                consider(t.id, node.value)
    elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
        consider(node.target.id, node.value)


def rstr(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def emit_strslice(name, items):
    body = ", ".join(rstr(x) for x in items)
    return f"pub const {name}: &[&str] = &[{body}];\n"


lines = [
    "//! AUTO-GENERIERT aus bot/analytics/api_chat_deep.py via rust/scripts/extract_chat_lexicon.py.",
    "//! Keyword-Listen fuer chat-content-analysis (exakte Daten-Paritaet, kein Abtippen).",
    "//! Bei Aenderung der Python-Listen neu generieren.",
    "",
]

# ALIAS_TO_HERO: dict-Reihenfolge, je Hero Aliase nach Laenge absteigend, lowercase.
alias_to_hero = []
for hero, aliases in vals["DEADLOCK_HEROES"].items():
    for alias in sorted(aliases, key=len, reverse=True):
        alias_to_hero.append((alias.lower(), hero))
pairs = ", ".join(f"({rstr(a)}, {rstr(h)})" for a, h in alias_to_hero)
lines.append(f"pub const ALIAS_TO_HERO: &[(&str, &str)] = &[{pairs}];\n")

# TOPIC_KEYWORDS: dict-Reihenfolge erhalten.
topic_entries = []
for topic, kws in vals["TOPIC_KEYWORDS"].items():
    inner = ", ".join(rstr(k) for k in kws)
    topic_entries.append(f"({rstr(topic)}, &[{inner}] as &[&str])")
lines.append("pub const TOPIC_KEYWORDS: &[(&str, &[&str])] = &[\n    " + ",\n    ".join(topic_entries) + ",\n];\n")

# Sets: sortiert (Membership-Reihenfolge egal).
for name in SET_TARGETS:
    lines.append(emit_strslice(name, sorted(vals[name])))
# Tuples: Quell-Reihenfolge.
for name in TUPLE_TARGETS:
    lines.append(emit_strslice(name, list(vals[name])))

open(OUT, "w", encoding="utf-8").write("\n".join(lines) + "\n")
print(f"Geschrieben: {OUT}")
print(f"ALIAS_TO_HERO: {len(alias_to_hero)} Eintraege")
print(f"TOPIC_KEYWORDS: {len(vals['TOPIC_KEYWORDS'])} Kategorien")
for name in SET_TARGETS + TUPLE_TARGETS:
    print(f"{name}: {len(vals[name])}")
