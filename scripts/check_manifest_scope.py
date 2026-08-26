#!/usr/bin/env python3
"""Prueft, dass Scan-Konfiguration und tatsaechliche Manifeste deckungsgleich sind.

Hintergrund: Am 26.08.2026 lagen 17 offene Dependabot-Alerts zu einer laengst
geloeschten `uv.lock` im Repo, und `.github/eslint-security/` erzeugte weiter
npm-Alerts, obwohl der Workflow, der den Ordner benutzt hat, in 1e8948a1
entfernt wurde. Beide Faelle sind Drift zwischen Konfiguration und Realitaet.

Der Check laeuft ohne Fremdbibliotheken (kein PyYAML im CI-Image) und meldet:
  * npm-Verzeichnisse in dependabot.yml ohne package.json im Baum,
  * package.json im Baum ohne Eintrag in dependabot.yml,
  * npm-Projekte, die weder Frontend-CI baut noch der Rust-Server ausliefert.

Exit 1 bei jedem Befund, sonst 0.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEPENDABOT = REPO_ROOT / ".github" / "dependabot.yml"
FRONTEND_CI = REPO_ROOT / ".github" / "workflows" / "lint-and-typecheck.yml"

# Verzeichnisse, die kein eigenes Projekt sind, sondern Build- oder Fremdstand.
SKIP_PARTS = {"node_modules", "dist", "dist-preview", ".git", ".tasks"}


def npm_dirs_in_dependabot() -> set[str]:
    """Liest die `directory:`-Werte aller npm-Eintraege aus dependabot.yml."""
    text = DEPENDABOT.read_text(encoding="utf-8")
    dirs: set[str] = set()
    ecosystem: str | None = None
    for line in text.splitlines():
        eco = re.match(r'\s*-?\s*package-ecosystem:\s*"([^"]+)"', line)
        if eco:
            ecosystem = eco.group(1)
            continue
        directory = re.match(r'\s*directory:\s*"([^"]+)"', line)
        if directory and ecosystem == "npm":
            dirs.add(directory.group(1).strip("/"))
    return dirs


def npm_dirs_in_tree() -> set[str]:
    """Findet alle package.json im Arbeitsbaum ausserhalb der Build-Ordner."""
    dirs: set[str] = set()
    for manifest in REPO_ROOT.rglob("package.json"):
        rel = manifest.relative_to(REPO_ROOT)
        if SKIP_PARTS & set(rel.parts):
            continue
        dirs.add(str(rel.parent).strip("."))
    return {d for d in dirs if d}


def npm_dirs_in_frontend_ci() -> set[str]:
    """Liest die `path:`-Werte der Frontend-CI-Matrix."""
    text = FRONTEND_CI.read_text(encoding="utf-8")
    return {m.group(1).strip("/") for m in re.finditer(r"\s*path:\s*(\S+)", text)}


def main() -> int:
    configured = npm_dirs_in_dependabot()
    present = npm_dirs_in_tree()
    built = npm_dirs_in_frontend_ci()

    problems: list[str] = []

    for missing in sorted(configured - present):
        problems.append(
            f"dependabot.yml ueberwacht /{missing}, dort liegt aber keine package.json. "
            "Eintrag entfernen oder Ordner wiederherstellen."
        )

    for unwatched in sorted(present - configured):
        problems.append(
            f"/{unwatched}/package.json ist in dependabot.yml nicht eingetragen. "
            "Eintrag ergaenzen oder Ordner loeschen."
        )

    for orphan in sorted(present - built):
        problems.append(
            f"/{orphan} wird von keiner Frontend-CI-Matrix gebaut. "
            "Entweder in lint-and-typecheck.yml aufnehmen oder als tot loeschen."
        )

    if problems:
        print("Manifest-Scope stimmt nicht:", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    print(f"Manifest-Scope in Ordnung: {len(present)} npm-Projekte, alle ueberwacht und gebaut.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
