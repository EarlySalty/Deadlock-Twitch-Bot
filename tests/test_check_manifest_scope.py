"""Tests fuer scripts/check_manifest_scope.py.

Der Check soll genau die Drift finden, die am 26.08.2026 zu Karteileichen-Alerts
gefuehrt hat: ein Verzeichnis steht in dependabot.yml, das Projekt gibt es aber
nicht mehr (`.github/eslint-security/`), oder ein Projekt liegt im Baum, ohne
dass eine Konfiguration es kennt.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
MODULE_PATH = REPO_ROOT / "scripts" / "check_manifest_scope.py"

_spec = importlib.util.spec_from_file_location("check_manifest_scope", MODULE_PATH)
assert _spec and _spec.loader
check_manifest_scope = importlib.util.module_from_spec(_spec)
sys.modules["check_manifest_scope"] = check_manifest_scope
_spec.loader.exec_module(check_manifest_scope)


DEPENDABOT_TEMPLATE = """version: 2
updates:
  - package-ecosystem: "github-actions"
    directory: "/"
    schedule:
      interval: "daily"
{npm_blocks}"""

NPM_BLOCK = """
  - package-ecosystem: "npm"
    directory: "{directory}"
    schedule:
      interval: "daily"
"""

WORKFLOW_TEMPLATE = """name: "Frontend CI"
jobs:
  frontend-ci:
    strategy:
      matrix:
        project:
{matrix}
    steps:
      - name: Set up Node.js
        uses: actions/setup-node@v7
        with:
          cache-dependency-path: ${{{{ matrix.project.path }}}}/package-lock.json
"""

MATRIX_ENTRY = """          - name: {name}
            path: {path}
"""


def build_repo(
    tmp_path: Path,
    *,
    watched: list[str],
    built: list[str],
    projects: list[str],
) -> Path:
    """Baut ein Mini-Repo mit Konfiguration und package.json-Dateien.

    `watched` sind Verzeichnisse wie in dependabot.yml (mit fuehrendem Slash),
    `built` die Matrix-Pfade der Frontend-CI, `projects` die Ordner, in denen
    tatsaechlich eine package.json liegt ("" bedeutet Repo-Root).
    """
    (tmp_path / ".github" / "workflows").mkdir(parents=True)

    npm_blocks = "".join(NPM_BLOCK.format(directory=d) for d in watched)
    (tmp_path / ".github" / "dependabot.yml").write_text(
        DEPENDABOT_TEMPLATE.format(npm_blocks=npm_blocks), encoding="utf-8"
    )

    matrix = "".join(
        MATRIX_ENTRY.format(name=p.replace("/", "_") or "root", path=p) for p in built
    )
    (tmp_path / ".github" / "workflows" / "lint-and-typecheck.yml").write_text(
        WORKFLOW_TEMPLATE.format(matrix=matrix), encoding="utf-8"
    )

    for project in projects:
        project_dir = tmp_path / project if project else tmp_path
        project_dir.mkdir(parents=True, exist_ok=True)
        (project_dir / "package.json").write_text('{"name": "x"}', encoding="utf-8")

    return tmp_path


def test_deckungsgleiche_konfiguration_meldet_nichts(tmp_path: Path) -> None:
    repo = build_repo(
        tmp_path,
        watched=["/website", "/bot/admin_dashboard"],
        built=["website", "bot/admin_dashboard"],
        projects=["website", "bot/admin_dashboard"],
    )

    assert check_manifest_scope.check(repo) == []


def test_ueberwachtes_verzeichnis_ohne_projekt_wird_gemeldet(tmp_path: Path) -> None:
    """Der Fall .github/eslint-security: Config blieb, Ordner war weg."""
    repo = build_repo(
        tmp_path,
        watched=["/website", "/.github/eslint-security"],
        built=["website"],
        projects=["website"],
    )

    problems = check_manifest_scope.check(repo)

    assert len(problems) == 1
    assert "/.github/eslint-security" in problems[0]
    assert "keine package.json" in problems[0]


def test_punktverzeichnis_mit_projekt_meldet_nichts(tmp_path: Path) -> None:
    """Zustand vor dem Aufraeumen: Ordner da, Config da, CI baut ihn.

    Faengt die Pfad-Normalisierung ab: wer fuehrende Punkte abschneidet, macht
    aus `.github/eslint-security` ein `github/eslint-security` und meldet zwei
    Fehler, von denen einer luegt.
    """
    repo = build_repo(
        tmp_path,
        watched=["/.github/eslint-security"],
        built=[".github/eslint-security"],
        projects=[".github/eslint-security"],
    )

    assert check_manifest_scope.check(repo) == []


def test_root_projekt_meldet_nichts(tmp_path: Path) -> None:
    """`directory: "/"` und ein Root-Manifest muessen zusammenpassen."""
    repo = build_repo(
        tmp_path,
        watched=["/"],
        built=["."],
        projects=[""],
    )

    assert check_manifest_scope.check(repo) == []


def test_unbeaufsichtigtes_projekt_wird_zweifach_gemeldet(tmp_path: Path) -> None:
    repo = build_repo(
        tmp_path,
        watched=["/website"],
        built=["website"],
        projects=["website", "tools/tote-app"],
    )

    problems = check_manifest_scope.check(repo)

    assert len(problems) == 2
    assert any("nicht eingetragen" in p and "tools/tote-app" in p for p in problems)
    assert any("keiner Frontend-CI-Matrix" in p and "tools/tote-app" in p for p in problems)


def test_ueberwachtes_projekt_ohne_ci_matrix_wird_gemeldet(tmp_path: Path) -> None:
    repo = build_repo(
        tmp_path,
        watched=["/website", "/bot/dashboard_v2"],
        built=["website"],
        projects=["website", "bot/dashboard_v2"],
    )

    problems = check_manifest_scope.check(repo)

    assert len(problems) == 1
    assert "bot/dashboard_v2" in problems[0]
    assert "keiner Frontend-CI-Matrix" in problems[0]


def test_build_ordner_zaehlen_nicht_als_projekt(tmp_path: Path) -> None:
    """node_modules und dist duerfen keine Befunde erzeugen."""
    repo = build_repo(
        tmp_path,
        watched=["/website"],
        built=["website"],
        projects=["website", "website/node_modules/react", "website/dist"],
    )

    assert check_manifest_scope.check(repo) == []


def test_cache_dependency_path_gilt_nicht_als_matrix_eintrag(tmp_path: Path) -> None:
    """`cache-dependency-path:` darf kein Projekt still als gebaut markieren."""
    repo = build_repo(
        tmp_path,
        watched=["/website"],
        built=[],
        projects=["website"],
    )

    built = check_manifest_scope.npm_dirs_in_frontend_ci(
        repo / ".github" / "workflows" / "lint-and-typecheck.yml"
    )

    assert built == set()
    problems = check_manifest_scope.check(repo)
    assert len(problems) == 1
    assert "keiner Frontend-CI-Matrix" in problems[0]


def test_echtes_repo_ist_deckungsgleich() -> None:
    """Der Produktivpfad: die eingecheckte Konfiguration selbst."""
    assert check_manifest_scope.check(REPO_ROOT) == []
