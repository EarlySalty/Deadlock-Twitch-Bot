# Impersonation guard

## Goal
Stop `deadlock_germany` (id 1505528697) and lookalikes from posing as the Deutsche Deadlock Community with private 6v6 invitations.

- Deterministic detection in `tb-chat/src/impersonation_guard.rs` (no LLM): `private_6v6_pitch`, `lookalike_login`, registry flag `impersonates_community`.
- Partner channels: timeout 600 s via `ModerationEngine::timeout_and_cleanup_with_evidence` (safe list, autoban audit) plus one public statement per channel and chatter per 30 min, for registry impersonators and non-community 6v6 pitches.
- Non-partner channels (scout path): never time out; one warning per channel and chatter per 12 h, at most 10 per hour, for registry impersonators, and for lookalike logins or accounts on our global ban list (`twitch_chatter_global_ban`) that send a 6v6 pitch (user request 2026-10-16: not only the one account). Sent with the bot user token through a send-only policy; skipped with one warning if `user:write:chat` is missing.
- Pitch detection needs the 6v6 form, a private/tournament/match cue and a real invitation (bock/lust/wer kommt/hast du/habt ihr/join/mitmachen/einladung and similar) or a short question like "6v6 privat turnier?". Topic talk such as "die 6v6 lobby hat ewig geladen" never matches.
- The scout path skips every channel the partner pipeline handles (active partner or chat EventSub roster from `select_eventsub_subscription_broadcasters`), so no foreign warning lands on top of the partner timeout. A failed lookup skips the warning.
- Partner statements go through `SuppressionGuardChatApi`, so the manual partner opt-out applies.
- User rules 2026-10-16, both paths (partner and scout), on top of the community exemption:
  - Regulars are never affected: a chatter seen in that same channel on at least 3 distinct earlier Berlin days within the last 60 days (`twitch_chat_messages` by streamer and chatter login, `category_chat_messages` by `room_user_id` and chatter id). Registry crew accounts are never regulars. A failed history lookup counts as regular.
  - Action (partner timeout plus statement, foreign warning) only when the sender is a `CREW_REGISTRY` account or a registry account was seen in that same channel within the last 24 h. Sightings are kept in memory per guard instance and channel id (the radar log is throttled per account and is not a reliable sighting record); after a restart the gate starts closed. Without a sighting the outcome is `no_crew_sighting`: radar log and alert only, no action.
- Throttled and gated outcomes are logged at debug and persisted as radar alerts, not actions.
- Radar log records the real action in `action_taken`; the 24 h alert throttle only limits the Discord alert, not the action or the log row.
- Config switch `bot.impersonation_guard_enabled`, default on.

## Acceptance
- Unit tests for detection (invitation required), foreign single warning, foreign silence in partner channels, partner timeout plus single statement, community members untouched, regulars exempt in both paths, no action without a crew sighting, action after a sighting in the same channel, sighting in another channel does not count.
- Formatter check, clippy, tb-chat and tb-bot tests green; existing crew radar tests unchanged.
- Merge gate ALLOW, merged to main.
