import csv, random, sys
from collections import defaultdict

rows = []
for r in csv.DictReader(open(sys.argv[1])):
    rows.append(dict(st=r["st"], act=r["act"], first_ever=r["first_ever"] == "t", pre=r["bc_pre"] == "t",
                     bc=float(r["bc_delay"]) if r["bc_delay"] else None,
                     fol=float(r["fol_delay"]) if r["fol_delay"] else None))


def deciles(xs):
    xs = sorted(xs)
    return [xs[min(len(xs) - 1, int(len(xs) * q / 10))] for q in range(1, 10)]


def landmarks(sub):
    by_act = defaultdict(list)
    for x in sub:
        if x["bc"] is not None:
            by_act[x["act"]].append(x["bc"])
    return {a: deciles(v) for a, v in by_act.items() if v}


def control(f, qs, horizon):
    inc = sum(1 for q in qs if f is None or f > q) / len(qs)
    out = sum(1 for q in qs if f is not None and q < f <= q + horizon) / len(qs)
    return inc, out


def reply_cells(sub, horizon):
    land = landmarks(sub)
    c = defaultdict(lambda: [0.0, 0.0, 0.0, 0.0])
    for x in sub:
        k = (x["st"], x["act"])
        f = x["fol"]
        if x["bc"] is not None:
            if f is not None and f <= x["bc"]:
                continue
            c[k][0] += 1
            c[k][1] += 1 if f is not None and f <= x["bc"] + horizon else 0
        elif x["act"] in land:
            inc, out = control(f, land[x["act"]], horizon)
            c[k][2] += inc
            c[k][3] += out
    return c


def placebo_cells(sub, horizon):
    land = landmarks(sub)
    c = defaultdict(lambda: [0.0, 0.0, 0.0, 0.0])
    for x in sub:
        if x["bc"] is not None or x["act"] not in land:
            continue
        inc, out = control(x["fol"], land[x["act"]], horizon)
        j = 0 if x["pre"] else 2
        c[(x["st"], x["act"])][j] += inc
        c[(x["st"], x["act"])][j + 1] += out
    return c


def mh(c, keys):
    num = den = 0.0
    for k in keys:
        n1, a1, n0, a0 = c[k]
        if n1 <= 0 or n0 <= 0:
            continue
        w = n1 * n0 / (n1 + n0)
        num += w * (a1 / n1 - a0 / n0)
        den += w
    return num / den if den else float("nan")


def report(label, c, horizon):
    keys = list(c)
    n1 = sum(c[k][0] for k in keys); a1 = sum(c[k][1] for k in keys)
    n0 = sum(c[k][2] for k in keys); a0 = sum(c[k][3] for k in keys)
    by = defaultdict(list)
    for k in keys:
        by[k[0]].append(k)
    sts = list(by)
    rnd = random.Random(7)
    bs = sorted(b for b in (mh(c, [k for s in rnd.choices(sts, k=len(sts)) for k in by[s]]) for _ in range(1000)) if b == b)
    lo, hi = bs[int(0.025 * len(bs))], bs[int(0.975 * len(bs)) - 1]
    print(f"{label} horizon {horizon // 60} min: arm n={n1:.0f} follow {100 * a1 / n1:.2f}% | comparison n={n0:.0f} follow {100 * a0 / n0:.2f}% | MH {100 * mh(c, keys):+.2f} pp (CI {100 * lo:+.2f} to {100 * hi:+.2f}) streamers {len(sts)}")


for label, sub in (("V7b per-session first message", rows), ("V7a first message ever in channel", [x for x in rows if x["first_ever"]])):
    rep = [x for x in sub if x["bc"] is not None]
    quick = [x for x in rep if x["fol"] is not None and x["fol"] <= 180]
    before = sum(1 for x in quick if x["fol"] <= x["bc"])
    print(f"\n{label}: events {len(sub)}, broadcaster message within 3 min {len(rep)}; follows within 3 min of the first message in that arm {len(quick)}: {before} before the broadcaster message, {len(quick) - before} after")
    for h in (3600, 86400):
        report(f"{label} reply vs none", reply_cells(sub, h), h)
    report(f"{label} placebo: broadcaster wrote in the 3 min before, no reply after", placebo_cells(sub, 3600), 3600)
