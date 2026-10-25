import csv, random, sys
from collections import defaultdict

rows = []
for r in csv.DictReader(open(sys.argv[1])):
    try:
        x = {k: float(r[k]) for k in ("endpeak", "cur_start_n", "cur_avg_n", "cur_peak_n", "lvl3_n", "next_start_n", "dur_h", "gap_h")}
    except ValueError:
        continue
    x["prev_start_n"] = float(r["prev_start_n"]) if r["prev_start_n"] else None
    x["fm"] = float(r["first_min"]) if r["first_min"] else None
    x["nfm"] = float(r["next_first_min"]) if r["next_first_min"] else None
    x["pfm"] = float(r["prev_first_min"]) if r["prev_first_min"] else None
    x["st"] = r["st"]
    x["med"] = float(r["med_avg"])
    rows.append(x)

def clip(v):
    return min(v, 5.0)

def solve(X, y):
    k = len(X[0])
    A = [[sum(X[i][a] * X[i][b] for i in range(len(X))) for b in range(k)] for a in range(k)]
    c = [sum(X[i][a] * y[i] for i in range(len(X))) for a in range(k)]
    for p in range(k):
        m = max(range(p, k), key=lambda r: abs(A[r][p]))
        A[p], A[m], c[p], c[m] = A[m], A[p], c[m], c[p]
        for r in range(k):
            if r != p and A[p][p]:
                f = A[r][p] / A[p][p]
                A[r] = [A[r][j] - f * A[p][j] for j in range(k)]
                c[r] -= f * c[p]
    return [c[i] / A[i][i] if A[i][i] else 0.0 for i in range(k)]

def fe_fit(sub, ycol, xcols):
    by = defaultdict(list)
    for r in sub:
        by[r["st"]].append(r)
    X, y = [], []
    for g in by.values():
        if len(g) < 4:
            continue
        mx = [sum(r[c] for r in g) / len(g) for c in xcols]
        my = sum(clip(r[ycol]) for r in g) / len(g)
        for r in g:
            X.append([r[c] - mx[j] for j, c in enumerate(xcols)])
            y.append(clip(r[ycol]) - my)
    return solve(X, y) if X else None, by

def run(label, sub, ycol, xcols):
    beta, by = fe_fit(sub, ycol, xcols)
    keys = [k for k, g in by.items() if len(g) >= 4]
    rnd = random.Random(5)
    bs = []
    for _ in range(300):
        pick = rnd.choices(keys, k=len(keys))
        s2 = []
        for j, k in enumerate(pick):
            for r in by[k]:
                q = dict(r)
                q["st"] = f"{k}#{j}"
                s2.append(q)
        b, _ = fe_fit(s2, ycol, xcols)
        bs.append(b[0])
    bs.sort()
    print(f"{label}: n {sum(len(by[k]) for k in keys)} streamers {len(keys)} coef hi_endpeak {beta[0]:+.3f} CI {bs[7]:+.3f} to {bs[292]:+.3f}")

med_ep = defaultdict(list)
for r in rows:
    med_ep[r["st"]].append(r["endpeak"])
for k, v in med_ep.items():
    v.sort()
    med_ep[k] = v[len(v) // 2]
for r in rows:
    r["hi"] = 1.0 if r["endpeak"] > med_ep[r["st"]] else 0.0
    r["cur_avg_c"] = clip(r["cur_avg_n"])
    r["cur_start_c"] = clip(r["cur_start_n"])
    r["lvl3_c"] = clip(r["lvl3_n"])
    r["peak_c"] = clip(r["cur_peak_n"])

base = [r for r in rows if r["nfm"] is not None and r["nfm"] <= 5]
print("pairs with next first sample <= 5 min:", len(base))
run("A raw (hi only)", base, "next_start_n", ["hi"])
run("B + current avg, start, level3", base, "next_start_n", ["hi", "cur_avg_c", "cur_start_c", "lvl3_c"])
run("C + duration, peak", base, "next_start_n", ["hi", "cur_avg_c", "cur_start_c", "lvl3_c", "dur_h", "peak_c"])
pl = [r for r in rows if r["prev_start_n"] is not None and r["pfm"] is not None and r["pfm"] <= 5]
run("Placebo: hi_endpeak vs PREVIOUS start, raw", pl, "prev_start_n", ["hi"])
run("Placebo: hi_endpeak vs PREVIOUS start, + level3", pl, "prev_start_n", ["hi", "lvl3_c"])
pop = [r for r in base if r["med"] >= 5]
run("Pop (median >= 5), model C", pop, "next_start_n", ["hi", "cur_avg_c", "cur_start_c", "lvl3_c", "dur_h", "peak_c"])
