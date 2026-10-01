"""Regression coverage for the real-history migration, independent of the UI."""
import copy
import gzip
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from feature_graph import (
    CURRENT_REPOSITORY,
    GENESIS,
    LEGACY_PATHS,
    LEGACY_REPOSITORY,
    ROOT_TITLE,
    atomic_write,
    build_graph,
    canonical,
    load_legacy,
    validate_graph,
)

HERE = Path(__file__).resolve().parent


def commit(sha, date, features=('chat',), kind='fix', repository=CURRENT_REPOSITORY):
    return {'id': sha, 'date': date, 'timestamp': date + 'T12:00:00+02:00',
                'title': 'Änderung ' + sha[:8], 'subject': kind + ': Änderung', 'kind': kind,
                'repository': repository, 'features': list(features), 'paths': ['bot/chat.py'],
                'pathCount': 1, 'basis': 'subject'}


def fixture():
    taxonomy = {'features': [
        {'id': 'chat', 'title': 'Chat', 'description': 'Chat-Befehle', 'parentId': 'product'},
        {'id': 'chat-commands', 'title': 'Befehle', 'description': 'Unterfunktion', 'parentId': 'chat'},
        {'id': 'other', 'title': 'Offene Zuordnung', 'description': 'Unklar', 'parentId': 'product'},
    ]}
    legacy = {'commits': [commit(GENESIS, '2025-09-21', repository=LEGACY_REPOSITORY)],
              'source': {'repository': LEGACY_REPOSITORY, 'revision': 'b' * 40, 'shallow': False,
                             'cutoff': '2026-02-24', 'paths': list(LEGACY_PATHS)}}
    legacy['sha256'] = hashlib.sha256(canonical(legacy)).hexdigest()
    data = {'commits': [commit('a' * 40, '2026-02-24', kind='feat')], 'revision': 'a' * 40,
                'ref': 'test', 'generatedAt': '2026-09-24T10:00:00Z', 'shallow': False}
    return data, taxonomy, legacy


