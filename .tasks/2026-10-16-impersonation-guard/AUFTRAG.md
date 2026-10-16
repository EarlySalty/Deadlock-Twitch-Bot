# Impersonation guard

## Goal
Stop `deadlock_germany` (id 1505528697) and lookalikes from posing as the Deutsche Deadlock Community with private 6v6 invitations.

- Deterministic detection in `tb-chat/src/impersonation_guard.rs` (no LLM): `private_6v6_pitch`, `lookalike_login`, registry flag `impersonates_community`.
- Partner channels: timeout 600 s via `ModerationEngine::timeout_and_cleanup_with_evidence` (safe list, autoban audit) plus one public statement per channel and chatter per 30 min, for registry impersonators and non-community 6v6 pitches.
- Non-partner channels (scout path): never time out; one warning per channel and chatter per 12 h, at most 10 per hour, for registry impersonators, and for lookalike logins or accounts on our global ban list (`twitch_chatter_global_ban`) that send a 6v6 pitch (user request 2026-10-16: not only the one account). Sent with the bot user token through a send-only policy; skipped with one warning if `user:write:chat` is missing.
- Radar log records the real action in `action_taken`; the 24 h alert throttle only limits the Discord alert, not the action or the log row.
- Config switch `bot.impersonation_guard_enabled`, default on.

## Acceptance
- Unit tests for detection, foreign single warning, partner timeout plus single statement, community members untouched.
- Formatter check, clippy, tb-chat and tb-bot tests green; existing crew radar tests unchanged.
- Merge gate ALLOW, merged to main.
