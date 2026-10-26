import csv, statistics as st, sys
from collections import defaultdict

def num(x):
    return float(x) if x not in ("", None) else None

rows = [dict(r) for r in csv.DictReader(open(sys.argv[1]))]
for r in rows:
    for k in ("first_mi", "last_mi", "n_min", "pk", "argmax_mi", "t95_mi", "duration_seconds") + tuple(f"h{i}" for i in range(1, 9)):
        r[k] = num(r[k])

def q(xs, p):
    xs = sorted(xs)
    return st.quantiles(xs, n=4)[0 if p == 25 else 2] if len(xs) >= 4 else None

def table(sel, label):
    by = defaultdict(list)
    for r in sel:
        by[r["sc"]].append(r)
    for sc in ("0-4", "5-19", "20-99", "100+"):
        g = by.get(sc, [])
        t = [r["t95_mi"] for r in g if r["t95_mi"] is not None]
        a = [r["argmax_mi"] for r in g if r["argmax_mi"] is not None]
        if t:
            print(label, sc, len(g), "t95 med", st.median(t), "iqr", q(t, 25), q(t, 75), "argmax med", st.median(a))

base = [r for r in rows if r["first_mi"] is not None and r["first_mi"] <= 15 and r["pk"] >= 3]
table(base, "first<=15,pk>=3")
five = [r for r in base if r["sc"] == "5-19" and r["duration_seconds"] >= 18000]
t = [r["t95_mi"] for r in five]
a = [r["argmax_mi"] for r in five]
print("5h 5-19 n", len(five), "t95 med", st.median(t), "argmax med", st.median(a), "share argmax>=180", round(sum(x >= 180 for x in a) / len(a), 3))
for sc in ("5-19",):
    for k in (3, 4, 5, 6, 8):
        rs = [r[f"h{k}"] / r["h2"] for r in base if r["sc"] == sc and r["h2"] and r[f"h{k}"] is not None]
        rs2 = [(r[f"h{k}"], r["h2"]) for r in base if r["sc"] == sc and r["h2"] and r[f"h{k}"] is not None]
        print(sc, "hour", k, "n", len(rs), "median ratio", round(st.median(rs), 3) if rs else None, "ratio of sums", round(sum(x for x, _ in rs2) / sum(y for _, y in rs2), 3) if rs2 else None)
