import csv, glob, os, sys
from collections import defaultdict

DAY_DIR, LIVE_CSV, BOTS_TXT = sys.argv[1], sys.argv[2], sys.argv[3]
MIN_SAMPLES = int(sys.argv[4]) if len(sys.argv) > 4 else 10
ROOM_BURN_DAYS = int(sys.argv[5]) if len(sys.argv) > 5 else 0
BOTS = set(open(BOTS_TXT).read().split())
FILES = sorted(glob.glob(os.path.join(DAY_DIR, "cat_*.csv")))
DAYS = [f.split("cat_")[-1][:8] for f in FILES]
DAYS = [f"{d[:4]}-{d[4:6]}-{d[6:]}" for d in DAYS]
IDX = {d: i for i, d in enumerate(DAYS)}


def live_bits():
    out = defaultdict(int)
    for r in csv.DictReader(open(LIVE_CSV, newline="")):
        if r["day"] in IDX and int(r["samples"]) >= MIN_SAMPLES:
            out[r["user_id"]] |= 1 << IDX[r["day"]]
    return out


def chat_bits():
    bits, rooms, info = {}, defaultdict(int), {}
    for f, d in zip(FILES, DAYS):
        i = IDX[d]
        for r in csv.DictReader(open(f, newline="")):
            if not r.get("room_user_id") or r["login"] in BOTS:
                continue
            k = r["room_user_id"] + "|" + r["chatter_user_id"]
            if k not in bits:
                info[k] = (r["lang"], r["room_user_id"], i)
            bits[k] = bits.get(k, 0) | (1 << i)
            rooms[r["room_user_id"]] |= 1 << i
    return bits, rooms, info


def window(i0):
    return sum(1 << (i0 + k) for k in range(1, 8) if i0 + k < len(DAYS))


def newcomers():
    live = live_bits()
    bits, rooms, info = chat_bits()
    first = {r: (b & -b).bit_length() - 1 for r, b in rooms.items()}
    out = []
    for k, (lang, room, i) in info.items():
        if i == 0 or not "2026-10-09" <= DAYS[i] <= "2026-10-17" or i - first[room] < ROOM_BURN_DAYS:
            continue
        w = window(i)
        out.append(dict(room=room, lang=lang, ret7=(bits[k] & w) != 0, chat_opp=(rooms[room] & w) != 0,
                        live_opp=(live[room] & w) != 0, live_d0=bool(live[room] >> i & 1),
                        live_days=bin(live[room] & w).count("1")))
    return out


def pct(xs, key):
    return 100 * sum(x[key] for x in xs) / len(xs) if xs else float("nan")


def summary(lab, xs):
    live = [x for x in xs if x["live_opp"]]
    nolive = [x for x in xs if not x["live_opp"]]
    chat = [x for x in xs if x["chat_opp"]]
    live_nochat = [x for x in live if not x["chat_opp"]]
    md = sorted(x["live_days"] for x in xs)
    print(f"\n{lab}: newcomers={len(xs)} rooms={len({x['room'] for x in xs})} ret7={pct(xs, 'ret7'):.1f} | room live in category on first day {pct(xs, 'live_d0'):.1f}%")
    print(f"  later recorded chat in room within 7d: {pct(xs, 'chat_opp'):.1f}% (ret7 {pct(chat, 'ret7'):.1f})")
    print(f"  room live in Deadlock category on a later day within 7d: {pct(xs, 'live_opp'):.1f}% (ret7 {pct(live, 'ret7'):.1f}); not live: ret7 {pct(nolive, 'ret7'):.1f} n={len(nolive)}")
    print(f"  live later but no recorded chat later: {100 * len(live_nochat) / len(live) if live else float('nan'):.1f}% of live-opportunity newcomers")
    print(f"  live days in the 7-day window: median {md[len(md) // 2]}, mean {sum(md) / len(md):.2f}")
    for lo, hi in ((0, 0), (1, 2), (3, 4), (5, 7)):
        g = [x for x in xs if lo <= x["live_days"] <= hi]
        print(f"    live days {lo}-{hi}: n={len(g)} ({100 * len(g) / len(xs):.1f}%) ret7={pct(g, 'ret7'):.1f}")


NC = newcomers()
print(f"min snapshots per live day: {MIN_SAMPLES}; room in chat data at least {ROOM_BURN_DAYS} days before; newcomers with first day 2026-10-09..10-17, bots excluded")
for lab, lang in (("all languages", None), ("German", "de"), ("English", "en")):
    summary(lab, [x for x in NC if lang is None or x["lang"] == lang])
