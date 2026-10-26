#!/usr/bin/env python3
import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ACTIVE = {"queued", "preparing", "transcribing", "analyzing", "planning", "rendering", "validating"}
BASE = "/twitch/social-media/api/ai"


def api(method, path, body=None, out=None):
    env = dict(os.environ)
    if out:
        env["OUT"] = str(out)
    args = [str(HERE / "api.sh"), method, path]
    if body is not None:
        args.append(json.dumps(body, ensure_ascii=False))
    raw = subprocess.run(args, capture_output=True, text=True, env=env, timeout=600).stdout
    if out:
        return int(raw.strip() or 0), None
    text, _, code = raw.rpartition("\n")
    try:
        return int(code), json.loads(text) if text.strip() else None
    except ValueError:
        return int(code or 0), {"raw": text[:500]}


def db_json(sql):
    raw = subprocess.run([str(HERE / "db.sh"), "-At", "-c", sql], capture_output=True, text=True, timeout=120).stdout
    raw = raw.strip()
    return json.loads(raw) if raw.startswith(("{", "[")) else None


def log(msg):
    print(time.strftime("%H:%M:%S"), msg, flush=True)


def get_job(owner, job_id):
    return api("GET", f"{BASE}/jobs/{job_id}?twitch_user_id={owner}")[1]


def wait_job(owner, job_id, timeout_s):
    start = time.time()
    while True:
        job = get_job(owner, job_id) or {}
        status = job.get("status")
        if status == "awaiting_confirmation":
            log(f"job {job_id} awaiting confirmation, accepting proposal")
            code, _ = api("POST", f"{BASE}/jobs/{job_id}/proposal", {"twitch_user_id": owner, "accept": True})
            log(f"proposal accept http {code}")
        elif status and status not in ACTIVE:
            return job, round(time.time() - start)
        if time.time() - start > timeout_s:
            job["loop_timeout"] = True
            return job, round(time.time() - start)
        time.sleep(10)


def planning(job_id):
    return db_json(
        "select json_build_object('plan', checkpoints->'planning'->'result'->'edit_plan', "
        "'produced_video_id', produced_video_id, 'output_duration_ms', output_duration_ms, "
        "'status', status, 'failure_code', failure_code) from social_ai_jobs where id=%d" % int(job_id)
    ) or {}


def plan_signature(plan):
    edit = (plan or {}).get("plan") or {}
    inner = edit.get("plan") or {}
    return {
        "kept": [[k.get("start_ms"), k.get("end_ms")] for k in inner.get("kept_intervals") or []],
        "hook": [inner["hook"]["start_ms"], inner["hook"]["end_ms"]] if inner.get("hook") else None,
        "overlay": edit.get("overlay_title"),
        "subtitle_scale": edit.get("subtitle_scale_percent"),
        "titles": {p: c.get("title") for p, c in (edit.get("platforms") or {}).items()},
        "output_duration_ms": (plan or {}).get("output_duration_ms"),
        "subtitles_enabled": edit.get("subtitles_enabled", (edit.get("subtitles") or {}).get("enabled") if isinstance(edit.get("subtitles"), dict) else None),
        "plan_keys": sorted(edit.keys()),
    }


def plan_rejections(since):
    raw = subprocess.run(
        ["sudo", "-n", "journalctl", "-u", "deadlock-twitch-bot-rust", "--since", since, "--no-pager", "-o", "cat"],
        capture_output=True, text=True, timeout=60,
    ).stdout
    return [line.split("reason=", 1)[1].strip('"') for line in raw.splitlines() if "AI edit plan rejected" in line and "reason=" in line]


def diff(before, after):
    return sorted(k for k in after if before.get(k) != after.get(k))


def own_version(job):
    for version in job.get("versions") or []:
        if version.get("job_id") == job.get("id"):
            return version
    return None


def frames(owner, job_id, version, dest):
    dest.mkdir(parents=True, exist_ok=True)
    video = dest / f"v{version['id']}.mp4"
    if not video.exists():
        code, _ = api("GET", f"{BASE}/jobs/{job_id}/versions/{version['id']}/file?twitch_user_id={owner}", out=video)
        if code != 200:
            return {"download_http": code}
    probe = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(video)],
        capture_output=True, text=True,
    ).stdout.strip()
    duration = float(probe or 0)
    points = [0.5, duration * 0.33, duration * 0.66, max(duration - 1.0, 0)]
    sheet = dest / f"v{version['id']}_sheet.jpg"
    inputs = []
    for t in points:
        inputs += ["-ss", f"{t:.2f}", "-i", str(video)]
    filters = "".join(f"[{i}:v]scale=270:-2,setpts=PTS-STARTPTS[f{i}];" for i in range(len(points)))
    filters += "".join(f"[f{i}]" for i in range(len(points))) + f"hstack=inputs={len(points)}"
    subprocess.run(
        ["ffmpeg", "-y", "-v", "error", *inputs, "-filter_complex", filters, "-frames:v", "1", "-q:v", "6", str(sheet)],
        capture_output=True,
    )
    return {"duration_s": round(duration, 2), "sheet": str(sheet), "points_s": [round(p, 2) for p in points]}


