"""Read-only Git history extraction and explicit, deterministic feature attribution."""
import datetime as dt
import json
from pathlib import Path
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


def load_taxonomy(path):
    data = json.loads(Path(path).read_text(encoding='utf-8'))
    group_ids = {g['id'] for g in data['groups']}
    ids = [f['id'] for f in data['features']]
    if len(ids) != len(set(ids)) or 'other' not in ids:
        raise ValueError('Feature IDs must be unique and include other')
    for f in data['features']:
        if f['group'] not in group_ids or not re.fullmatch(r'[a-z][a-z0-9-]*', f['id']):
            raise ValueError('Invalid feature ID or group')
        re.compile(f['subject'], re.I)
        re.compile(f['paths'], re.I)
    for sha, override in data.get('overrides', {}).items():
        if not re.fullmatch(r'[0-9a-f]{40,64}', sha):
            raise ValueError('Overrides require a full commit hash')
        if not override.get('features') or not set(override['features']) <= set(ids):
            raise ValueError('Unknown feature in override')
    return data


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


def attribute(subject, paths, taxonomy):
    features = taxonomy['features']
    matches = [f for f in features if re.search(f['subject'], subject, re.I)]
    specific = [f for f in matches if f['id'] not in GENERIC]
    if specific:
        return [f['id'] for f in specific[:3]], 'subject', ['Commit-Titel']
    path_matches = []
    for f in features:
        hits = [p for p in paths if re.search(f['paths'], p, re.I)]
        if hits:
            path_matches.append((f, hits))
    specific_paths = [(f, hits) for f, hits in path_matches if f['id'] not in GENERIC]
    if 0 < len(specific_paths) <= 3:
        evidence = sorted({p for _, hits in specific_paths for p in hits})[:8]
        return [f['id'] for f, _ in specific_paths], 'path', evidence
    if matches:
        return [matches[0]['id']], 'subject', ['Commit-Titel']
    if len(specific_paths) > 3:
        return ['runtime'], 'crosscut', ['Mehr als drei Feature-Pfade: bereichsübergreifend']
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
            evidence = [override.get('reason', 'Redaktionelle Zuordnung')]
        commits.append({
            'id': sha, 'date': date, 'timestamp': timestamp,
            'title': title, 'subject': subject, 'kind': kind,
            'features': ids, 'basis': basis, 'evidence': evidence,
            'paths': paths[:20], 'pathCount': len(paths), 'note': override.get('note', ''),
        })
    return sorted(commits, key=lambda c: (c['date'], c['timestamp'], c['id']))


def build_data(repo, ref, taxonomy):
    revision = git(repo, 'rev-parse', '--verify', ref + '^{commit}').strip()
    shallow = git(repo, 'rev-parse', '--is-shallow-repository').strip() == 'true'
    raw = git(repo, 'log', revision, '--no-merges', '--no-renames', '--name-only',
              '--format=%x1e%H%x1f%cI%x1f%s')
    commits = parse_log(raw, taxonomy)
    if not commits:
        raise ValueError('The selected revision contains no commits')
    return {
        'schemaVersion': 1,
        'generatedAt': dt.datetime.now(dt.timezone.utc).isoformat(),
        'revision': revision, 'ref': ref, 'shallow': shallow,
        'repository': 'EarlySalty/Deadlock-Twitch-Bot', 'timezone': 'Europe/Berlin',
        'groups': taxonomy['groups'],
        'features': [{k: f[k] for k in ('id', 'group', 'title', 'description')} for f in taxonomy['features']],
        'commits': commits,
    }
