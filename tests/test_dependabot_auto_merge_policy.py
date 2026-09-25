from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "dependabot-auto-merge.yml"


class DependabotAutoMergePolicyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflow = WORKFLOW.read_text(encoding="utf-8")

    def test_no_direct_or_dispatch_merge_path_remains(self):
        self.assertNotIn("workflow_dispatch:", self.workflow)
        self.assertNotIn("reconcile-open-prs:", self.workflow)
        self.assertNotIn("github.rest.pulls.merge", self.workflow)

    def test_native_auto_merge_is_bound_to_the_evaluated_head(self):
        self.assertIn(
            'gh pr merge --repo "$GITHUB_REPOSITORY" "$PR_NUMBER" --auto --squash '
            '--match-head-commit "$HEAD_SHA"',
            self.workflow,
        )
        self.assertIn("current.head.sha !== pr.head.sha", self.workflow)

    def test_all_release_checks_must_be_enforced_by_github_actions(self):
        for check in (
            "Semantic review",
            "Scope-Abgleich (keine toten Projekte)",
            "Frontend PR Gate (website)",
            "Frontend PR Gate (admin_dashboard)",
            "Frontend PR Gate (dashboard_v2)",
            "Rust SQLx required",
        ):
            self.assertIn(f'"{check}"', self.workflow)

        self.assertIn("strict_required_status_checks_policy === true", self.workflow)
        self.assertIn("check.integration_id === 15368", self.workflow)
        self.assertIn("missing.length > 0", self.workflow)

    def test_policy_changes_and_untrusted_heads_cannot_auto_merge(self):
        self.assertIn("github.event.pull_request.user.id == 49699333", self.workflow)
        self.assertIn(
            "github.event.pull_request.head.repo.full_name == github.repository",
            self.workflow,
        )
        self.assertIn('filename.startsWith(".github/")', self.workflow)
        self.assertIn('filename.startsWith("scripts/ci/")', self.workflow)


if __name__ == "__main__":
    unittest.main()