def tokens(job_id):
    return db_json(
        "select json_build_object('shown_input', j.input_tokens, 'shown_output', j.output_tokens, "
        "'tokens_in', coalesce(sum(u.tokens_in), 0), 'tokens_out', coalesce(sum(u.tokens_out), 0), "
        "'cache_creation', coalesce(sum(u.cache_creation_input_tokens), 0), "
        "'cache_read', coalesce(sum(u.cache_read_input_tokens), 0), 'calls', count(u.id)) "
        "from social_ai_jobs j left join llm_usage u on u.social_ai_job_id in (j.id::text, 'manual:' || j.id) where j.id=%d "
        "group by j.input_tokens, j.output_tokens" % int(job_id)
    ) or {}


def create_root(clip, evidence, timeout_s):
    owner = str(clip["owner"])
    result = {"clip": clip, "revisions": []}
    log(f"create job for clip {clip['clip_db_id']} ({clip['key']})")
    code, body = api("POST", f"{BASE}/jobs", {"twitch_user_id": owner, "clip_db_id": clip["clip_db_id"]})
    result["create_http"] = code
    job_id = ((body or {}).get("job") or {}).get("id")
    if not job_id:
        result["create_error"] = body
        return result
    job, waited = wait_job(owner, job_id, timeout_s)
    result["root"] = {"job_id": job_id, "status": job.get("status"), "failure_code": job.get("failure_code"),
                      "waited_s": waited, "review_reasons": job.get("review_reasons"),
                      "highlights": job.get("highlights"), "original_duration_ms": job.get("original_duration_ms")}
    log(f"root job {job_id} -> {job.get('status')} after {waited}s")
    version = own_version(job)
    if not version:
        return result
    result["root"]["version_id"] = version["id"]
    result["root"]["metadata"] = api("GET", f"{BASE}/videos/{version['id']}/metadata?twitch_user_id={owner}")[1]
    result["root"]["frames"] = frames(owner, job_id, version, evidence / clip["key"])
    result["root"]["signature"] = plan_signature(planning(job_id))
    result["root"]["tokens"] = tokens(job_id)
    return result


def chain_parent(result):
    for entry in reversed(result["revisions"]):
        if entry.get("version_id") and entry["request"].get("from") != "root":
            return entry["job_id"], entry["version_id"], entry.get("signature")
    root = result["root"]
    return root["job_id"], root["version_id"], root.get("signature")


def run_request(clip, result, request, evidence, timeout_s):
    owner = str(clip["owner"])
    root = result["root"]
    if request.get("from") == "root":
        parent_job, parent_version, parent_sig = root["job_id"], root["version_id"], root.get("signature")
    else:
        parent_job, parent_version, parent_sig = chain_parent(result)
    log(f"revision '{request['key']}' on version {parent_version}")
    since = time.strftime("%Y-%m-%d %H:%M:%S")
    code, body = api("POST", f"{BASE}/jobs/{parent_job}/versions/{parent_version}/revision",
                     {"twitch_user_id": owner, "feedback": request["text"]})
    entry = {"request": request, "parent_version_id": parent_version, "http": code}
    rev_id = ((body or {}).get("job") or {}).get("id")
    if not rev_id:
        entry["error"] = body
        return entry
    rev, waited = wait_job(owner, rev_id, timeout_s)
    sig = plan_signature(planning(rev_id))
    entry.update({"job_id": rev_id, "status": rev.get("status"), "failure_code": rev.get("failure_code"),
                  "failure_message": rev.get("failure_message"), "waited_s": waited,
                  "proposal": rev.get("revision_proposal"), "review_reasons": rev.get("review_reasons"),
                  "plan_rejections": plan_rejections(since), "tokens": tokens(rev_id), "parent_signature": parent_sig,
                  "signature": sig, "changed": diff(parent_sig or {}, sig) if rev.get("status") == "review" else []})
    log(f"revision job {rev_id} -> {rev.get('status')} changed={entry['changed']}")
    new_version = own_version(rev) if rev.get("status") == "review" else None
    if new_version:
        entry["version_id"] = new_version["id"]
        entry["metadata"] = api("GET", f"{BASE}/videos/{new_version['id']}/metadata?twitch_user_id={owner}")[1]
        entry["frames"] = frames(owner, rev_id, new_version, evidence / clip["key"])
    return entry


def run_clip(clip, requests, evidence, timeout_s, target):
    result = json.loads(target.read_text()) if target.exists() else create_root(clip, evidence, timeout_s)
    target.write_text(json.dumps(result, ensure_ascii=False, indent=1))
    if not (result.get("root") or {}).get("version_id"):
        return result
    for job in [result["root"]] + result["revisions"]:
        if job.get("job_id") and "tokens" not in job:
            job["tokens"] = tokens(job["job_id"])
    done = {entry["request"]["key"] for entry in result["revisions"]}
    for request in requests:
        if request["key"] in done:
            continue
        result["revisions"].append(run_request(clip, result, request, evidence, timeout_s))
        target.write_text(json.dumps(result, ensure_ascii=False, indent=1))
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--run", required=True)
    parser.add_argument("--only", nargs="*")
    parser.add_argument("--evidence", default="/home/nathanael/.cache/social-ai-quality")
    parser.add_argument("--timeout", type=int, default=1800)
    args = parser.parse_args()
    config = json.loads((HERE / "testset.json").read_text())
    evidence = Path(args.evidence) / args.run
    out_dir = HERE / "runs" / args.run
    out_dir.mkdir(parents=True, exist_ok=True)
    for clip in config["clips"]:
        if args.only and clip["key"] not in args.only:
            continue
        run_clip(clip, config["requests"], evidence, args.timeout, out_dir / f"{clip['key']}.json")
    return 0


if __name__ == "__main__":
    sys.exit(main())
