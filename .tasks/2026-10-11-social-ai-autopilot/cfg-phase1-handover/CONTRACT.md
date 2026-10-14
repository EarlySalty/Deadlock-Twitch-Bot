# CFG contracts

- Main configuration remains `/var/lib/deadlock-twitch/config/bot.toml`; productive Social AI reads use the installed BotConfigSnapshot.
- Provider, price version, model and rates must match the trusted main section. Frozen checkpoints cannot self-certify prices or serialize the private trust marker.
- Media-only loading validates its own roots and resources without depending on AI-only configuration.
- Writes share the existing editor lock, SHA-256 expected revision, metadata checks, atomic rename, fsync and previous-file backup. Unrelated configuration and comments remain unchanged.
- Live migration copies precisely the previous release values and leaves `social_ai.runtime.brain` absent. Apply precedes activation of the main-TOML loader release.
- Admin settings use an explicit allowlist. DSNs, tokens, keys and arbitrary configuration objects are neither catalog entries nor API values. Existing database connection controls remain compatible.
- Admin authentication precedes config access. Writes retain the existing CSRF/Origin protection, reject stale revisions and validate a complete candidate before persistence.
- Nullable fields remove the explicit value and show Dienststandard. Integer wire values do not round through JavaScript numbers. Fields report restart activation rather than implying immediate effect.
- Active values come from actual service snapshots or are marked unconfirmed. A saved file with a different fingerprint is not evidence of the bot's active values.
- No paid AI/STT, publishing, real streamer-setting edits, autopilot enablement, Brain activation, external user-data transfer or unrelated service changes.
