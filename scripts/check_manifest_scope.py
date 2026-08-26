#!/usr/bin/env python3
"""Prueft, dass Scan-Konfiguration und tatsaechliche npm-Projekte deckungsgleich sind.

Hintergrund: `.github/eslint-security/` stand am 26.08.2026 noch in
dependabot.yml und erzeugte npm-Alerts und Update-PRs, obwohl der Workflow,
der den Ordner benutzt hat, in 1e8948a1 entfernt wurde. Dieselbe Drift in die
andere Richtung (Projekt da, Konfiguration fehlt) faellt genauso auf.

Bewusste Grenze: geprueft wird ausschliesslich das npm-Oekosystem, weil nur
dort Verzeichnisse in dependabot.yml gepflegt werden. Cargo laeuft ueber
cargo-audit, und Python gibt es im Repo nicht mehr. Karteileichen in anderen
Oekosystemen (etwa die geloeschte `uv.lock`, deren Alerts am 26.08.2026 offen
blieben) faengt dieser Check nicht, die haengen am Dependency-Graph von GitHub.

Der Check laeuft ohne Fremdbibliotheken (kein PyYAML im CI-Image) und meldet:
  * npm-Verzeichnisse in dependabot.yml ohne package.json im Baum,
  * package.json im Baum ohne Eintrag in dependabot.yml,
  * npm-Projekte, die die Frontend-CI nicht baut.

Exit 1 bei jedem Befund, sonst 0.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEPENDABOT_REL = Path(".github/dependabot.yml")
FRONTEND_CI_REL = Path(".github/workflows/lint-and-typecheck.yml")

# Verzeichnisse, die kein eigenes Projekt sind, sondern Build- oder Fremdstand.
SKIP_PARTS = {"node_modules", "dist", "dist-preview", ".git", ".tasks"}

# Nur eingerueckte `path:`-Schluessel der Matrix, nicht `cache-dependency-path:`
# und andere Schluessel, die auf "path:" enden.
MATRIX_PATH_RE = re.compile(r"^\s+path:\s*(\S+)\s*$", re.MULTILINE)


def normalize(directory: str) -> str:
    """Vereinheitlicht einen Projektpfad relativ zum Repo-Root.

    Das Repo-Root selbst wird zu "". Fuehrende Punkte bleiben erhalten, sonst
    wuerde aus `.github/eslint-security` ein `github/eslint-security` und der
    Check meldete genau den Ordner falsch, fuer den er gebaut ist.
    """
    cleaned = directory.strip()
    while cleaned.startswith("/"):
        cleaned = cleaned[1:]
    while cleaned.endswith("/"):
        cleaned = cleaned[:-1]
    return "" if cleaned == "." else cleaned


def npm_dirs_in_dependabot(config: Path) -> set[str]:
    """Liest die `directory:`-Werte aller npm-Eintraege aus dependabot.yml."""
    dirs: set[str] = set()
    ecosystem: str | None = None
    for line in config.read_text(encoding="utf-8").splitlines():
        eco = re.match(r'\s*-?\s*package-ecosystem:\s*"([^"]+)"', line)
        if eco:
            ecosystem = eco.group(1)
            continue
        directory = re.match(r'\s*directory:\s*"([^"]+)"', line)
        if directory and ecosystem == "npm":
            dirs.add(normalize(directory.group(1)))
    return dirs


def npm_dirs_in_tree(root: Path) -> set[str]:
    """Findet alle package.json im Arbeitsbaum ausserhalb der Build-Ordner."""
    dirs: set[str] = set()
    for manifest in root.rglob("package.json"):
        rel = manifest.relative_to(root)
        if SKIP_PARTS & set(rel.parts):
            continue
        parent = "" if rel.parent == Path(".") else rel.parent.as_posix()
        dirs.add(normalize(parent))
    return dirs


def npm_dirs_in_frontend_ci(workflow: Path) -> set[str]:
    """Liest die `path:`-Werte der Frontend-CI-Matrix."""
    text = workflow.read_text(encoding="utf-8")
    return {normalize(m.group(1)) for m in MATRIX_PATH_RE.finditer(text)}


def label(directory: str) -> str:
    """Schreibweise fuer die Meldung: Root heisst `/`, sonst `/pfad`."""
    return "/" if directory == "" else f"/{directory}"


def find_problems(configured: set[str], present: set[str], built: set[str]) -> list[str]:
    """Baut die Mangelliste aus den drei Mengen."""
    problems: list[str] = []

    for missing in sorted(configured - present):
        problems.append(
            f"dependabot.yml ueberwacht {label(missing)}, dort liegt aber keine package.json. "
            "Eintrag entfernen oder Ordner wiederherstellen."
        )

    for unwatched in sorted(present - configured):
        problems.append(
            f"{label(unwatched)}/package.json ist in dependabot.yml nicht eingetragen. "
            "Eintrag ergaenzen oder Ordner loeschen."
        )

    for orphan in sorted(present - built):
        problems.append(
            f"{label(orphan)} wird von keiner Frontend-CI-Matrix gebaut. "
            "Entweder in lint-and-typecheck.yml aufnehmen oder als tot loeschen."
        )

    return problems


def check(root: Path) -> list[str]:
    """Fuehrt den Abgleich fuer ein Repo-Verzeichnis aus."""
    return find_problems(
        configured=npm_dirs_in_dependabot(root / DEPENDABOT_REL),
        present=npm_dirs_in_tree(root),
        built=npm_dirs_in_frontend_ci(root / FRONTEND_CI_REL),
    )


def main() -> int:
    problems = check(REPO_ROOT)
    if problems:
        print("Manifest-Scope stimmt nicht:", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    count = len(npm_dirs_in_tree(REPO_ROOT))
    print(f"Manifest-Scope in Ordnung: {count} npm-Projekte, alle ueberwacht und gebaut.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
