"""Tests fuer scripts/check_manifest_scope.py.

Der Check soll genau die Drift finden, die am 26.08.2026 zu Karteileichen-Alerts
gefuehrt hat: ein Verzeichnis steht in dependabot.yml, das Projekt gibt es aber
nicht mehr (`.github/eslint-security/`), oder ein Projekt liegt im Baum, ohne
dass eine Konfiguration es kennt.
"""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
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


class ManifestScopeTests(unittest.TestCase):
    def make_repo(
        self,
        *,
        watched: list[str],
        built: list[str],
        projects: list[str],
    ) -> Path:
        temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(temp_dir.cleanup)
        return build_repo(
            Path(temp_dir.name), watched=watched, built=built, projects=projects
        )

    def test_deckungsgleiche_konfiguration_meldet_nichts(self) -> None:
        repo = self.make_repo(
            watched=["/website", "/bot/admin_dashboard"],
            built=["website", "bot/admin_dashboard"],
            projects=["website", "bot/admin_dashboard"],
        )

        self.assertEqual(check_manifest_scope.check(repo), [])

    def test_ueberwachtes_verzeichnis_ohne_projekt_wird_gemeldet(self) -> None:
        """Der Fall .github/eslint-security: Config blieb, Ordner war weg."""
        repo = self.make_repo(
            watched=["/website", "/.github/eslint-security"],
            built=["website"],
            projects=["website"],
        )

        problems = check_manifest_scope.check(repo)

        self.assertEqual(len(problems), 1)
        self.assertIn("/.github/eslint-security", problems[0])
        self.assertIn("keine package.json", problems[0])

    def test_punktverzeichnis_mit_projekt_meldet_nichts(self) -> None:
        """Zustand vor dem Aufraeumen: Ordner da, Config da, CI baut ihn.

        Faengt die Pfad-Normalisierung ab: wer fuehrende Punkte abschneidet, macht
        aus `.github/eslint-security` ein `github/eslint-security` und meldet zwei
        Fehler, von denen einer luegt.
        """
        repo = self.make_repo(
            watched=["/.github/eslint-security"],
            built=[".github/eslint-security"],
            projects=[".github/eslint-security"],
        )

        self.assertEqual(check_manifest_scope.check(repo), [])

    def test_root_projekt_meldet_nichts(self) -> None:
        """`directory: "/"` und ein Root-Manifest muessen zusammenpassen."""
        repo = self.make_repo(watched=["/"], built=["."], projects=[""])

        self.assertEqual(check_manifest_scope.check(repo), [])

    def test_unbeaufsichtigtes_projekt_wird_zweifach_gemeldet(self) -> None:
        repo = self.make_repo(
            watched=["/website"],
            built=["website"],
            projects=["website", "tools/tote-app"],
        )

        problems = check_manifest_scope.check(repo)

        self.assertEqual(len(problems), 2)
        self.assertTrue(
            any("nicht eingetragen" in p and "tools/tote-app" in p for p in problems)
        )
        self.assertTrue(
            any(
                "keiner Frontend-CI-Matrix" in p and "tools/tote-app" in p
                for p in problems
            )
        )

    def test_ueberwachtes_projekt_ohne_ci_matrix_wird_gemeldet(self) -> None:
        repo = self.make_repo(
            watched=["/website", "/bot/dashboard_v2"],
            built=["website"],
            projects=["website", "bot/dashboard_v2"],
        )

        problems = check_manifest_scope.check(repo)

        self.assertEqual(len(problems), 1)
        self.assertIn("bot/dashboard_v2", problems[0])
        self.assertIn("keiner Frontend-CI-Matrix", problems[0])

    def test_build_ordner_zaehlen_nicht_als_projekt(self) -> None:
        """node_modules und dist duerfen keine Befunde erzeugen."""
        repo = self.make_repo(
            watched=["/website"],
            built=["website"],
            projects=["website", "website/node_modules/react", "website/dist"],
        )

        self.assertEqual(check_manifest_scope.check(repo), [])

    def test_cache_dependency_path_gilt_nicht_als_matrix_eintrag(self) -> None:
        """`cache-dependency-path:` darf kein Projekt still als gebaut markieren."""
        repo = self.make_repo(
            watched=["/website"],
            built=[],
            projects=["website"],
        )

        built = check_manifest_scope.npm_dirs_in_frontend_ci(
            repo / ".github" / "workflows" / "lint-and-typecheck.yml"
        )

        self.assertEqual(built, set())
        problems = check_manifest_scope.check(repo)
        self.assertEqual(len(problems), 1)
        self.assertIn("keiner Frontend-CI-Matrix", problems[0])

    def test_echtes_repo_ist_deckungsgleich(self) -> None:
        """Der Produktivpfad: die eingecheckte Konfiguration selbst."""
        self.assertEqual(check_manifest_scope.check(REPO_ROOT), [])


if __name__ == "__main__":
    unittest.main()
