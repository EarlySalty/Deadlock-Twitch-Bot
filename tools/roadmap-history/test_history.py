"""Regression tests for attribution, provenance and the self-contained renderer."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
from history_data import attribute, build_data, kind_of, load_taxonomy, parse_log
from generate_roadmap import atomic_write, render, safe_json


class HistoryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.taxonomy = load_taxonomy(HERE / 'features.json')

    def test_uplink_intent_wins_over_shared_dashboard(self):
        ids, basis, _ = attribute('feat(admin): add AV1 native 2K controls', ['bot/admin_dashboard/src/App.tsx'], self.taxonomy)
        self.assertEqual(ids, ['uplink-av1'])
        self.assertEqual(basis, 'subject')

    def test_caster_history_is_not_generic_admin(self):
        ids, _, _ = attribute('fix: remove duplicate caster camera routes', ['rust/crates/tb-dashboard-api/src/lib.rs'], self.taxonomy)
        self.assertEqual(ids, ['caster-camera'])

    def test_specific_path_beats_generic_subject(self):
        ids, basis, _ = attribute('fix(dashboard): layout verbessern', ['bot/admin_dashboard/src/CasterOverlay.tsx'], self.taxonomy)
        self.assertEqual(ids, ['caster-overlay'])
        self.assertEqual(basis, 'path')

    def test_unknown_not_fabricated(self):
        ids, basis, _ = attribute('Update notes', ['NOTES.txt'], self.taxonomy)
        self.assertEqual(ids, ['other'])
        self.assertEqual(basis, 'unassigned')

    def test_one_commit_can_touch_multiple_features(self):
        ids, _, _ = attribute('feat(chat): pitch free coaching for stuck players', [], self.taxonomy)
        self.assertEqual(ids, ['promos', 'coaching'])

    def test_cross_cutting_migration_not_every_feature(self):
        ids, basis, _ = attribute('general cleanup', ['uplink.rs', 'caster_overlay.rs', 'pause_loop.rs', 'announcements.rs'], self.taxonomy)
        self.assertEqual(ids, ['other'])
        self.assertEqual(basis, 'ambiguous')

    def test_kinds_not_feature_count(self):
        self.assertEqual(kind_of('feat(uplink)!: AV1 hinzufügen'), ('feat', 'AV1 hinzufügen'))
        self.assertEqual(kind_of('chore(deps): package update')[0], 'deps')
        self.assertEqual(kind_of('refactor: shared layout')[0], 'change')
        self.assertEqual(kind_of('security: harden authentication')[0], 'security')
        self.assertEqual(kind_of('Unknown conventional shape')[0], 'change')

    def test_parse_dates_and_hashes(self):
        raw = '\x1e' + 'a' * 40 + '\x1f2026-09-16T23:30:00+00:00\x1ffeat(uplink): AV1\n\nuplink.rs\n'
        parsed = parse_log(raw, self.taxonomy)
        self.assertEqual(parsed[0]['date'], '2026-09-17')
        self.assertEqual(parsed[0]['features'], ['uplink-av1'])
        self.assertEqual(parsed[0]['pathCount'], 1)
        self.assertEqual(parsed[0]['subject'], 'feat(uplink): AV1')

    def test_duplicate_hash_rejected(self):
        raw = '\x1e' + 'b' * 40 + '\x1f2026-09-01T12:00:00Z\x1ffix: a\nfile.txt\n'
        with self.assertRaises(ValueError):
            parse_log(raw + raw, self.taxonomy)

    def test_override_has_explicit_provenance(self):
        taxonomy = {**self.taxonomy, 'overrides': {'c' * 40: {'features': ['uplink'], 'title': 'Kuratierter Titel', 'reason': 'Manuell geprüft'}}}
        raw = '\x1e' + 'c' * 40 + '\x1f2026-09-01T12:00:00Z\x1ffix: a\nfile.txt\n'
        commit = parse_log(raw, taxonomy)[0]
        self.assertEqual(commit['basis'], 'curated')
        self.assertEqual(commit['title'], 'Kuratierter Titel')
        self.assertEqual(commit['subject'], 'fix: a')

    def test_embedded_json_cannot_close_script(self):
        value = {'title': '</script><test> & \u2028', 'quote': '"'}
        encoded = safe_json(value)
        self.assertNotIn('<', encoded)
        self.assertNotIn('&', encoded)
        self.assertEqual(json.loads(encoded), value)

    def test_renderer_is_self_contained(self):
        output = render({'schemaVersion': 1, 'commits': []})
        self.assertNotIn('<!-- ROADMAP_', output)
        self.assertNotIn('<script src=', output)
        self.assertIn('Feature-Stammbaum', output)
        self.assertIn('<dialog id="detail"', output)
        self.assertIn('textContent', output)

    def test_atomic_publish(self):
        with tempfile.TemporaryDirectory() as folder:
            destination = Path(folder) / 'index.html'
            atomic_write(destination, 'before')
            atomic_write(destination, 'after')
            self.assertEqual(destination.read_text(), 'after')
            self.assertEqual(list(Path(folder).iterdir()), [destination])

    def test_reads_only_selected_revision_and_excludes_merge(self):
        with tempfile.TemporaryDirectory() as folder:
            repo = Path(folder)
            def run(*args):
                return subprocess.run(['git', '-C', str(repo), *args], check=True, capture_output=True, text=True).stdout.strip()
            run('init', '-b', 'main')
            run('config', 'user.name', 'Roadmap Test')
            run('config', 'user.email', 'test@example.invalid')
            run('commit', '--allow-empty', '-m', 'feat(uplink): first version')
            run('checkout', '-b', 'side')
            run('commit', '--allow-empty', '-m', 'fix(uplink): second step')
            run('checkout', 'main')
            run('merge', '--no-ff', 'side', '-m', 'Merge side')
            main_sha = run('rev-parse', 'HEAD')
            run('checkout', '-b', 'unmerged')
            run('commit', '--allow-empty', '-m', 'feat(caster): not merged')
            data = build_data(repo, 'main', self.taxonomy)
            self.assertEqual(len(data['commits']), 2)
            self.assertEqual(data['revision'], main_sha)
            self.assertTrue(all(c['features'] == ['uplink'] for c in data['commits']))
            self.assertFalse(data['shallow'])
            self.assertEqual(data['schemaVersion'], 2)
            # Empty test repository: named components must remain explicitly unverified.
            self.assertTrue(all(not f['relation']['verified'] for f in data['features']))
            uplink = next(f for f in data['features'] if f['id'] == 'uplink')
            self.assertTrue(uplink['relation']['missingSources'])
            again = build_data(repo, 'main', self.taxonomy)
            self.assertEqual(data['commits'], again['commits'])
            self.assertEqual(data['features'], again['features'])


if __name__ == '__main__':
    unittest.main()