class FeatureGraphTests(unittest.TestCase):
    def graph(self):
        return build_graph(*fixture())

    def test_unique_historical_root_and_repository(self):
        graph = self.graph()
        roots = [n for n in graph['nodes'] if n['parentId'] is None]
        self.assertEqual(len(roots), 1)
        self.assertEqual(roots[0]['id'], 'genesis')
        self.assertEqual(roots[0]['commitHash'], GENESIS)
        self.assertEqual(roots[0]['date'], '2025-09-21')
        self.assertEqual(roots[0]['repository'], LEGACY_REPOSITORY)

    def test_every_commit_is_represented_and_each_node_reaches_root(self):
        data, taxonomy, legacy = fixture()
        data['commits'].append(commit('c' * 40, '2026-03-03', ('chat-commands', 'other')))
        graph = build_graph(data, taxonomy, legacy)
        self.assertEqual({h for n in graph['nodes'] for h in n['commitIds']}, {c['id'] for c in graph['commits']})
        index = {n['id']: n for n in graph['nodes']}
        for n in graph['nodes']:
            seen = set()
            while n['parentId'] is not None:
                self.assertNotIn(n['id'], seen)
                seen.add(n['id'])
                n = index[n['parentId']]
            self.assertEqual(n['id'], 'genesis')
        self.assertEqual(graph['stats']['commits'], 3)

    def test_fixed_buckets_are_bounded(self):
        data, taxonomy, legacy = fixture()
        import datetime as dt
        for i in range(35):
            day = (dt.date(2026, 3, 1) + dt.timedelta(days=i)).isoformat()
            data['commits'].append(commit(format(i + 100, '040x'), day))
        graph = build_graph(data, taxonomy, legacy)
        for n in graph['nodes']:
            if n.get('spanEnd'):
                self.assertLess((dt.date.fromisoformat(n['spanEnd']) - dt.date.fromisoformat(n['date'])).days, 14)
        self.assertGreater(sum(n['role'] == 'event' for n in graph['nodes']), 2)

    def test_refactor_and_maintenance_remain_distinct(self):
        data, taxonomy, legacy = fixture()
        data['commits'].extend([commit('c' * 40, '2026-03-03', kind='refactor'), commit('d' * 40, '2026-03-03', kind='docs')])
        graph = build_graph(data, taxonomy, legacy)
        self.assertTrue(any(n['type'] == 'refactor' for n in graph['nodes']))
        self.assertTrue(any(n['maintenance'] for n in graph['nodes']))

    def test_unknown_origin_is_rejected(self):
        data, taxonomy, legacy = fixture()
        legacy['commits'][0]['id'] = '0' * 40
        with self.assertRaises(ValueError):
            build_graph(data, taxonomy, legacy)

    def test_shallow_current_history_is_rejected(self):
        data, taxonomy, legacy = fixture()
        data['shallow'] = True
        with self.assertRaisesRegex(ValueError, 'vollständige Git-Historie'):
            build_graph(data, taxonomy, legacy)

    def test_cutoff_mismatch_is_rejected(self):
        data, taxonomy, legacy = fixture()
        legacy['source']['cutoff'] = '2026-02-25'
        with self.assertRaises(ValueError):
            build_graph(data, taxonomy, legacy)

    def test_orphan_is_rejected(self):
        nodes = self.graph()['nodes']
        nodes[-1]['parentId'] = 'missing'
        with self.assertRaises(ValueError):
            validate_graph(nodes)

    def test_duplicate_id_is_rejected(self):
        nodes = self.graph()['nodes']
        with self.assertRaises(ValueError):
            validate_graph(nodes + [copy.deepcopy(nodes[0])])

    def test_second_root_is_rejected(self):
        nodes = self.graph()['nodes']
        nodes[-1]['parentId'] = None
        nodes[-1]['type'] = 'root'
        with self.assertRaises(ValueError):
            validate_graph(nodes)

    def test_root_must_be_the_verified_genesis(self):
        nodes = self.graph()['nodes']
        nodes[0]['id'] = 'fake'
        for node in nodes[1:]:
            if node['parentId'] == 'genesis':
                node['parentId'] = 'fake'
        with self.assertRaises(ValueError):
            validate_graph(nodes)

    def test_direct_graph_rejects_forged_genesis_metadata(self):
        for field, value in (
            ('title', 'Erfunden'),
            ('date', '2024-01-01'),
            ('commitHash', '0' * 40),
            ('repository', CURRENT_REPOSITORY),
            ('role', 'feature'),
            ('commitIds', []),
        ):
            nodes = copy.deepcopy(self.graph()['nodes'])
            nodes[0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate_graph(nodes)
        self.assertEqual(self.graph()['nodes'][0]['title'], ROOT_TITLE)

    def test_cycle_is_rejected(self):
        nodes = self.graph()['nodes']
        nodes[-1]['parentId'] = nodes[-1]['id']
        with self.assertRaises(ValueError):
            validate_graph(nodes)

    def test_false_dates_and_unsafe_links_are_rejected(self):
        for field, value in [('date', '2024-01-01'), ('date', '2026-02-30'), ('prUrl', 'javascript:alert(1)'), ('commitHash', '../secret')]:
            nodes = self.graph()['nodes']
            nodes[-1][field] = value
            with self.assertRaises(ValueError):
                validate_graph(nodes)

    def test_cache_integrity(self):
        legacy = fixture()[2]
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'history.gz'
            path.write_bytes(gzip.compress(canonical(legacy), mtime=0))
            self.assertEqual(load_legacy(path)['commits'][0]['id'], GENESIS)
            legacy['commits'][0]['title'] = 'Verändert'
            path.write_bytes(gzip.compress(canonical(legacy), mtime=0))
            with self.assertRaises(ValueError):
                load_legacy(path)

    def test_atomic_replacement(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'graph.json'
            atomic_write(path, b'before')
            atomic_write(path, b'after')
            self.assertEqual(path.read_bytes(), b'after')
            self.assertEqual(list(Path(folder).iterdir()), [path])

    def test_direct_nodes_need_no_legacy_cache(self):
        with tempfile.TemporaryDirectory() as folder:
            source, target = Path(folder) / 'nodes.json', Path(folder) / 'result.json'
            source.write_text(json.dumps(self.graph()['nodes']))
            result = subprocess.run([sys.executable, str(HERE / 'feature_graph.py'), '--input', str(source), '--output', str(target), '--legacy-cache', str(Path(folder) / 'missing')], capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(target.read_text())['rootId'], 'genesis')

    def test_invalid_input_preserves_previous_output(self):
        with tempfile.TemporaryDirectory() as folder:
            source, target = Path(folder) / 'nodes.json', Path(folder) / 'result.json'
            source.write_text('[]')
            target.write_bytes(b'previous valid deployment')
            result = subprocess.run([sys.executable, str(HERE / 'feature_graph.py'), '--input', str(source), '--output', str(target)], capture_output=True, text=True, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(target.read_bytes(), b'previous valid deployment')

    def test_shallow_current_input_preserves_previous_output(self):
        data, _, legacy = fixture()
        data['shallow'] = True
        with tempfile.TemporaryDirectory() as folder:
            source = Path(folder) / 'shallow.json'
            cache = Path(folder) / 'legacy.json.gz'
            target = Path(folder) / 'published.html'
            source.write_text(json.dumps(data), encoding='utf-8')
            cache.write_bytes(gzip.compress(canonical(legacy), mtime=0))
            target.write_text('previous valid deployment', encoding='utf-8')
            result = subprocess.run(
                [sys.executable, str(HERE / 'feature_graph.py'), '--input', str(source),
                 '--legacy-cache', str(cache), '--output', str(target)],
                capture_output=True, text=True, check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(target.read_text(encoding='utf-8'), 'previous valid deployment')

    def test_frozen_real_legacy_snapshot(self):
        legacy = load_legacy(HERE / 'legacy-history.json.gz')
        self.assertEqual(legacy['commits'][0]['id'], GENESIS)
        self.assertGreater(len(legacy['commits']), 300)
        self.assertTrue(all(c['date'] <= legacy['source']['cutoff'] for c in legacy['commits']))

    def test_generator_keeps_published_html_when_legacy_cache_is_damaged(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            assets = root / 'tools' / 'roadmap-history'
            assets.mkdir(parents=True)
            shutil.copy2(HERE.parent / 'generate_roadmap.py', root / 'tools' / 'generate_roadmap.py')
            for name in ('features.json', 'index.html', 'style.css', 'app.js',
                         'history_data.py', 'feature_graph.py'):
                shutil.copy2(HERE / name, assets / name)
            (assets / 'legacy-history.json.gz').write_bytes(b'corrupt cache')
            published = root / 'published.html'
            published.write_text('previous valid page', encoding='utf-8')
            result = subprocess.run(
                [sys.executable, str(root / 'tools' / 'generate_roadmap.py'),
                 '--repo', str(HERE.parents[1]), '--ref', 'origin/main', '--output', str(published)],
                capture_output=True, text=True, check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(published.read_text(encoding='utf-8'), 'previous valid page')


if __name__ == '__main__':
    unittest.main()
