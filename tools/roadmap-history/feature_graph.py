"""Build a connected, dated feature tree from the existing Git taxonomy.

Only commit metadata and changed paths are read. Parent relations describe
editorial feature families, not inferred technical dependencies or deployments.
"""
from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile

from history_data import attribute, build_data, git, load_taxonomy, parse_log

CATEGORIES = ('core', 'twitch', 'dashboard', 'api', 'community')
TYPES = ('root', 'major_feature', 'update', 'refactor')
MAINTENANCE = {'docs', 'test', 'chore', 'deps', 'ci'}
GENESIS = '3654f6c73be53fc569da673fb307e7e0c79f2b87'
LEGACY_REPOSITORY = 'EarlySalty/Deadlock-Bots'
CURRENT_REPOSITORY = 'EarlySalty/Deadlock-Twitch-Bot'
LEGACY_PATHS = ('cogs/twitch_deadlock', 'cogs/twitch', 'cogs/twitch_cog')
CATEGORY_ROOTS = {
    'runtime': 'core', 'brain': 'core', 'other': 'core',
    'auth': 'api', 'billing': 'api',
    'dashboard': 'dashboard', 'analytics': 'dashboard', 'coaching': 'dashboard',
    'raids': 'community', 'engagement': 'community', 'promos': 'community',
    'partners': 'community', 'moderation': 'community',
}
SHA = re.compile(r'[0-9a-f]{40,64}')


