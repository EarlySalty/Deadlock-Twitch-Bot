import csv, glob, collections, datetime, statistics, sys

D = sys.argv[1].rstrip('/') + '/'
PATTERN = sys.argv[2]
GAP_MONTHS = {'2026-04', '2026-05'}
MIN_BINS = [(0, 10), (10, 20), (20, 30), (30, 45), (45, 60), (60, 90), (90, 120), (120, 180), (180, 240), (240, 360), (360, 10 ** 6)]
BANDS = ['0-4', '5-9', '10-19', '20-49', '50-99', '100-299', '300+']


def ts(s):
    return datetime.datetime.fromisoformat(s)


def load():
    exp = []
    fol = {}
    for f in sorted(glob.glob(D + PATTERN + '2026-*.csv')):
        month = f.split(PATTERN)[1][:7]
        for r in csv.DictReader(open(f)):
            if r['kind'] == 'exp':
                exp.append((month, r['min5'], r['band'], r['peak5'], r['in_raid'] == 't', float(r['seconds']), float(r['viewer_seconds'])))
            else:
                key = (r['follow_id'], r['followed_at'])
                if key in fol:
                    raise SystemExit('follow %s at %s lies in two exposure intervals; the intervals must be disjoint' % key)
                fol[key] = (ts(r['session_start']), month, r)
    return exp, fol


def rate(n, vs):
    vh = vs / 3600
    return vh, (100 * n / vh if vh else float('nan'))


def in_bin(v, lo, hi):
    return v != '' and lo <= float(v) < hi


def main():
    exp, fol = load()
    follows = [(m, r) for _, m, r in fol.values()]
    print('aligned follows (one exposure interval each):', len(follows))
    mapped = list(csv.DictReader(open(D + 'follows_mapped.csv')))
    aligned_keys = {(k[0], ts(k[1])) for k in fol}
    mapped_keys = {(r['follow_id'], ts(r['followed_at'])) for r in mapped}
    offline = sum(1 for r in mapped if not r['session_id'])
    in_sess_unaligned = sum(1 for r in mapped if r['session_id'] and (r['follow_id'], ts(r['followed_at'])) not in aligned_keys)
    offline_aligned = sum(1 for r in mapped if not r['session_id'] and (r['follow_id'], ts(r['followed_at'])) in aligned_keys)
    print('cohort %d | offline %d (of which aligned %d) | in a session but outside a sampled interval %d | aligned not in cohort %d' % (
        len(mapped), offline, offline_aligned, in_sess_unaligned, len(aligned_keys - mapped_keys)))
    tot_vs = sum(e[6] for e in exp)
    vh, r = rate(len(follows), tot_vs)
    print('\nOVERALL aligned: follows %d, vh %.0f, per100vh %.2f' % (len(follows), vh, r))
    nogap_f = sum(1 for m, _ in follows if m not in GAP_MONTHS)
    nogap_vs = sum(e[6] for e in exp if e[0] not in GAP_MONTHS)
    vh2, r2 = rate(nogap_f, nogap_vs)
    print('without Apr/May: follows %d, vh %.0f, per100vh %.2f' % (nogap_f, vh2, r2))
    print('\nMONTHLY')
    for m in sorted({e[0] for e in exp}):
        n = sum(1 for mm, _ in follows if mm == m)
        vh, r = rate(n, sum(e[6] for e in exp if e[0] == m))
        print('%s follows %4d vh %7.0f per100vh %5.2f' % (m, n, vh, r))
    print('\nSTREAM MINUTE (min5 on both sides)')
    unk_f = sum(1 for _, r in follows if r['min5'] == '')
    for lo, hi in MIN_BINS:
        n = sum(1 for _, r in follows if in_bin(r['min5'], lo, hi))
        vh, rr = rate(n, sum(e[6] for e in exp if in_bin(e[1], lo, hi)))
        print('%4d-%-6d n=%4d vh=%7.0f per100vh=%5.2f' % (lo, hi, n, vh, rr))
    vh, rr = rate(unk_f, sum(e[6] for e in exp if e[1] == ''))
    print('unknown minute n=%d vh=%.0f' % (unk_f, vh))
    fm = [float(r['follow_min']) for _, r in follows]
    print('median follow minute %.1f; after minute 60 %.1f%%; first 15 min %.1f%%' % (
        statistics.median(fm), 100 * sum(1 for x in fm if x > 60) / len(fm), 100 * sum(1 for x in fm if x < 15) / len(fm)))
    print('\nVIEWER BAND')
    for b in BANDS:
        n = sum(1 for _, r in follows if r['band'] == b)
        vh, rr = rate(n, sum(e[6] for e in exp if e[2] == b))
        print('%-8s n=%4d vh=%7.0f per100vh=%5.2f' % (b, n, vh, rr))
    print('\nRAID WINDOW')
    for flag, lab in ((True, 'in window'), (False, 'outside')):
        n = sum(1 for _, r in follows if (r['in_raid'] == 't') == flag)
        vh, rr = rate(n, sum(e[6] for e in exp if e[4] == flag))
        print('%-9s n=%4d vh=%7.0f per100vh=%5.2f' % (lab, n, vh, rr))
    print('\nPEAK (follow interval start minus session peak, 5-min bins)')
    for b in range(-60, 60, 5):
        n = sum(1 for _, r in follows if r['peak5'] != '' and int(float(r['peak5'])) == b)
        vh, rr = rate(n, sum(e[6] for e in exp if e[3] != '' and int(float(e[3])) == b))
        print('[%4d,%4d) n=%4d vh=%6.0f per100vh=%5.2f' % (b, b + 5, n, vh, rr))


main()
