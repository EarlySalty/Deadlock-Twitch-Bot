import collections
import csv
import re
import statistics as st
import sys
from datetime import datetime, timedelta

START = datetime(2026, 2, 15)
GAP = (datetime(2026, 3, 15, 18, 0), datetime(2026, 6, 16, 16, 45))
CUT = datetime(2026, 10, 25, 17, 28)


def ts(s):
    m = re.match(r"(\d{4}-\d{2}-\d{2})[ T](\d{2}:\d{2}:\d{2})(\.\d+)?", (s or "").strip())
    if not m:
        return None
    t = datetime.strptime(f"{m.group(1)} {m.group(2)}", "%Y-%m-%d %H:%M:%S")
    return t.replace(microsecond=int(m.group(3)[1:7].ljust(6, "0"))) if m.group(3) else t


def window_ok(a, b):
    return b <= CUT and not (b > GAP[0] and a < GAP[1])


def merge(ivs):
    out = []
    for a, b in sorted(ivs):
        if out and a <= out[-1][1]:
            out[-1][1] = max(out[-1][1], b)
        else:
            out.append([a, b])
    return out


def clip(ivs, lo, hi):
    return [[max(a, lo), min(b, hi)] for a, b in ivs if min(b, hi) > max(a, lo)]


def without_gap(ivs):
    out = []
    for a, b in ivs:
        out += [[a, min(b, GAP[0])], [max(a, GAP[1]), b]]
    return [x for x in out if x[1] > x[0]]


def intersect(x, y):
    out, i, j = [], 0, 0
    while i < len(x) and j < len(y):
        a, b = max(x[i][0], y[j][0]), min(x[i][1], y[j][1])
        if b > a:
            out.append([a, b])
        if x[i][1] < y[j][1]:
            i += 1
        else:
            j += 1
    return out


def inside(ivs, t):
    return any(a <= t < b for a, b in ivs)


def minutes(ivs):
    return sum((b - a).total_seconds() / 60 for a, b in ivs)


def control_comparison(raid_rows, t0_by_id):
    ev = []
    for r in raid_rows:
        t = ts(r["t_utc"])
        if r["covers60"] != "t" or r["other_raids_pm60"] != "0" or not r["session_id"] or t < START:
            continue
        if r["tid"] in t0_by_id and t >= t0_by_id[r["tid"]] and window_ok(t, t + timedelta(minutes=60)):
            ev.append(r)
    raid = [r for r in ev if r["kind"] == "raid"]
    ctl = [r for r in ev if r["kind"] == "ctl"]
    print(f"control comparison: raids {len(raid)}, controls {len(ctl)}, channels {len({r['tid'] for r in ev})}")
    for k in ("fol30", "fol60"):
        rm, cm = st.mean(int(r[k]) for r in raid), st.mean(int(r[k]) for r in ctl)
        print(f"  {k}: raid {rm:.3f} ({sum(int(r[k]) for r in raid)}) vs control {cm:.3f} ({sum(int(r[k]) for r in ctl)}), difference {rm - cm:+.3f}")


def session_end(r):
    if ts(r["ended_at"]):
        return ts(r["ended_at"])
    if r["duration_seconds"]:
        return ts(r["started_at"]) + timedelta(seconds=float(r["duration_seconds"]))
    return CUT


def window_share(raid_rows, sessions, follows, t0_by_login):
    session_by_id = {r["session_id"]: r for r in sessions}
    raids = collections.defaultdict(list)
    for r in raid_rows:
        if r["kind"] == "raid" and r["session_id"] in session_by_id:
            raids[session_by_id[r["session_id"]]["streamer"]].append(ts(r["t_utc"]))
    live = collections.defaultdict(list)
    for r in sessions:
        if r["streamer"] in t0_by_login and ts(r["started_at"]):
            live[r["streamer"]].append([ts(r["started_at"]), session_end(r)])
    tot_live = tot_win = n = n_in = 0
    channels = set()
    for login, t0 in t0_by_login.items():
        lv = without_gap(clip(merge(live[login]), max(t0, START), CUT))
        wn = intersect(lv, merge([[w, w + timedelta(minutes=30)] for w in raids.get(login, [])]))
        tot_live += minutes(lv)
        tot_win += minutes(wn)
        for t in follows.get(login, []):
            if inside(lv, t):
                n += 1
                n_in += inside(wn, t)
                channels.add(login)
    r_in, r_org = n_in / tot_win, (n - n_in) / (tot_live - tot_win)
    print(f"window share: follows in tracked live time {n} (channels {len(channels)}), within 30 min after a raid {n_in} ({100 * n_in / n:.2f}%)")
    print(f"  raid windows {100 * tot_win / tot_live:.2f}% of {tot_live:.0f} live minutes, rate per 1,000 min {1000 * r_in:.2f} vs {1000 * r_org:.2f}, ratio {r_in / r_org:.2f}")


raid_rows = list(csv.DictReader(open(sys.argv[1])))
sessions = list(csv.DictReader(open(sys.argv[2])))
follows, t0_by_login, t0_by_id = collections.defaultdict(list), {}, {}
for r in csv.DictReader(open(sys.argv[3])):
    t = ts(r["followed_at"])
    follows[r["streamer"]].append(t)
    t0_by_login[r["streamer"]] = min(t0_by_login.get(r["streamer"], t), t)
    t0_by_id[r["twitch_user_id"]] = min(t0_by_id.get(r["twitch_user_id"], t), t)
control_comparison(raid_rows, t0_by_id)
window_share(raid_rows, sessions, follows, t0_by_login)