def canonical(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode('utf-8')


def atomic_write(path, content):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.feature-graph-', delete=False) as handle:
            temporary = Path(handle.name)
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        temporary.chmod(0o644)
        temporary.replace(path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def load_legacy(path):
    """A frozen closed chapter: SHA-256 detects truncation or accidental edits."""
    raw = Path(path).read_bytes()
    payload = json.loads(gzip.decompress(raw) if raw[:2] == b'\x1f\x8b' else raw)
    body = {key: payload[key] for key in ('commits', 'source')}
    if hashlib.sha256(canonical(body)).hexdigest() != payload.get('sha256'):
        raise ValueError('Die Prüfsumme der Vorgeschichte stimmt nicht.')
    if not body['commits'] or body['source'].get('shallow'):
        raise ValueError('Die Vorgeschichte fehlt oder ist unvollständig.')
    if body['source'].get('repository') != LEGACY_REPOSITORY:
        raise ValueError('Unerwartetes Ursprungsrepository.')
    if not SHA.fullmatch(body['source'].get('revision', '')):
        raise ValueError('Gepinnte Revision der Vorgeschichte fehlt.')
    if set(body['source'].get('paths', [])) != set(LEGACY_PATHS):
        raise ValueError('Die Vorgeschichte ist nicht auf die Twitch-Pfade begrenzt.')
    return payload


def extract_legacy(repo, ref, taxonomy, cutoff='2026-02-24'):
    dt.date.fromisoformat(cutoff)
    revision = git(repo, 'rev-parse', '--verify', '--end-of-options', ref + '^{commit}').strip()
    shallow = git(repo, 'rev-parse', '--is-shallow-repository').strip() == 'true'
    if shallow:
        raise ValueError('Für den Ursprung wird eine vollständige Git-Historie benötigt.')
    raw = git(repo, 'log', revision, '--no-merges', '--full-history', '--no-renames',
              '--name-only', '--format=%x1e%H%x1f%cI%x1f%s', '--', *LEGACY_PATHS)
    commits = []
    for c in parse_log(raw, taxonomy):
        if c['date'] > cutoff:
            continue
        paths = [re.sub(r'^cogs/(?:twitch_deadlock|twitch_cog|twitch)/', 'bot/', p) for p in c['paths']]
        ids, basis, evidence = attribute(c['subject'], paths, taxonomy)
        commits.append({**c, 'features': ids, 'basis': basis, 'evidence': evidence,
                        'repository': LEGACY_REPOSITORY})
    if not commits or commits[0]['id'] != GENESIS or commits[0]['date'] != '2025-09-21':
        raise ValueError('Der geprüfte Twitch-Ursprung wurde in dieser Historie nicht gefunden.')
    source = dict(repository=LEGACY_REPOSITORY, revision=revision, ref=ref,
                  shallow=False, paths=list(LEGACY_PATHS), cutoff=cutoff)
    body = dict(commits=commits, source=source)
    return {**body, 'sha256': hashlib.sha256(canonical(body)).hexdigest()}


def validate_graph(nodes):
    index = {}
    for n in nodes:
        for key in ('id', 'title', 'description', 'date', 'category', 'type', 'parentId'):
            if key not in n:
                raise ValueError('FeatureNode.' + key + ' fehlt.')
        if not isinstance(n['id'], str) or not n['id'] or n['id'] in index:
            raise ValueError('Knoten-ID fehlt oder ist doppelt.')
        if not isinstance(n['title'], str) or not isinstance(n['description'], str):
            raise ValueError('Titel und Beschreibung müssen Text sein.')
        if not re.fullmatch(r'\d{4}-\d{2}-\d{2}', n['date']):
            raise ValueError('ISO-Datum erforderlich.')
        dt.date.fromisoformat(n['date'])
        if n['category'] not in CATEGORIES or n['type'] not in TYPES:
            raise ValueError('Unbekannte Kategorie oder Knotenart.')
        if n.get('spanEnd'):
            dt.date.fromisoformat(n['spanEnd'])
            if n['spanEnd'] < n['date']:
                raise ValueError('Bündel endet vor seinem Beginn.')
        if n.get('commitHash') and not SHA.fullmatch(n['commitHash']):
            raise ValueError('Ungültiger Commit-Hash.')
        if n.get('repository') and n['repository'] not in (LEGACY_REPOSITORY, CURRENT_REPOSITORY):
            raise ValueError('Unerwartetes Repository.')
        if n.get('prUrl') and not re.fullmatch(r'https://github\.com/EarlySalty/Deadlock-(?:Twitch-)?Bots?/pull/[1-9]\d*', n['prUrl']):
            raise ValueError('Ungültige PR-Referenz.')
        index[n['id']] = n
    roots = [n for n in nodes if n['parentId'] is None]
    if len(roots) != 1 or roots[0]['type'] != 'root':
        raise ValueError('Genau ein Ursprung mit parentId=null erforderlich.')
    done = set()
    for n in nodes:
        seen, cursor = set(), n
        while cursor['id'] not in done:
            if cursor['id'] in seen:
                raise ValueError('Zyklus in den Elternbeziehungen.')
            seen.add(cursor['id'])
            parent = cursor['parentId']
            if parent is None:
                break
            if cursor['type'] == 'root' or not isinstance(parent, str) or parent not in index:
                raise ValueError('Unbekannter Elternknoten.')
            if index[parent]['date'] > cursor['date']:
                raise ValueError('Ein Kind liegt vor seinem Elternknoten.')
            cursor = index[parent]
        done.update(seen)
    return roots[0]['id']


def build_graph(data, taxonomy, legacy, bucket_days=14):
    if not 1 <= bucket_days <= 31:
        raise ValueError('Bündelfenster muss zwischen 1 und 31 Tagen liegen.')
    features = {f['id']: f for f in taxonomy['features']}
    current = [{**c, 'repository': CURRENT_REPOSITORY} for c in data['commits']]
    cutoff = min((c['date'] for c in current if any(p.startswith(('bot/', 'twitch_cog/', 'rust/')) for p in c['paths'])), default='')
    if not cutoff or legacy['source']['cutoff'] != cutoff:
        raise ValueError('Die Schnittstelle zwischen Vorgeschichte und neuem Repository stimmt nicht.')
    unique = {c['id']: c for c in current}
    for c in legacy['commits']:
        if c['date'] > cutoff or c.get('repository') != LEGACY_REPOSITORY:
            raise ValueError('Commit außerhalb der abgeschlossenen Vorgeschichte.')
        unique[c['id']] = c
    commits = sorted(unique.values(), key=lambda c: (c['date'], dt.datetime.fromisoformat(c['timestamp']), c['id']))
    if not commits or commits[0]['id'] != GENESIS or commits[0]['date'] != '2025-09-21':
        raise ValueError('Der belegte Ursprung ist nicht der erste Twitch-Knoten.')
    origin = commits[0]
    assigned = {fid: [] for fid in features}
    subtree = {fid: [] for fid in features}
    for c in commits:
        for fid in set(c['features']):
            if fid not in features:
                raise ValueError('Commit verweist auf eine unbekannte Funktion.')
            assigned[fid].append(c)
            cursor = fid
            while cursor != 'product':
                subtree[cursor].append(c)
                cursor = features[cursor]['parentId']
    def category(fid):
        while features[fid]['parentId'] != 'product':
            fid = features[fid]['parentId']
        return CATEGORY_ROOTS.get(fid, 'twitch')
    def node(nid, title, description, lead, cat, kind, parent, **more):
        result = dict(id=nid, title=title, description=description, date=lead['date'],
                      category=cat, type=kind, parentId=parent, commitHash=lead['id'],
                      repository=lead['repository'], **more)
        if lead.get('prUrl'):
            result['prUrl'] = lead['prUrl']
        return result
    nodes = [node('genesis', 'Twitch Bot Genesis / Core Init',
                  'Erster Git-Nachweis des Twitch-Bots im ursprünglichen Deadlock-Bots-Repository. Kein Nachweis des ersten Live-Betriebs.',
                  origin, 'core', 'root', None, role='root', commitIds=[origin['id']], maintenance=False)]
    epoch = dt.date(1970, 1, 1)
    for fid, f in features.items():
        if not subtree[fid]:
            continue
        lead = min(subtree[fid], key=lambda c: (c['date'], c['timestamp'], c['id']))
        parent = 'genesis' if f['parentId'] == 'product' else 'feature:' + f['parentId']
        direct = assigned[fid]
        first_direct = direct[0] if direct else None
        initial_ids = [first_direct['id']] if first_direct and first_direct['date'] == lead['date'] else []
        nodes.append(node('feature:' + fid, f['title'], f['description'], lead, category(fid),
                          'major_feature', parent, role='feature', featureId=fid,
                          commitIds=initial_ids, maintenance=False, relation=f.get('relation', {}),
                          evidenceCount=len({c['id'] for c in subtree[fid]})))
        buckets = {}
        for c in direct:
            if c['id'] in initial_ids or c['id'] == origin['id']:
                continue
            kind = 'refactor' if re.match(r'^(refactor|perf|refine)(?:\(|:)', c['subject'], re.I) else 'major_feature' if c['kind'] == 'feat' else 'update'
            maintenance = c['kind'] in MAINTENANCE
            bucket = (dt.date.fromisoformat(c['date']) - epoch).days // bucket_days
            buckets.setdefault((bucket, kind, maintenance), []).append(c)
        for (bucket, kind, maintenance), group in sorted(buckets.items()):
            representative = next((c for c in reversed(group) if c['kind'] == 'feat'), group[-1])
            n = node('event:' + fid + ':' + str(bucket) + ':' + kind + (':care' if maintenance else ''),
                     representative['title'], f['description'], representative, category(fid), kind,
                     'feature:' + fid, role='event', featureId=fid, commitIds=[c['id'] for c in group],
                     spanEnd=group[-1]['date'], maintenance=maintenance, count=len(group))
            n['date'] = group[0]['date']
            nodes.append(n)
    validate_graph(nodes)
    represented = {sha for n in nodes for sha in n['commitIds']}
    if represented != set(unique):
        raise ValueError('Die Bündelung hat Git-Änderungen verloren.')
    return dict(schemaVersion=3, rootId='genesis', nodes=nodes, commits=commits,
                revision=data['revision'], ref=data['ref'], generatedAt=data['generatedAt'],
                shallow=data['shallow'], timezone='Europe/Berlin', bucketDays=bucket_days,
                sources=[legacy['source'], dict(repository=CURRENT_REPOSITORY, revision=data['revision'], ref=data['ref'])],
                legacyFingerprint=legacy['sha256'], coverage=dict(first=origin['date'], last=commits[-1]['date']),
                stats=dict(commits=len(commits), nodes=len(nodes), features=sum(n['role'] == 'feature' for n in nodes),
                           legacyCommits=len(legacy['commits']), unassigned=sum(c['basis'] in ('unassigned', 'ambiguous') for c in commits)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path)
    parser.add_argument('--ref', default='origin/main')
    parser.add_argument('--taxonomy', type=Path, default=Path(__file__).with_name('features.json'))
    parser.add_argument('--legacy-repo', type=Path)
    parser.add_argument('--legacy-ref', default='origin/main')
    parser.add_argument('--legacy-cache', type=Path, default=Path(__file__).with_name('legacy-history.json.gz'))
    parser.add_argument('--write-legacy-cache', type=Path)
    parser.add_argument('--input', type=Path, help='Existing schema-2 metadata or validated FeatureNode JSON')
    parser.add_argument('--bucket-days', type=int, default=14)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    incoming = json.loads(args.input.read_text(encoding='utf-8')) if args.input else None
    if incoming is not None and (isinstance(incoming, list) or 'nodes' in incoming):
        if not args.output:
            parser.error('--output erforderlich')
        nodes = incoming if isinstance(incoming, list) else incoming['nodes']
        root_id = validate_graph(nodes)
        output = {'schemaVersion': 3, 'rootId': root_id, 'nodes': nodes} if isinstance(incoming, list) else {**incoming, 'schemaVersion': 3, 'rootId': root_id}
        atomic_write(args.output, canonical(output))
        print(json.dumps({'nodes': len(nodes)}))
        return
    taxonomy = load_taxonomy(args.taxonomy)
    legacy = extract_legacy(args.legacy_repo, args.legacy_ref, taxonomy) if args.legacy_repo else load_legacy(args.legacy_cache)
    if args.write_legacy_cache:
        atomic_write(args.write_legacy_cache, gzip.compress(canonical(legacy), mtime=0))
    if args.output:
        if incoming is not None:
            data = incoming
        elif args.repo:
            data = build_data(args.repo, args.ref, taxonomy)
        else:
            parser.error('--repo oder --input erforderlich')
        output = build_graph(data, taxonomy, legacy, args.bucket_days)
        atomic_write(args.output, canonical(output))
        print(json.dumps(output.get('stats', {'nodes': len(output['nodes'])})))
    elif not args.write_legacy_cache:
        parser.error('--output oder --write-legacy-cache erforderlich')


if __name__ == '__main__':
    main()
