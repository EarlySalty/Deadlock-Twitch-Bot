import pickle, random, statistics as st
from collections import defaultdict

SM = pickle.load(open("/tmp/reichweite-scratch/P12/py/sm_16s.pkl", "rb"))
ix = {c: i for i, c in enumerate(SM["cols"])}
rows = SM["rows"]


def vbin(v):
    for lo, hi, lab in ((1, 2, "1-2"), (3, 4, "3-4"), (5, 9, "5-9"), (10, 19, "10-19"), (20, 49, "20-49"), (50, 10**9, "50+")):
        if lo <= v <= hi:
            return lab
    return None


def nbin(n):
    if n <= 5:
        return "1-5"
    if n <= 10:
        return "6-10"
    return "11+"


def hband(h):
    if 18 <= h <= 23:
        return "eve"
    if 12 <= h <= 17:
        return "aft"
    return "other"


cells = defaultdict(lambda: defaultdict(lambda: [0, 0.0]))
for r in rows:
    v, p, n = r[ix["V"]], r[ix["new_pres15"]], r[ix["n_de"]]
    if p is None or v is None or v <= 0 or n is None or n < 1:
        continue
    vb = vbin(v)
    if vb is None:
        continue
    key = (r[ix["streamer"]], vb, hband(r[ix["hour"]]), r[ix["weekday"]] >= 5)
    c = cells[key][nbin(n)]
    c[0] += 1
    c[1] += p


def contrast(hi_lab, min_n=30):
    per_streamer = defaultdict(list)
    per_vbin = defaultdict(list)
    for key, d in cells.items():
        a, b = d.get("1-5"), d.get(hi_lab)
        if not a or not b or a[0] < min_n or b[0] < min_n:
            continue
        diff = b[1] / b[0] - a[1] / a[0]
        w = min(a[0], b[0])
        per_streamer[key[0]].append((diff, w))
        per_vbin[key[1]].append(diff)
    s_diffs = [sum(d * w for d, w in L) / sum(w for _, w in L) for L in per_streamer.values()]
    return s_diffs, per_vbin


def boot_mean(xs, n=2000, seed=7):
    rnd = random.Random(seed)
    ms = sorted(st.mean(rnd.choices(xs, k=len(xs))) for _ in range(n))
    return round(ms[int(0.025 * n)], 4), round(ms[int(0.975 * n)], 4)


base = defaultdict(lambda: [0, 0.0])
for key, d in cells.items():
    a = d.get("1-5")
    if a:
        base[key[1]][0] += a[0]
        base[key[1]][1] += a[1]
print("baseline new_pres15 per minute at 1-5 German streams live, by viewer bin")
for vb in ("1-2", "3-4", "5-9", "10-19", "20-49", "50+"):
    b = base[vb]
    if b[0]:
        print(vb, b[0], round(b[1] / b[0], 4))

for lab in ("6-10", "11+"):
    s_diffs, per_vbin = contrast(lab)
    if not s_diffs:
        continue
    print(f"\n{lab} minus 1-5, within streamer x viewer bin x hour band x weekend cell")
    print("streamers", len(s_diffs), "mean diff", round(st.mean(s_diffs), 4), "CI", boot_mean(s_diffs),
          "median", round(st.median(s_diffs), 4), "share negative", round(sum(d < 0 for d in s_diffs) / len(s_diffs), 3))
    for vb in ("1-2", "3-4", "5-9", "10-19", "20-49", "50+"):
        L = per_vbin.get(vb)
        if L and len(L) >= 5:
            print("  vbin", vb, "cells", len(L), "median diff", round(st.median(L), 4), "mean", round(st.mean(L), 4))
