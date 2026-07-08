"""Prompt-Templates fuer den Phase-2 LLM-Layer."""

from __future__ import annotations

from .base import LLMRequest

SYSTEM_PROMPT = """\
Du bist ein Social-Media-Editor fuer Gaming-Clips.
Deadlock ist ein Hero-Shooter-MOBA von Valve. Aus dem Clip-Transcript
erstellst du natuerliche, nicht cringe Social-Vorschlaege fuer TikTok,
Instagram Reels und YouTube Shorts.

Hard rules:
- Output STRICT JSON only. No prose, no markdown, no code fences.
- Keep all values concise. Reasons and moments are one sentence each.
- Each platform must have: title (string), title_options (array of 5 strings),
  description (string), hashtags (array of strings).
- The root object must also include: main_moment, content_angle, title_options
  (10 strings), best_title, best_title_reason, captions, hashtag_groups,
  pin_comments, calls_to_action and video_hooks.
- Never invent facts not present in the transcript or the detected terms.
- No fake claims like Weltrekord, Pro Player, Cheater or unfassbar unless the
  transcript proves it.
- Use 'Deadlock' as the game tag. Always include #Deadlock as one hashtag per platform.
- Hashtags: 5-10 each, lowercase preferred where it makes sense, no duplicates,
  no spaces inside a hashtag, never start with a number. Avoid filler tags like
  #gaming unless no more specific tag fits. Use at most one broad filler from
  #gaming, #clip, #funny, #lustig, #twitchclip per platform.
- Title char limits: platform titles <= 100; root title_options <= 70.
- Titles are plain text: no hashtags, no emoji, no "Clip it", no generic
  "Fail des Tages" or "Highlight" unless the transcript actually supports it.
- Make title_options meaningfully different: quote hook, question hook,
  consequence hook, streamer-voice hook, clean SEO hook.
- Platform blocks must not be identical: YouTube gets the clearest searchable
  title, TikTok the strongest hook, Instagram the cleanest caption-friendly line.
- Description: 1-3 short sentences. Crisp, on-brand.
- Do not repeat the full hashtag list inside the description.
- Language: all output fields must be German when language=de; common gaming
  terms like Parry, Ultimate, Hook, Fail or Clip may stay English.
- Be concrete: name the hero/item/ability that appears in detected_terms when relevant.
- Preserve the clip's spoken punchline when it is stronger than generic copy.
- Content angle must be one of: Clutch, Fail, Skill, Comedy, Rage, Spannung,
  Tutorial, Reaction, Unklar.
- If the transcript is too short, repetitive or unclear, use content_angle
  "Unklar" and write conservative suggestions without pretending a real moment.
"""


JSON_SCHEMA_HINT = """\
Required JSON schema:
{
  "main_moment": "...",
  "content_angle": "Fail",
  "title_options": ["...", "...", "...", "...", "...", "...", "...", "...", "...", "..."],
  "best_title": "...",
  "best_title_reason": "...",
  "captions": {
    "tiktok": ["...", "...", "..."],
    "instagram": ["...", "...", "..."],
    "youtube": ["...", "...", "..."]
  },
  "hashtag_groups": {
    "game_specific": ["#...", "#...", "#...", "#...", "#..."],
    "gaming_clip": ["#...", "#...", "#...", "#...", "#..."],
    "german": ["#...", "#...", "#..."]
  },
  "pin_comments": ["...", "...", "..."],
  "calls_to_action": ["...", "...", "..."],
  "video_hooks": ["...", "...", "..."],
  "youtube":   {"title": "...", "title_options": ["...", "...", "...", "...", "..."], "description": "...", "hashtags": ["..."]},
  "tiktok":    {"title": "...", "title_options": ["...", "...", "...", "...", "..."], "description": "...", "hashtags": ["..."]},
  "instagram": {"title": "...", "title_options": ["...", "...", "...", "...", "..."], "description": "...", "hashtags": ["..."]}
}
"""


def render_user_prompt(request: LLMRequest) -> str:
    streamer = request.streamer
    streamer_block = "Streamer: unknown"
    if streamer:
        bits: list[str] = [f"login={streamer.streamer_login}"]
        if streamer.display_name:
            bits.append(f"display_name={streamer.display_name}")
        if streamer.language:
            bits.append(f"language={streamer.language}")
        if streamer.persona_hint:
            bits.append(f"persona={streamer.persona_hint}")
        streamer_block = "Streamer: " + ", ".join(bits)

    detected = ", ".join(request.detected_terms) if request.detected_terms else "(none)"
    title_hint = request.clip_title or "(none)"
    game = request.game_name or "Deadlock"
    duration = (
        f"{request.duration_seconds:.0f}s"
        if request.duration_seconds is not None
        else "unknown"
    )

    transcript = request.transcript.strip() or "(empty transcript - rely on detected terms)"

    return (
        f"{streamer_block}\n"
        f"Game: {game}\n"
        "Platform: alle\n"
        "Clip-Art: Unklar - infer from transcript\n"
        "Zielgruppe: Casual Gamer, Competitive Gamer, deutschsprachige Gaming-Community\n"
        f"Clip duration: {duration}\n"
        f"Original Twitch clip title: {title_hint}\n"
        f"Detected Deadlock vocabulary: {detected}\n\n"
        f"Transcript (corrected):\n\"\"\"\n{transcript}\n\"\"\"\n\n"
        f"{JSON_SCHEMA_HINT}"
    )
