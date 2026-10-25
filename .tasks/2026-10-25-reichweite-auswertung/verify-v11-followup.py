import bisect, csv, random, sys
from collections import defaultdict

sess = defaultdict(list)
st_of = {}
for r in csv.DictReader(open(sys.argv[1])):
    sess[r["sid"]].append((int(r["t"]), r["ch"], r["bc"] == "t", r["label"]))
    st_of[r["sid"]] = r["st"]

LABELS = {"Social", "Game-Related", "Question", "Statement", "Reaction", "Greeting", "Hype", "Feedback", "Technical", "Other", "Command"}


def bucket(b):
    return 0 if b == 0 else 1 if b <= 2 else 2 if b <= 5 else 3 if b <= 10 else 4 if b <= 20 else 5


obs = []
for sid, rows in sess.items():
    rows.sort()
    ts = [r[0] for r in rows]
    own = defaultdict(list)
    for t, ch, bc, lab in rows:
        own[ch].append(t)
    t0, t1 = ts[0], ts[-1]
    for t, ch, bc, lab in rows:
        if bc or lab not in LABELS or t < t0 + 300 or t > t1 - 120:
            continue
        o = own[ch]
        after = (bisect.bisect_right(ts, t + 120) - bisect.bisect_right(ts, t)) - (bisect.bisect_right(o, t + 120) - bisect.bisect_right(o, t))
        before = (bisect.bisect_left(ts, t) - bisect.bisect_left(ts, t - 120)) - (bisect.bisect_left(o, t) - bisect.bisect_left(o, t - 120))
        obs.append((st_of[sid], sid, lab, before, after))
print("scored viewer messages", len(obs), "streamers", len({o[0] for o in obs}))


def naive_lift(label):
    by = defaultdict(lambda: [0, 0, 0, 0])
    for st, sid, lab, b, a in obs:
        c = by[(st, sid)]
        c[0] += 1
        c[1] += a
        if lab == label:
            c[2] += 1
            c[3] += a
    per = defaultdict(list)
    for (st, sid), (n, s, nl, sl) in by.items():
        if n >= 30 and nl >= 3 and s > 0:
            per[st].append((sl / nl) / (s / n))
    meds = sorted(sorted(v)[len(v) // 2] for v in per.values())
    return len(meds), meds[len(meds) // 2], sum(m > 1 for m in meds) / len(meds)


def adjusted(label):
    cells = defaultdict(lambda: [0, 0, 0, 0])
    for st, sid, lab, b, a in obs:
        c = cells[(st, sid, bucket(b))]
        if lab == label:
            c[0] += 1
            c[1] += a
        else:
            c[2] += 1
            c[3] += a
    keys = [k for k, c in cells.items() if c[0] and c[2]]
    by = defaultdict(list)
    for k in keys:
        by[k[0]].append(k)

    def est(ks):
        num = den = base = 0.0
        for k in ks:
            n1, a1, n0, a0 = cells[k]
            w = n1 * n0 / (n1 + n0)
            num += w * (a1 / n1 - a0 / n0)
            base += w * (a0 / n0)
            den += w
        return num / den, num / base

    d, r = est(keys)
    sts = list(by)
    rnd = random.Random(11)
    bs = sorted(est([k for s in rnd.choices(sts, k=len(sts)) for k in by[s]]) for _ in range(1000))
    rs = sorted(x[1] for x in bs)
    return len(sts), sum(cells[k][0] for k in keys), d, r, rs[25], rs[975]


for label in ("Social", "Game-Related", "Question", "Greeting", "Statement", "Reaction"):
    n, med, share = naive_lift(label)
    s, m, d, r, lo, hi = adjusted(label)
    print(f"{label}: naive lift streamers {n} median {med:.2f} share>1 {share:.2f} | prior-activity adjusted vs other labels: streamers {s} msgs {m} diff {d:+.3f} msgs, rel {100 * r:+.1f}% CI {100 * lo:+.1f}% to {100 * hi:+.1f}%")
