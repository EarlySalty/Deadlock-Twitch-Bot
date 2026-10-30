# Social Media Dashboard v2: agent chat | Grillme

Legend: BUILD-P0/P1/P2, DROP, KLAEREN, MODERN. This file is authoritative once complete.

## Not up for debate (user brief, 2026-10-30)

- v2 is built alongside; v1 stays as it is for now.
- Centre is an agent chat focused on social media and clips, with the action on a panel left or right of it.
- Starts with an interview that produces a style guide, ideally with one reference clip showing "this is how I want it".
- Work is processed top to bottom.

## Inventory (origin/main cad827177, live DB 2026-10-30)

- Usage: 5 channels, 139 AI jobs, all in the last 30 days: 87 review, 51 failed, 1 awaiting_confirmation. 104 produced videos.
- Free-text revision already exists: 105 feedback events, 16 proposals accepted, 10 versions accepted, 10 rejected (`social_ai_revision_events`).
- Per-channel learning exists: `social_ai_personal_preferences` (5 rows), fed from those events (`ai_editor/learning.rs`). Global skills are in `skill_registry.rs`, with admin pages.
- Gemini observer, auto cam and Auto-Pilot scheduling and publishing exist in `tb-social-media`.
- Subtitles: the frontend search found only a burn-in on/off option, and no text editing. Subtitle text comes from STT.
- Editor: `bot/dashboard_v2/src/pages/SocialMedia.tsx`, `components/socialmedia/`.

## Community feedback to place (2026-10-30)

- F1 Timosius: clip category should sort newest first. Cause not traced yet.
- F2 Timosius: cam cannot be clicked when editing a clip. The cam button is disabled in `blur_pad` and `fill` modes (`LayoutEditor.tsx:693`); likely cause, not reproduced.
- F3 Luckymakky: add or edit subtitles.

## Agenda

- A Session flow: the first and the returning visit, what the agent opens with, what "top to bottom" works through. OPEN
- B Autonomy: what the agent may do alone, what needs a click, posting. OPEN
- C Style guide: interview content, reference clip source, per channel or per platform, editing it later. OPEN
- D Chat capabilities: which actions, including precise edits such as subtitles and cam. OPEN
- E Action panel: what is visible, how progress and "what happens when" is shown. OPEN
- F Access and rollout: who gets v2, entry point, relation to v1 and when v1 goes. OPEN
- G Cost and model: agent runtime, limits per channel. OPEN
- H Scope: F1 to F3 in v1 or only in v2, and the first shippable slice. OPEN

## Decisions

