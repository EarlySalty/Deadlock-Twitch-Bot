import csv, math, random, sys
from collections import defaultdict

rows = defaultdict(list)
for r in csv.DictReader(open(sys.argv[1])):
    rows[r["st"]].append((int(r["t"]), math.log2(float(r["avg_viewers"]) + 1), {k: r[k] == "t" for k in ("bang", "discord", "bracket", "short", "chill")}))

WIN = 14 * 86400


def per_streamer(feat, local):
    out = []
    for st, ss in rows.items():
        on = [s for s in ss if s[2][feat]]
        off = [s for s in ss if not s[2][feat]]
        if len(on) < 2 or len(off) < 2:
            continue
        if not local:
            out.append(sum(s[1] for s in on) / len(on) - sum(s[1] for s in off) / len(off))
            continue
        diffs = []
        for s in on:
            near = [o[1] for o in off if abs(o[0] - s[0]) <= WIN]
            if len(near) >= 2:
                diffs.append(s[1] - sum(near) / len(near))
        if len(diffs) >= 2:
            out.append(sum(diffs) / len(diffs))
    return out


def summarize(xs):
    xs = sorted(xs)
    n = len(xs)
    med = xs[n // 2] if n % 2 else (xs[n // 2 - 1] + xs[n // 2]) / 2
    pos = sum(x > 0 for x in xs)
    neg = sum(x < 0 for x in xs)
    k, m = min(pos, neg), pos + neg
    p = min(1.0, 2 * sum(math.comb(m, i) for i in range(k + 1)) / 2 ** m) if m else 1.0
    rnd = random.Random(3)
    bs = sorted(sorted(rnd.choices(xs, k=n))[n // 2] for _ in range(2000))
    pct = lambda d: 100 * (2 ** d - 1)
    return f"streamers {n} median {pct(med):+.1f}% CI {pct(bs[50]):+.1f}% to {pct(bs[1950]):+.1f}% share+ {pos / n:.2f} sign p {p:.3f}"


for feat in ("bang", "discord", "bracket", "short", "chill"):
    print(feat)
    print("  all sessions    ", summarize(per_streamer(feat, False)))
    print("  +-14 day matched", summarize(per_streamer(feat, True)))
