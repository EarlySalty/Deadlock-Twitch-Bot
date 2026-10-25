import csv, random, sys
from collections import defaultdict

sess = defaultdict(list)
st_of = {}
for r in csv.DictReader(open(sys.argv[1])):
    sid = r["session_id"]
    st_of[sid] = r["st"]
    sess[sid].append((int(r["slot"]), int(r["vmsg"]), int(r["bcmsg"]), int(r["newc"])))


def vb(v):
    return 0 if v == 0 else 1 if v <= 2 else 2 if v <= 5 else 3 if v <= 15 else 4


def build(expo, outcome, stratum, keep=lambda prev, cur: True):
    cells = defaultdict(lambda: [0, 0, 0, 0])
    for sid, rows in sess.items():
        rows.sort()
        for i in range(1, len(rows) - 1):
            prev, cur, nxt = rows[i - 1], rows[i], rows[i + 1]
            if not keep(prev, cur):
                continue
            k = (st_of[sid], sid, stratum(prev, cur))
            e = expo(prev, cur)
            y = outcome(cur, nxt)
            c = cells[k]
            if e:
                c[0] += 1
                c[1] += y
            else:
                c[2] += 1
                c[3] += y
    return cells


def mh(cells, keys):
    num = den = 0.0
    for k in keys:
        n1, a1, n0, a0 = cells[k]
        if n1 and n0:
            w = n1 * n0 / (n1 + n0)
            num += w * (a1 / n1 - a0 / n0)
            den += w
    return num / den if den else float("nan")


def report(label, cells):
    keys = [k for k, c in cells.items() if c[0] and c[2]]
    by = defaultdict(list)
    for k in keys:
        by[k[0]].append(k)
    sts = list(by)
    n1 = sum(cells[k][0] for k in keys)
    a1 = sum(cells[k][1] for k in keys)
    n0 = sum(cells[k][2] for k in keys)
    a0 = sum(cells[k][3] for k in keys)
    est = mh(cells, keys)
    rnd = random.Random(9)
    bs = sorted(mh(cells, [k for s in rnd.choices(sts, k=len(sts)) for k in by[s]]) for _ in range(1000))
    print(f"{label}: streamers {len(sts)}, exposed slots {n1} ({100 * a1 / max(n1, 1):.1f}%), unexposed {n0} ({100 * a0 / max(n0, 1):.1f}%), MH diff {100 * est:+.2f} pp CI {100 * bs[25]:+.2f} to {100 * bs[975]:+.2f}")


nxt_new = lambda cur, nxt: int(nxt[3] > 0)
strat = lambda prev, cur: (vb(cur[1]), vb(prev[1]), cur[3] > 0)
report("A broadcaster msg in t -> new chatter in t+1, within session x viewer activity t, t-1",
       build(lambda p, c: c[2] > 0, nxt_new, strat))
report("B same, only slots where viewers were silent in t",
       build(lambda p, c: c[2] > 0, nxt_new, lambda p, c: (vb(p[1]),), keep=lambda p, c: c[1] == 0))
report("C reverse: new chatter in t -> broadcaster msg in t+1, within session x viewer activity",
       build(lambda p, c: c[3] > 0, lambda c, n: int(n[2] > 0), lambda p, c: (vb(c[1]), vb(p[1]), c[2] > 0)))
report("D viewer chat in t (vs silent) -> new chatter in t+1, within session x previous activity",
       build(lambda p, c: c[1] > 0, nxt_new, lambda p, c: (vb(p[1]), c[2] > 0)))
