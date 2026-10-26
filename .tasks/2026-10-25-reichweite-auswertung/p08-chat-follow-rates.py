import collections
import csv
import datetime as dt
import re
import sys


def ts(s):
    s = (s or "").strip()
    if not s:
        return None
    if re.search(r"[+-]\d\d$", s):
        s += ":00"
    return dt.datetime.fromisoformat(s)


GAP = (dt.datetime(2026, 3, 15, 18, 0, tzinfo=dt.timezone.utc), dt.datetime(2026, 6, 16, 16, 45, tzinfo=dt.timezone.utc))
CUT = dt.datetime(2026, 10, 25, 17, 28, tzinfo=dt.timezone.utc)
WINDOW = dt.timedelta(minutes=60)


def outcome_window_ok(start, end):
    return end <= CUT and not (end > GAP[0] and start < GAP[1])


def rows(path):
    return list(csv.DictReader(open(path)))


def first_chat(path):
    fr = rows(path)
    rep = [r for r in fr if r["bc_reply_3m"] == "t"]
    non = [r for r in fr if r["bc_reply_3m"] == "f"]
    rate = lambda xs, k: sum(r[k] == "t" for r in xs) / len(xs)
    print(f"first-chat events {len(fr)}, channels {len({r['streamer_login'] for r in fr})}, broadcaster message within 3 min {len(rep)} ({100 * len(rep) / len(fr):.1f}%)")
    for k in ("follow_60m", "follow_24h"):
        print(f"  {k}: reply {100 * rate(rep, k):.2f}% (n {len(rep)}) vs none {100 * rate(non, k):.2f}% (n {len(non)}), ratio {rate(rep, k) / rate(non, k):.2f}, all {100 * rate(fr, k):.2f}%")


def presences(sessions_path, presence_path, bots_path, follows_path):
    bots = {r["chatter"].lower() for r in rows(bots_path)}
    sessions = {r["session_id"]: dict(login=r["login"].lower(), start=ts(r["started_at"]), end=ts(r["end_at"]))
                for r in rows(sessions_path) if r["sampled"] == "t" and r["end_at"]}
    pres = collections.defaultdict(list)
    for r in rows(presence_path):
        if r["chatter"].lower() not in bots:
            pres[r["session_id"]].append((r["chatter"].lower(), int(r["messages"] or 0) == 0))
    fol = collections.defaultdict(list)
    for r in rows(follows_path):
        if r["follower_login"].lower() not in bots:
            fol[(r["streamer_login"].lower(), r["follower_login"].lower())].append(ts(r["followed_at"]))
    cnt, hit, used = collections.Counter(), collections.Counter(), set()
    for sid, s in sessions.items():
        if not pres.get(sid) or not outcome_window_ok(s["start"], s["end"] + WINDOW):
            continue
        used.add(sid)
        for chatter, silent in pres[sid]:
            fl = fol.get((s["login"], chatter), [])
            if any(t < s["start"] for t in fl):
                continue
            k = "silent" if silent else "chatter"
            cnt[k] += 1
            hit[k] += any(s["start"] <= t <= s["end"] + WINDOW for t in fl)
    rate = {k: hit[k] / cnt[k] for k in cnt}
    print(f"sampled sessions {len(used)}, channels {len({sessions[x]['login'] for x in used})}")
    for k in ("silent", "chatter"):
        print(f"  {k} presences {cnt[k]}, follow in session + 60 min {hit[k]} = {1000 * rate[k]:.2f} per 1,000")
    print(f"  ratio chatter / silent {rate['chatter'] / rate['silent']:.1f}")


first_chat(sys.argv[1])
presences(*sys.argv[2:6])
