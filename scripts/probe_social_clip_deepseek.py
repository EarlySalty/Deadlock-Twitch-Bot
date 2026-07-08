#!/usr/bin/env python3
"""One-clip probe: Twitch clip -> OpenAI Whisper transcript -> Fireworks DeepSeek copy."""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import re
import subprocess
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Probe one existing Twitch clip through Whisper + Fireworks DeepSeek."
    )
    parser.add_argument("source", nargs="?", help="Twitch clip URL or local video file.")
    parser.add_argument("--streamer", default="earlysalty")
    parser.add_argument("--range", default="7d", choices=("24hr", "7d", "30d", "all"))
    parser.add_argument("--title", default=None)
    parser.add_argument("--game", default="Deadlock")
    parser.add_argument("--language", default="de")
    parser.add_argument(
        "--model",
        default=None,
        help="DeepSeek model path, default accounts/fireworks/models/deepseek-v4-pro.",
    )
    parser.add_argument("--workdir", default="data/social_clip_probe")
    parser.add_argument("--keep-video", action="store_true")
    return parser.parse_args()


async def main() -> int:
    from bot.social_media.llm.base import LLMRequest, StreamerProfile
    from bot.social_media.llm.deepseek import DeepSeekProvider
    from bot.social_media.transcription import (
        correct_transcript,
        get_transcriber,
        load_all_vocab_safe,
        transcribe_clip,
    )

    args = parse_args()
    workdir = Path(args.workdir)
    workdir.mkdir(parents=True, exist_ok=True)

    source = args.source or await latest_clip_url(args.streamer, args.range)
    video_path = await local_video(source, workdir)

    transcriber = get_transcriber("openai_api")
    transcript = await transcribe_clip(video_path, engine=transcriber)
    vocab = load_all_vocab_safe() if os.getenv("TWITCH_ANALYTICS_DSN") else []
    correction = correct_transcript(transcript.text, vocab=vocab)

    result = {
        "source": source,
        "local_video": str(video_path),
        "created_at": datetime.now(UTC).isoformat(),
        "transcript": {
            "engine": transcript.engine,
            "model": transcript.model,
            "language": transcript.language,
            "duration_seconds": transcript.duration_seconds,
            "text": transcript.text,
            "segments": transcript.segments_as_dicts(),
        },
        "correction": {
            "text": correction.corrected,
            "detected_terms": list(correction.detected_terms),
            "replacements": list(correction.replacements),
        },
    }

    exit_code = 0
    try:
        provider = DeepSeekProvider(model=args.model)
        response = await provider.generate(
            LLMRequest(
                transcript=correction.corrected,
                detected_terms=tuple(correction.detected_terms),
                streamer=StreamerProfile(streamer_login=args.streamer, language=args.language),
                clip_title=args.title,
                game_name=args.game,
                duration_seconds=transcript.duration_seconds,
            )
        )
        result["deepseek"] = {
            "model": response.model,
            "cost_usd_estimate": response.cost_usd_estimate,
            "youtube": _platform(response.youtube),
            "tiktok": _platform(response.tiktok),
            "instagram": _platform(response.instagram),
        }
    except Exception as exc:
        result["deepseek_error"] = str(exc)
        exit_code = 3

    stem = safe_stem(source)
    json_path = workdir / f"{stem}.json"
    md_path = workdir / f"{stem}.md"
    json_path.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    md_path.write_text(render_markdown(result), encoding="utf-8")
    if not args.keep_video and str(video_path).startswith(str(workdir)):
        video_path.unlink(missing_ok=True)

    print(f"json={json_path}")
    print(f"markdown={md_path}")
    if "deepseek" in result:
        print(f"youtube_title={result['deepseek']['youtube']['title']}")
        print("tags=" + " ".join(result["deepseek"]["youtube"]["hashtags"]))
    else:
        print(f"deepseek_error={result['deepseek_error']}")
    return exit_code


def _platform(value: Any) -> dict[str, Any]:
    return {
        "title": value.title,
        "title_options": list(value.title_options),
        "description": value.description,
        "hashtags": list(value.hashtags),
    }


async def latest_clip_url(streamer: str, range_value: str) -> str:
    url = f"https://www.twitch.tv/{streamer}/clips?filter=clips&range={range_value}"
    data = await run_json(["yt-dlp", "--no-update", "--flat-playlist", "--dump-single-json", url])
    entries = data.get("entries") or []
    if not entries:
        raise RuntimeError(f"no clips found for {streamer} ({range_value})")
    return str(entries[0]["url"])


async def local_video(source: str, workdir: Path) -> Path:
    if is_url(source):
        out = workdir / f"{safe_stem(source)}.mp4"
        await run_checked(["yt-dlp", "--no-update", "-f", "best", "-o", str(out), source])
        return out
    path = Path(source)
    if not path.exists():
        raise FileNotFoundError(source)
    return path


async def run_json(cmd: list[str]) -> dict[str, Any]:
    output = await asyncio.to_thread(
        subprocess.check_output,
        cmd,
        stderr=subprocess.PIPE,
        text=True,
    )
    return json.loads(output)


async def run_checked(cmd: list[str]) -> None:
    def _run() -> None:
        subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    await asyncio.to_thread(_run)


def is_url(value: str) -> bool:
    return value.startswith("https://") or value.startswith("http://")


def safe_stem(value: str) -> str:
    tail = value.rstrip("/").rsplit("/", 1)[-1] or "clip"
    tail = re.sub(r"[^A-Za-z0-9._-]+", "-", tail).strip("-")
    return tail[:80] or "clip"


def render_markdown(result: dict[str, Any]) -> str:
    lines = [
        "# Social Clip Probe",
        "",
        f"Quelle: {result['source']}",
        "",
        "## Transcript",
        "",
        result["correction"]["text"] or result["transcript"]["text"] or "(leer)",
        "",
        "## DeepSeek",
        "",
    ]
    deepseek = result.get("deepseek")
    if not isinstance(deepseek, dict):
        lines.append(f"Fehler: {result.get('deepseek_error', 'nicht ausgefuehrt')}")
        return "\n".join(lines).rstrip() + "\n"
    for platform in ("youtube", "tiktok", "instagram"):
        item = deepseek[platform]
        title_options = item.get("title_options") or []
        lines.extend(
            [
                f"### {platform}",
                "",
                f"Title: {item['title']}",
                "",
            ]
        )
        if title_options:
            lines.extend(["Titeloptionen:", *[f"- {option}" for option in title_options], ""])
        lines.extend(
            [
                item["description"],
                "",
                " ".join(item["hashtags"]),
                "",
            ]
        )
    return "\n".join(lines).rstrip() + "\n"


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
