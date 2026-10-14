# Package CFG: Social AI config into the one main bot TOML

Read /home/nathanael/.worktrees/tb-social-coord/.tasks/2026-10-11-social-ai-autopilot/briefs-20261014/COMMON.md first; it applies fully.

User decision 2026-10-14: the Twitch bot has ONE TOML config, the main bot TOML (live: `/var/lib/deadlock-twitch/config/bot.toml`, edited through tb-config, see `rust/crates/tb-config/src/editor.rs`, already parsed as `BotConfigSnapshot`). Social AI must not have its own TOML files. Today it has `ops/social-ai/editor.toml` and `ops/social-ai/pricing-v1.toml`.

Goal:
- All Social AI settings (provider, pricing, runtime, resources, plan, original_preview, copy, stt, revisions, optional brain) live in a `[social_ai]` section (with subtables) of the main bot TOML and are loaded from there at runtime.
- `ops/social-ai/editor.toml` and `ops/social-ai/pricing-v1.toml` are deleted; no code loads or `include_str!`s a separate Social AI TOML any more. Test fixtures may build TOML strings inline.
- Model name, effort, timeouts and prices stay configuration, not hard-coded (CLAUDE.md rule). Keep the existing value checks (pricing trust check, limits validation) working against the new source.
- Deploy: `ops/systemd/install-twitch-release.sh` and its existing test `ops/systemd/test_deploy_twitch_pruefen.py` stop expecting the two files; the skills `.md` files stay as they are.
- Live: write the `[social_ai]` section into the live bot TOML with exactly the current editor.toml values, using the existing tb-config edit path (hash-checked, backup), not by hand-rewriting the file. NEVER print the bot TOML: it may contain secrets. Only print section headers or the `[social_ai]` part. Leave `[social_ai.runtime.brain]` absent (Brain maintenance is off). Read access needs `sudo -n`; if it fails, report the blocker.
- Order matters: the live section must exist before the release that reads it is started, otherwise the editor worker fails to start. Verify after deploy that the AI editor worker starts cleanly (journal) and the Social AI admin routes still answer.

Write paths: `rust/crates/tb-llm/src/social_editor/config.rs` (and its callers in tb-llm), `rust/crates/tb-social-media/src/ai_editor/runtime_config.rs` (and the load call sites), `rust/crates/tb-config/**` where a `[social_ai]` passthrough or edit support is needed, `ops/social-ai/*.toml`, `ops/systemd/install-twitch-release.sh`, `ops/systemd/test_deploy_twitch_pruefen.py`. Package AI is editing `ai_editor/model.rs`, `options.rs`, `pipeline_tests.rs` and `handlers/social_media_ai.rs` in parallel: do not touch those; if you must, keep the change minimal and merge fresh origin/main before the gate.

Branch `feat/social-cfg-bot-toml-20261014`, worktree `~/.worktrees/tb-social-cfg`. Final report at most 8 lines.

## Addendum (user, 2026-10-14): editable like the Discord bot "Betriebseinstellungen"

Reference the user pointed at: https://admin.deutsche-deadlock-community.de/#betrieb (Deadlock-Bots repo, read-only for you): `rust/crates/dl-core/src/settings_catalog.rs` (Catalog, Field, Kind, ChangeRequest, toml_value), `rust/crates/dl-core/src/admin_settings.rs`, the handler in `rust/crates/dl-dashboard/src/web.rs` (`/api/admin/betriebskonfiguration`) and the UI `service/static/operating-config.js`. One TOML per service, a typed field catalog (kind, min/max, nullable = "Dienststandard", choices, lists), grouped and searchable cards, save with validation, active value per service shown.

Phase 2 after phase 1 is live: build the same for the Twitch bot's main TOML. Typed catalog in tb-config covering the bot TOML sections including `[social_ai]`, read and change routes in tb-dashboard-api (admin only, CSRF/origin like the existing admin routes, writes through the existing hash-checked tb-config edit path with backup), and extend the Twitch admin page "Betriebseinstellungen" (`bot/admin_dashboard/src/pages/config/OperatingConfig.tsx`, today only database connections) to show and edit the catalog in the same structure and look as the reference. Secret-bearing fields (DSNs, tokens, keys) never appear in the catalog or API responses; keep the existing database-connection handling as it is. German UI text with real umlauts. Restart hint where a value only takes effect after restart. Screenshots at 390 and 1440 via Moli.
