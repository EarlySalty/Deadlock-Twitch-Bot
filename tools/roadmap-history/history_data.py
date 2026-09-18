"""Read-only Git extraction with explicit, reviewable feature relationships."""
import datetime as dt
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
from zoneinfo import ZoneInfo

GENERIC = {'chat', 'analytics', 'auth', 'dashboard', 'runtime', 'other'}
MAINTENANCE = {'docs', 'test', 'chore', 'deps', 'ci'}
CONVENTIONAL = re.compile(r'^([a-z]+)(?:\(([^)]+)\))?!?:\s*(.+)$', re.I)


def git(repo, *args):
    result = subprocess.run(
        ['git', '-C', str(repo), '-c', 'core.quotepath=false', *args],
        check=True, capture_output=True, text=True, encoding='utf-8',
        errors='replace', timeout=120,
    )
    return result.stdout


def validate_taxonomy(data):
    group_ids = {g['id'] for g in data['groups']}
    ids = [f['id'] for f in data['features']]
    if len(ids) != len(set(ids)) or 'other' not in ids or 'product' in ids:
        raise ValueError('Feature IDs must be unique, exclude product and include other')
    parents = {}
    for f in data['features']:
        if f['group'] not in group_ids or not re.fullmatch(r'[a-z][a-z0-9-]*', f['id']):
            raise ValueError('Invalid feature ID or group')
        re.compile(f['subject'], re.I)
        re.compile(f['paths'], re.I)
        parent = f.get('parentId')
        if parent not in set(ids) | {'product'}:
            raise ValueError('Unknown parent: ' + str(parent))
        parents[f['id']] = parent
        relation = f.get('relation', {})
        if relation.get('kind') not in {'editorial', 'historical'} or not relation.get('reason', '').strip():
            raise ValueError('Every relationship requires an explicit kind and reason')
        if relation['kind'] == 'historical' and not re.fullmatch(r'[0-9a-f]{40,64}', relation.get('commit', '')):
            raise ValueError('Historical relationships require a full evidence commit')
        for source in relation.get('sources', []):
            if not source or PurePosixPath(source).is_absolute() or '..' in PurePosixPath(source).parts or any(ord(c) < 32 for c in source):
                raise ValueError('Source paths must be repository-relative')
    for feature in ids:
        seen, cursor = set(), feature
        while cursor != 'product':
            if cursor in seen:
                raise ValueError('Cycle in feature relationships: ' + feature)
            seen.add(cursor)
            cursor = parents[cursor]
    for sha, override in data.get('overrides', {}).items():
        if not re.fullmatch(r'[0-9a-f]{40,64}', sha):
            raise ValueError('Overrides require a full commit hash')
        assigned = override.get('features', [])
        if not assigned or not set(assigned) <= set(ids) or len(assigned) != len(set(assigned)):
            raise ValueError('Unknown or duplicate feature in override')
        if not override.get('reason', '').strip():
            raise ValueError('Manual overrides require a reason')
    return data


def load_taxonomy(path):
    return validate_taxonomy(json.loads(Path(path).read_text(encoding='utf-8')))


def kind_of(subject):
    match = CONVENTIONAL.match(subject)
    kind, title = (match[1].lower(), match[3]) if match else ('change', subject)
    if kind in {'security', 'sec'}:
        kind = 'security'
    elif kind in {'perf', 'refactor', 'refine', 'improve'}:
        kind = 'change'
    elif kind in {'build', 'style'}:
        kind = 'chore'
    if kind == 'chore' and match and match[2] == 'deps':
        kind = 'deps'
    if re.search(r'\b(dependabot|bump .+ from .+ to)\b', subject, re.I):
        kind = 'deps'
    if kind not in {'feat', 'fix', 'security', 'change'} | MAINTENANCE:
        kind = 'change'
    return kind, title.strip()


def most_specific(features, taxonomy):
    """Suppress a broad parent tag, never infer or create a parent relationship."""
    parents = {f['id']: f.get('parentId', 'product') for f in taxonomy['features']}
    ancestors = set()
    for feature in features:
        parent, seen = parents[feature['id']], set()
        while parent != 'product' and parent not in seen:
            ancestors.add(parent)
            seen.add(parent)
            parent = parents.get(parent, 'product')
    return [f for f in features if f['id'] not in ancestors]


