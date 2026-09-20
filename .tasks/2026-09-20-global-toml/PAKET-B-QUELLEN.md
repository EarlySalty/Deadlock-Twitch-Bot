# Bündel B: Bot-, Chat-, Raid- und Monitoring-Verbraucher

## B1: Startschalter, Raid-Zeiten und MCP

Alle Werte stehen in `[bot]`. Der Start übergibt sie explizit an bestehende Konstruktoren/Ports; Bibliotheken lesen keine zweite Betriebsquelle. Keine zusätzlichen Hintergrundaufgaben, keine neuen KI-Aufrufe/Modelle.

| Alte Quelle | Neues Feld | Alter Default / Semantik |
|---|---|---|
| TB_CHAT_ENABLED | chat_enabled | false, bestehende Token-/Helix-Voraussetzungen bleiben |
| TB_CHAT_PERSIST_ALL_GAMES | chat_persist_all_games | true, vorhandener expliziter Tracker-Konstruktor |
| LFG_PITCH_ENABLED | lfg_pitch_enabled | true, bestehender Responder-Bool |
| TB_GOLIVE_TIPS_ENABLED | golive_tips_enabled | false, im ChatHooks-Snapshot |
| TB_IRC_LURKER_ENABLED | irc_lurker_enabled | false, Startfreigabe |
| OUTREACH_SHADOW_ENABLED | outreach_shadow_enabled | false; Retention bleibt unabhängig wie vorher |
| SMALLTALK_LOOP_ENABLED | smalltalk_loop_enabled | false |
| SMALLTALK_LOOP_LIVE_SEND | smalltalk_loop_live_send | false, eigenständiger Schalter |
| TB_AUTO_RAID_OFFLINE_GRACE_SECS | auto_raid_offline_grace_seconds | 5, u64-Dauer |
| TB_AUTO_UNRAID_WINDOW_SECS | auto_unraid_window_seconds | 90.0, endlich und >=0 |
| TB_FLIP_REPEAT_WINDOW_SECS | flip_repeat_window_seconds | 600.0, endlich und >=0 |
| TB_FLIP_PAUSE_SECS | flip_pause_seconds | 43200.0, endlich und >=0 |
| TB_MCP_HOST/PORT | mcp_host / mcp_port | Loopback 127.0.0.1:8892, geprüfter positiver Port |

MCP, EventSub (127.0.0.1), Internal-API, Dashboard und STT werden vor dem Start auf gleiche konfigurierte Listeneradressen geprüft. Das verhindert Teilstarts durch offensichtliche Portkollisionen. Externe bereits belegte Ports erfordern weiterhin Laufzeitprüfung.

`ChatterTracker::new` bleibt ein reiner Bibliothekskonstruktor mit bisherigem Defaulttrue; der einzige produktive Aufrufer nutzt jetzt `with_persist_all_games` mit dem TOML-Wert. Unbenutzter LFG-ENV-Getter entfernt. Alte Stringparser für Outreach/Smalltalk existieren ausschließlich als bisherige Testreferenzen.

Prüfung: Config-Global-Suite26 grün; `cargo check -p tb-bot --tests` grün nach Anpassung der bestehenden Default-Referenzprüfung. Bot-/Dashboard-Normalcheck zuvor grün. Bündel wird zusammengehörend erneut geprüft und gegatet; dieser Commit ist kein fertiger Rollout.
