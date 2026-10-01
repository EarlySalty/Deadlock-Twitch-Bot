# Paket 2a: Bot-Betriebsschalter

| Alter Leser | Typisierter Anschluss | Bestehender Default/Vertrag |
|---|---|---|
| main TB_DB_MIGRATE | bot.run_database_migrations | true; eigenständiger Bot-Wert |
| Dashboard-main TB_DB_MIGRATE | dashboard.run_database_migrations | true; eigener Dienstwert, nicht mit Bot-Override zusammengelegt |
| main TB_HIGHLIGHT_CLIPPER_ENABLED | bot.highlight_clipper_enabled | false, keine neue Aktivierung |
| main TB_MONITORING_POLL_ENABLED | bot.monitoring_poll_enabled | false, Helix-Voraussetzung unverändert |
| main TB_CLIP_FETCHER_ENABLED | bot.clip_fetcher_enabled | false, Helix-Voraussetzung unverändert |
| main TB_EVENTSUB_RECEIVER_PORT | bot.eventsub_receiver_port | 8786, positive u16 |
| main SCAM_GUARD_DISCORD_CHANNEL_ID | bot.scam_guard_discord_channel_id | 1374364800817303632, positive i64-Grenze vor Weitergabe |
| main STREAMER_GUILD_ID → MAIN_GUILD_ID | bot.live_ping_guild_id | 1289721245281292288, positive u64; nur Live-Ping, andere Guild-Fallbacks bleiben getrennt |
| main TWITCH_ALERT_MENTION | bot.alert_mention | None; optionaler unverändert weitergereichter Text |
| main TWITCH_DISCORD_REF_CODE | bot.discord_ref_code | None; optionaler unverändert weitergereichter Text |

Alte Bool-String-Synonyme und ungültige Werte mit stillen Fallbacks werden nicht als Strings in TOML weitergeführt: echte Bool-/Integer-Typen, falsche Typen/Werte stoppen den Start. Optionale Texte maximal4096 Byte, keine NUL-Zeichen. Bestehende Defaults sind Schema-Defaults, keine behaupteten produktiven Werte.

`ClipFetchTask::start_if_enabled` hatte keinen Aufrufer im Workspace. Der alte zweite ENV-Einstieg ist entfernt; tatsächlich gestartet wird weiterhin über `start()` nach der Freigabe in Bot-main. Keine neue Taskaktivierung, keine neuen Modelle/Connectoren.

Noch getrennt: Raid-Redirect hat in Chat-Clip-Port Default leer, in Main festen Callback. Guild-ID im Token-Lifecycle hat keinen Default, andere Pfade haben Community-Default. Diese Werte werden nicht anhand des Namens blind zusammengeführt.

Prüfungen: gezielter Configtest für getrennte Dienstmigrationen, unveränderte ausgeschaltete Tasks und Typ-/Wertefehler erfolgreich; Bot-/Dashboard-Debugcheck erfolgreich. Vollrollout und produktive Werte weiterhin offen.
