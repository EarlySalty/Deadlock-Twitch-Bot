# Scam ban + Anthropic Haiku switch (Twitch bot)

Branch `fix/scam-ban-haiku`, worktree `/home/nathanael/.worktrees/ttb-scam-ban-haiku`, base origin/main 726d5982e.

## Goal

- Scam pitches and befriending-to-Discord contact drops get banned, not just warned.
- Central bot LLM runs on Anthropic `claude-haiku-5-5` with the Social AI key (ANTHROPIC_API_KEY + ANTHROPIC_WORKSPACE_ID); the Fireworks account is suspended (HTTP 412, billing).
- Real cases that must be caught: frienollly (growth pitch), tifeh90_ ("gaming together" + "On DC:novas8"), umeasilte ("I'm cluthub on dc"), deenexus_lux ("linking up on dc. deenexus"), hhsjsnjnj ("want to try some rounds" + "dc: meydix_02").

## Done (committed, not pushed)

- 4b89fbc86: tb-llm Anthropic endpoint via TB_LLM_CENTRAL_PROVIDER/MODEL (run scripts default anthropic/claude-haiku-5-5), Messages API adapter without temperature; pipeline bans an AI-confirmed scam pitch (`ban_confirmed_scam_pitch`).
- c8651c4f5: conversation_scam contact-drop detection (`is_contact_drop`) triggers the judge on one message and lowers the AutoBan threshold to suggestion_floor; dangling `,"}` repair in both judge parsers (Haiku emits it sometimes).
- Haiku probes: conversation judge says scam 0.7 to 0.9 on all real cases, clean on harmless German "dc" chat.
- Root cause why nothing fired: conversation scam guard had 0 decisions in 7 days because every LLM call failed on suspended Fireworks (logged only at debug).

## Gate round 1: BLOCK (Opus, Sol out of quota). Fix all in one new commit

1. BLOCKING `selection.rs` `endpoint_for`: respect `FIREWORKS_ONLY_USE_CASES` (ricky_crew_review, outreach_shadow, promo_pitch) before returning the Anthropic endpoint; add a test. Same root cause for the script defaults in run_tb_bot_service.sh:155 and run_tb_dashboard_service.sh:114; once the exclusion exists, the defaults are the intended switch.
2. NIT: `anthropic_endpoint` must return None when `keys::anthropic_api_key()` is None, so it falls back to Fireworks.
3. NIT: raise ANTHROPIC_DEFAULT_MAX_TOKENS (4096); map `stop_reason == "max_tokens"` to `finish_reason: "length"` in `anthropic_als_openai` and warn.
4. NIT: debug/warn once when json_object/temperature/reasoning_off are dropped for Anthropic.
5. NIT: update stale docs in pipeline.rs (~217, ~1034, ~1662) to say a confirmed scam pitch is a permanent ban.
6. NIT plus the new hhsjsnjnj case: the colon/dot forms in `CONTACT_DROP` must not hit "dc: bin gleich zurück". Rule: the handle is the last token of the message OR contains a digit, `_` or `.`. Add negative tests for "dc: bin gleich zurück", "dc: brb" and positive tests for "dc: meydix_02".
7. NIT: `threshold_source = "contact_drop"` only when the lowered threshold is actually below the regular one.

Then: gate-learning ledger lines (round 1: 1 untested BLOCKING class "Fireworks-only use cases bypassed by central provider"), rustfmt (toolchain binary), `cargo-slot clippy -p tb-llm -p tb-chat --all-targets -- -D warnings`, `cargo-slot test -p tb-llm --lib` and `cargo-slot test -p tb-chat --lib -- conversation_scam scam_pitch pipeline` (pipeline test `regression_pipeline_partner_id_und_cache_trotz_namenswechsel` fails on origin/main too), commit, `gate-review --repo /home/nathanael/.worktrees/ttb-scam-ban-haiku --base origin/main`.

## After ALLOW

`git push origin HEAD:main`, `ops/twitch-release-build <sha>`, `sudo -n /usr/local/bin/deploy-twitch-release <sha>`, check `/proc/<MainPID>/exe` for both units, journal shows `Conversation-Scam-Judge-Entscheidung` and llm_usage rows with provider anthropic, then delete branch, remote branch and worktree. Tell the user that Fireworks is suspended for billing, which also hits other bots and the resolver timer.