def attribute(subject, paths, taxonomy):
    features = taxonomy['features']
    matches = [f for f in features if re.search(f['subject'], subject, re.I)]
    specific = most_specific([f for f in matches if f['id'] not in GENERIC], taxonomy)
    if specific:
        if len(specific) > 3:
            return ['other'], 'ambiguous', ['Mehrdeutiger Commit-Titel; mögliche Funktionen: ' + ', '.join(f['id'] for f in specific)]
        return [f['id'] for f in specific], 'subject', ['Commit-Titel']
    path_matches = []
    for f in features:
        hits = [p for p in paths if re.search(f['paths'], p, re.I)]
        if hits:
            path_matches.append((f, hits))
    specific_ids = {f['id'] for f in most_specific([f for f, _ in path_matches if f['id'] not in GENERIC], taxonomy)}
    specific_paths = [(f, hits) for f, hits in path_matches if f['id'] in specific_ids]
    if 0 < len(specific_paths) <= 3:
        evidence = sorted({p for _, hits in specific_paths for p in hits})[:8]
        return [f['id'] for f, _ in specific_paths], 'path', evidence
    if len(specific_paths) > 3:
        return ['other'], 'ambiguous', ['Bereichsübergreifende Pfade; mögliche Funktionen: ' + ', '.join(f['id'] for f, _ in specific_paths)]
    if matches:
        return [matches[0]['id']], 'subject', ['Commit-Titel']
    if path_matches:
        feature, hits = path_matches[0]
        return [feature['id']], 'path', hits[:8]
    return ['other'], 'unassigned', []


def parse_log(raw, taxonomy):
    commits, seen = [], set()
    for record in raw.split('\x1e'):
        if not record.strip():
            continue
        header, _, file_text = record.lstrip('\n').partition('\n')
        sha, timestamp, subject = header.split('\x1f', 2)
        if not re.fullmatch(r'[0-9a-f]{40,64}', sha) or sha in seen:
            raise ValueError('Invalid or duplicate commit hash')
        seen.add(sha)
        date = dt.datetime.fromisoformat(timestamp).astimezone(ZoneInfo('Europe/Berlin')).date().isoformat()
        paths = [p for p in file_text.splitlines() if p]
        kind, title = kind_of(subject)
        ids, basis, evidence = attribute(subject, paths, taxonomy)
        override = taxonomy.get('overrides', {}).get(sha, {})
        if override:
            ids, basis = override['features'], 'curated'
            title = override.get('title', title)
            evidence = [override['reason']]
        commits.append({
            'id': sha, 'date': date, 'timestamp': timestamp,
            'title': title, 'subject': subject, 'kind': kind,
            'features': ids, 'basis': basis, 'evidence': evidence,
            'paths': paths[:20], 'pathCount': len(paths), 'note': override.get('note', ''),
        })
    return sorted(commits, key=lambda c: (c['date'], c['timestamp'], c['id']))


def build_data(repo, ref, taxonomy):
    validate_taxonomy(taxonomy)
    revision = git(repo, 'rev-parse', '--verify', '--end-of-options', ref + '^{commit}').strip()
    shallow = git(repo, 'rev-parse', '--is-shallow-repository').strip() == 'true'
    raw = git(repo, 'log', revision, '--no-merges', '--no-renames', '--name-only',
              '--format=%x1e%H%x1f%cI%x1f%s')
    commits = parse_log(raw, taxonomy)
    if not commits:
        raise ValueError('The selected revision contains no commits')
    tree = set(git(repo, 'ls-tree', '-r', '--name-only', revision).splitlines())
    commit_ids = {c['id'] for c in commits}
    features = []
    for feature in taxonomy['features']:
        item = {key: feature[key] for key in ('id', 'parentId', 'group', 'title', 'description')}
        relation = dict(feature['relation'])
        relation['verifiedSources'] = [p for p in relation.get('sources', []) if p in tree or any(t.startswith(p.rstrip('/') + '/') for t in tree)]
        relation['missingSources'] = [p for p in relation.get('sources', []) if p not in relation['verifiedSources']]
        relation['verified'] = bool(relation['verifiedSources']) and not relation['missingSources']
        if relation['kind'] == 'historical':
            relation['verified'] = relation['verified'] and relation['commit'] in commit_ids
        item['relation'] = relation
        features.append(item)
    return {
        'schemaVersion': 2,
        'generatedAt': dt.datetime.now(dt.timezone.utc).isoformat(),
        'revision': revision, 'ref': ref, 'shallow': shallow,
        'repository': 'EarlySalty/Deadlock-Twitch-Bot', 'timezone': 'Europe/Berlin',
        'groups': taxonomy['groups'], 'features': features, 'commits': commits,
    }
