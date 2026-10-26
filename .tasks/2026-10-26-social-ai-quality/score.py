#!/usr/bin/env python3
import argparse
import json
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
MANUAL = ["moment", "cut", "framing", "text_de"]
SUBTITLE_OFF = {"remove_subtitle_en", "de_remove_subtitle", "remove_subtitle_clip_en"}
NO_TEXT = {"de_no_text", "no_text_en"}
GERMAN = re.compile(r"\b(der|die|das|und|nicht|wurde|bleibt|ist)\b", re.I)


def kept_start(sig):
    kept = (sig or {}).get("kept") or []
    return kept[0][0] if kept else None


def effect(request_key, parent, entry):
    if entry.get("status") != "review":
        return False
    sig = entry.get("signature") or {}
    if request_key == "start_later":
        before, after = kept_start(parent), kept_start(sig)
        return before is not None and after is not None and 2000 <= after - before <= 4500
    if request_key == "new_title":
        return bool({"overlay", "titles"} & set(entry.get("changed") or []))
    if request_key == "shorter":
        before, after = (parent or {}).get("output_duration_ms"), sig.get("output_duration_ms")
        return bool(before and after and after <= before * 0.9)
    if request_key == "caption_end":
        return sig.get("subtitles_end_ms") is not None
    if request_key in SUBTITLE_OFF:
        return sig.get("subtitles_enabled") is False and sig.get("overlay") == (parent or {}).get("overlay")
    if request_key in NO_TEXT:
        return sig.get("subtitles_enabled") is False and ((sig.get("overlay") or {}).get("mode") == "none")
    return bool(entry.get("changed"))


def answer_score(request, entry, took_effect):
    summary = ((entry.get("proposal") or {}).get("summary") or "").strip()
    if not summary:
        return 0
    english_request = request["key"].endswith("_en")
    if english_request and GERMAN.search(summary):
        return 1
    claims_unchanged = re.search(r"unverändert|nicht umsetzen|lassen sich nicht|nicht möglich|cannot|unchanged", summary, re.I)
    if took_effect or (claims_unchanged and not entry.get("changed")):
        return 2
    return 1


def subtitle_target_score(result):
    entries = [e for e in result.get("revisions", []) if e["request"]["key"] in SUBTITLE_OFF]
    if not entries:
        return None
    hits = 0
    for entry in entries:
        before = (entry.get("parent_signature") or {}).get("overlay")
        after = (entry.get("signature") or {}).get("overlay")
        title_touched = entry.get("status") == "review" and "overlay" in (entry.get("changed") or []) and before != after
        hits += 0 if title_touched else 1
    return round(2 * hits / len(entries), 1)


def token_score(result):
    jobs = [result.get("root") or {}] + result.get("revisions", [])
    rows = [j.get("tokens") for j in jobs if j.get("tokens")]
    if not rows:
        return None
    plausible = 0
    for t in rows:
        real = (t.get("tokens_in") or 0) + (t.get("cache_creation") or 0) + (t.get("cache_read") or 0)
        plausible += 1 if real == 0 or (t.get("shown_input") or 0) >= 0.9 * real else 0
    return round(2 * plausible / len(rows), 1)


def render_score(result):
    root = result.get("root") or {}
    renders = [root] + [r for r in result.get("revisions", []) if r.get("status") == "review"]
    ok = [r for r in renders if (r.get("frames") or {}).get("duration_s")]
    if root.get("status") != "review" or not ok:
        return 0
    return 2 if len(ok) == len(renders) else 1


def score_clip(result, manual):
    parent = (result.get("root") or {}).get("signature")
    rows, answers = [], []
    for entry in result.get("revisions", []):
        request = entry["request"]
        took = effect(request["key"], entry.get("parent_signature") or parent, entry)
        rows.append((request["key"], took))
        answers.append(answer_score(request, entry, took))
        if entry.get("status") == "review":
            parent = entry.get("signature")
    scores = {key: manual.get(key) for key in MANUAL}
    scores["change_effect"] = round(2 * sum(t for _, t in rows) / len(rows), 1) if rows else 0
    scores["answer_match"] = round(sum(answers) / len(answers), 1) if answers else 0
    scores["render"] = render_score(result)
    scores["subtitle_target"] = subtitle_target_score(result)
    scores["tokens"] = token_score(result)
    return scores, rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run", required=True)
    args = parser.parse_args()
    run_dir = HERE / "runs" / args.run
    manual_path = run_dir / "manual-scores.json"
    manual = json.loads(manual_path.read_text()) if manual_path.exists() else {}
    keys = MANUAL + ["change_effect", "answer_match", "render", "subtitle_target", "tokens"]
    print("| clip | " + " | ".join(keys) + " | total | requests that took effect |")
    print("|" + "---|" * (len(keys) + 3))
    totals = {k: 0.0 for k in keys}
    count = 0
    for path in sorted(run_dir.glob("*.json")):
        if path.name in ("manual-scores.json", "EXCLUDED.json"):
            continue
        result = json.loads(path.read_text())
        scores, rows = score_clip(result, manual.get(path.stem, {}))
        total = sum(v or 0 for v in scores.values())
        effects = ", ".join(f"{k} {'yes' if t else 'no'}" for k, t in rows)
        print(f"| {path.stem} | " + " | ".join(str(scores[k]) for k in keys) + f" | {round(total, 1)} | {effects} |")
        for k in keys:
            totals[k] += scores[k] or 0
        count += 1
    if count:
        avg = {k: round(v / count, 2) for k, v in totals.items()}
        print("| **mean** | " + " | ".join(str(avg[k]) for k in keys) + f" | {round(sum(avg.values()), 2)} of {2 * len(keys)} | |")


if __name__ == "__main__":
    main()
