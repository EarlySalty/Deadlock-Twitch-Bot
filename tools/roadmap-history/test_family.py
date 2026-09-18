"""Schema-v2 regression tests: explicit ancestry and honest evidence boundaries."""
import copy
from pathlib import Path
import unittest
from history_data import attribute, load_taxonomy, parse_log, validate_taxonomy


class FamilyTests(unittest.TestCase):
    def setUp(self):
        self.taxonomy = load_taxonomy(Path(__file__).with_name('features.json'))
        self.features = {f['id']: f for f in self.taxonomy['features']}

    def test_four_generations_are_explicit_not_keyword_inferred(self):
        self.assertEqual(self.features['uplink-av1']['parentId'], 'uplink-output')
        self.assertEqual(self.features['uplink-output']['parentId'], 'uplink')
        self.assertEqual(self.features['uplink']['parentId'], 'product')

    def test_cycle_rejected(self):
        self.features['uplink']['parentId'] = 'uplink-av1'
        with self.assertRaisesRegex(ValueError, 'Cycle'):
            validate_taxonomy(self.taxonomy)

    def test_orphan_rejected(self):
        self.features['rank-steam']['parentId'] = 'not-a-feature'
        with self.assertRaisesRegex(ValueError, 'Unknown parent'):
            validate_taxonomy(self.taxonomy)

    def test_duplicate_id_rejected(self):
        self.taxonomy['features'].append(copy.deepcopy(self.features['uplink']))
        with self.assertRaisesRegex(ValueError, 'unique'):
            validate_taxonomy(self.taxonomy)

    def test_relation_without_basis_rejected(self):
        self.features['partner-tags']['relation']['reason'] = ''
        with self.assertRaisesRegex(ValueError, 'reason'):
            validate_taxonomy(self.taxonomy)

    def test_historical_claim_requires_commit(self):
        self.features['caster-camera']['relation']['kind'] = 'historical'
        with self.assertRaisesRegex(ValueError, 'evidence commit'):
            validate_taxonomy(self.taxonomy)

    def test_all_shipped_relations_are_editorial_not_invented_splits(self):
        self.assertTrue(all(f['relation']['kind'] == 'editorial' for f in self.features.values()))
        self.assertTrue(all(f['relation']['reason'] for f in self.features.values()))

    def test_source_must_be_inside_repository(self):
        for invalid in ['/etc/passwd', '../private', 'path\nsecret']:
            self.features['uplink']['relation']['sources'] = [invalid]
            with self.assertRaisesRegex(ValueError, 'repository-relative'):
                validate_taxonomy(self.taxonomy)

    def test_override_requires_reason_and_unique_valid_features(self):
        for override in [{'features': ['uplink']}, {'features': ['missing'], 'reason': 'review'}, {'features': ['uplink', 'uplink'], 'reason': 'review'}]:
            self.taxonomy['overrides'] = {'a' * 40: override}
            with self.assertRaises(ValueError):
                validate_taxonomy(self.taxonomy)

    def test_ambiguous_path_candidates_are_preserved(self):
        ids, basis, evidence = attribute('general cleanup', ['uplink.rs', 'caster_overlay.rs', 'pause_loop.rs', 'announcements.rs'], self.taxonomy)
        self.assertEqual(ids, ['other'])
        self.assertEqual(basis, 'ambiguous')
        for candidate in ['uplink', 'caster-overlay', 'pause', 'announcements']:
            self.assertIn(candidate, evidence[0])

    def test_changes_do_not_create_features(self):
        before = copy.deepcopy(self.taxonomy)
        ids, _, _ = attribute('fix: remove duplicate caster camera routes', [], self.taxonomy)
        self.assertEqual(ids, ['caster-camera'])
        self.assertEqual(self.taxonomy, before)

    def test_specific_descendant_not_double_tagged_with_parent(self):
        ids, _, _ = attribute('feat(uplink): separate AV1 upload-saving enhanced mode', [], self.taxonomy)
        self.assertEqual(ids, ['uplink-av1'])
        self.assertNotIn('uplink', ids)

    def test_independent_feature_membership_can_be_multiple(self):
        ids, _, _ = attribute('feat: add AV1 native 2K controls and host hardware handoff', [], self.taxonomy)
        self.assertEqual(set(ids), {'uplink-av1', 'uplink-host'})
        self.assertEqual(len(ids), len(set(ids)))

    def test_git_timestamps_sort_by_instant_not_offset_text(self):
        records = [
            ('a' * 40, '2026-09-17T22:10:00-01:00'),
            ('b' * 40, '2026-09-18T00:50:00+02:00'),
            ('c' * 40, '2026-09-18T01:30:00+02:00'),
        ]
        raw = ''.join('\x1e' + sha + '\x1f' + timestamp + '\x1ffix: uplink\nuplink.rs\n' for sha, timestamp in records)
        commits = parse_log(raw, self.taxonomy)
        self.assertEqual([c['id'] for c in commits], ['b' * 40, 'a' * 40, 'c' * 40])
        self.assertEqual({c['date'] for c in commits}, {'2026-09-18'})
        self.assertEqual(commits[1]['timestamp'], records[0][1])

    def test_equal_instants_use_stable_commit_id_order(self):
        raw = ('\x1e' + 'a' * 40 + '\x1f2026-09-18T01:10:00+02:00\x1ffix: uplink\n'
               '\x1e' + 'b' * 40 + '\x1f2026-09-17T22:10:00-01:00\x1ffix: uplink\n')
        self.assertEqual([c['id'] for c in parse_log(raw, self.taxonomy)], ['a' * 40, 'b' * 40])

    def test_naive_git_timestamp_rejected_for_reproducibility(self):
        raw = '\x1e' + 'a' * 40 + '\x1f2026-09-18T01:10:00\x1ffix: uplink\n'
        with self.assertRaisesRegex(ValueError, 'timezone'):
            parse_log(raw, self.taxonomy)

    def test_unknown_does_not_receive_an_invented_relationship(self):
        ids, basis, _ = attribute('Unbekannte Erweiterung', ['unrelated.txt'], self.taxonomy)
        self.assertEqual((ids, basis), (['other'], 'unassigned'))


if __name__ == '__main__':
    unittest.main()
