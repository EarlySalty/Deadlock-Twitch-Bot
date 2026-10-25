import csv, random, sys
from collections import defaultdict

rows = list(csv.DictReader(open(sys.argv[1])))
cells = defaultdict(dict)
for r in rows:
    cells[(r["stratum_streamer"], r["act"])][r["reply3"]] = (int(r["n"]), int(r["fol60"]), int(r["fol24h"]))


def pooled(col):
    t = {"t": [0, 0], "f": [0, 0]}
    for d in cells.values():
        for k, v in d.items():
            t[k][0] += v[0]
            t[k][1] += v[col]
    return {k: (v[0], v[1], round(100 * v[1] / v[0], 2) if v[0] else None) for k, v in t.items()}


def mh_rd(keys, col):
    num = den = 0.0
    for k in keys:
        d = cells[k]
        if "t" not in d or "f" not in d:
            continue
        n1, a1 = d["t"][0], d["t"][col]
        n0, a0 = d["f"][0], d["f"][col]
        w = n1 * n0 / (n1 + n0)
        num += w * (a1 / n1 - a0 / n0)
        den += w
    return num / den if den else None


def mh_or(keys, col):
    num = den = 0.0
    for k in keys:
        d = cells[k]
        if "t" not in d or "f" not in d:
            continue
        n1, a1 = d["t"][0], d["t"][col]
        n0, a0 = d["f"][0], d["f"][col]
        n = n1 + n0
        num += a1 * (n0 - a0) / n
        den += a0 * (n1 - a1) / n
    return num / den if den else None


keys = [k for k, d in cells.items() if "t" in d and "f" in d]
streamers = sorted({k[0] for k in keys})
by_s = defaultdict(list)
for k in keys:
    by_s[k[0]].append(k)
print("strata with both arms", len(keys), "streamers", len(streamers))
print("treated n in informative strata", sum(cells[k]["t"][0] for k in keys))
for col, lab in ((1, "follow 60m"), (2, "follow 24h")):
    print(f"\n{lab}: pooled", pooled(col))
    rd = mh_rd(keys, col)
    orr = mh_or(keys, col)
    rnd = random.Random(11)
    bs_rd, bs_or = [], []
    for _ in range(2000):
        ks = [k for s in rnd.choices(streamers, k=len(streamers)) for k in by_s[s]]
        x = mh_rd(ks, col)
        y = mh_or(ks, col)
        if x is not None:
            bs_rd.append(x)
        if y is not None:
            bs_or.append(y)
    bs_rd.sort()
    bs_or.sort()
    q = lambda L, p: L[int(p * len(L))]
    print(f"  MH risk diff {100 * rd:.2f} pp, CI {100 * q(bs_rd, .025):.2f} to {100 * q(bs_rd, .975):.2f}")
    print(f"  MH OR {orr:.2f}, CI {q(bs_or, .025):.2f} to {q(bs_or, .975):.2f}")
    top = sorted(by_s, key=lambda s: -sum(cells[k]["t"][0] for k in by_s[s]))[:3]
    rest = [k for s in streamers if s not in top for k in by_s[s]]
    print(f"  without top 3 treated streamers: MH RD {100 * mh_rd(rest, col):.2f} pp, OR {mh_or(rest, col):.2f}")
