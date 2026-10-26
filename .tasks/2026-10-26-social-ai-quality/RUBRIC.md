# Social AI quality rubric

Nine items per clip, each 0 to 2, maximum 18. `score.py` computes the last five from the run JSON; the first four are judged from the contact sheets and stored in `runs/<run>/manual-scores.json`.

| item | 0 | 1 | 2 | how it is scored |
|---|---|---|---|---|
| moment | the key moment is missing or cut | moment present but buried | moment present and obvious early (hook or first seconds) | manual, contact sheet plus highlight list |
| cut | no trimming, dead air at start or end, or output longer than needed | some trimming, still slack | tight start and end around the moment | manual, kept intervals and output duration vs source |
| framing | gameplay unreadable on a phone | readable but small (blur pad band, tiny HUD) | gameplay fills the frame, action readable | manual, contact sheet |
| text_de | wrong, invented or unreadable title and copy | correct but bland, or wrong language for the audience | correct, specific, natural German | manual, metadata and overlay |
| change_effect | no request took effect | | every request took effect | automatic, 2 x share of the five requests whose expected effect is visible in the new plan |
| answer_match | no answer from the AI | answer exists but contradicts what happened, or is in the wrong language | answer describes exactly what changed or why nothing changed | automatic, mean over the five requests |
| subtitle_target | a subtitle request changed the hook title | | every "Untertitel" or "subtitle" request left the hook title alone | automatic, overlay before and after on the three subtitle requests; "Untertitel" and "subtitle" mean the speech captions, never the title |
| tokens | the shown token counts hide most of the real input | | shown input tokens are at least 90 % of tokens_in plus cache_creation plus cache_read in llm_usage | automatic, per job |
| render | root render failed | some revision renders missing | root and every new version rendered and downloadable | automatic, ffprobe on every version |

The expected effect per request is listed in `TESTSET.md`. A request that ends as `failed` never counts as taking effect, even if the refusal text is reasonable.

Additional hard rules from the 2026-10-26 report:

- "no text" or "kein Text" means every burned-in text is gone: speech captions and overlay title. The @handle watermark is judged on the contact sheet and noted, it is not in the plan signature.
- The AI chat answer must state what it actually changed. An answer that says captions cannot be removed while it removed the title instead scores 1 at most on answer_match.
- Expected effects need fields in the plan signature. Captions on or off is read from `subtitles_enabled` (or `subtitles.enabled`) in the edit plan; until the planner has such a field, the caption requests cannot take effect.
