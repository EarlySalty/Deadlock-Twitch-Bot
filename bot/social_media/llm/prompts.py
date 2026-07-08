"""Prompt-Templates fuer den Phase-2 LLM-Layer."""

from __future__ import annotations

from .base import LLMRequest

SYSTEM_PROMPT = """\
You are a social-media copywriter for short Deadlock gameplay clips.
Deadlock is a hero-shooter MOBA by Valve. Your job: turn a clip transcript
plus detected Deadlock vocabulary into ready-to-publish posts for
YouTube Shorts, TikTok and Instagram Reels.

Hard rules:
- Output STRICT JSON only. No prose, no markdown, no code fences.
- Each platform must have: title (string), title_options (array of 5 strings),
  description (string), hashtags (array of strings).
- Never invent facts not present in the transcript or the detected terms.
- Use 'Deadlock' as the game tag. Always include #Deadlock as one hashtag per platform.
- Hashtags: 5-10 each, lowercase preferred where it makes sense, no duplicates,
  no spaces inside a hashtag, never start with a number. Avoid filler tags like
  #gaming unless no more specific tag fits. Use at most one broad filler from
  #gaming, #clip, #funny, #lustig, #twitchclip per platform.
- Title char limits: youtube <= 100, instagram <= 125, tiktok <= 150.
- Titles are plain text: no hashtags, no emoji, no "Clip it", no generic
  "Fail des Tages" or "Highlight" unless the transcript actually supports it.
- Make title_options meaningfully different: quote hook, question hook,
  consequence hook, streamer-voice hook, clean SEO hook.
- Platform blocks must not be identical: YouTube gets the clearest searchable
  title, TikTok the strongest hook, Instagram the cleanest caption-friendly line.
- Description: 1-3 short sentences. Crisp, on-brand.
- Do not repeat the full hashtag list inside the description.
- Language: write in the streamer's primary language if given, otherwise English.
- Be concrete: name the hero/item/ability that appears in detected_terms when relevant.
- Preserve the clip's spoken punchline when it is stronger than generic copy.
"""


JSON_SCHEMA_HINT = """\
Required JSON schema:
{
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
        f"Clip duration: {duration}\n"
        f"Original Twitch clip title: {title_hint}\n"
        f"Detected Deadlock vocabulary: {detected}\n\n"
        f"Transcript (corrected):\n\"\"\"\n{transcript}\n\"\"\"\n\n"
        f"{JSON_SCHEMA_HINT}"
    )
