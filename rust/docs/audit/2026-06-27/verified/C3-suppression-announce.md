# C3 Suppression/Announcement Verification

Auftrag: adversarialer Read-only-Check der Befunde `B1-019`, `B1-021`, `B1-022`, `B1-024` und `B3-P1-01`. Kein Git, keine Secrets, keine Tests/Services. Methode: statische Source-Inspection per `rg`, `sed`, `nl`.

## Verdicts

| ID | Verdict | Realistische Betroffenheit |
|---|---|---|
| B1-019 | CONFIRMED | Aktive Partner-Channels, die den Targeted-Promo-Slot erreichen. Durch Kanal-/Activity-Cooldowns nicht dauernd, aber jeder faellige Targeted-Slot kann trotz aktiver Promo-Suppression senden/attempten. |
| B1-021 | DOWNGRADE | Kein globaler Rust-Guard ist belegt; fuer `manual_partner_opt_out` ist die normale Chat-Pipeline aber oft schon ueber `is_partner_active`/Roster vorgefiltert. Reale Restbetroffenheit: nicht globale DB-Suppression und einzelne direkte Sendepfade ausserhalb/innerhalb der Pipeline. |
| B1-022 | CONFIRMED | Frische Auto-Bans in aktiven Partner-Channels. Manual-Opt-out ist upstream meist durch Partner-Aktivstatus gemindert; DB-Suppression/Timeout-Mute wird vor der Notice aber nicht zentral beachtet. |
| B1-024 | CONFIRMED | Wiederholte starke Service-Pitch-/Spam-Faelle waehrend User-Cooldown. Timeout und Alert passieren, aber der oeffentliche Eskalationstext fehlt jedes Mal. |
| B3-P1-01 | CONFIRMED | Alle Streamer mit gespeicherter alter/UI-Live-Announcement-Config im Rust-Auto-Post-Pfad. Custom Content/Titel/Felder/Rollen-ID greifen nicht; Defaults werden verwendet. Teilkorrektur: `button.label` wird bereits gelesen. |

## B1-019 Targeted-Promo Suppression

Python erzwingt den Guard zentral im Sendepfad:

- `_send_announcement` bricht bei `manual_partner_opt_out` und `suppression` ab: `bot/chat/moderation.py:1339`, `bot/chat/moderation.py:1346`.
- `_send_chat_message` macht dasselbe: `bot/chat/moderation.py:1430`, `bot/chat/moderation.py:1437`.
- `targeted_promo.py` nutzt genau diese Wrapper mit `source="promo"`: `bot/chat/targeted_promo.py:258`, `bot/chat/targeted_promo.py:260`, `bot/chat/targeted_promo.py:262`.

Rust hat keinen gleichwertigen zentralen Blocker:

- `ChatApi` hat keine Source-/Login-Parameter fuer Suppression: `rust/crates/tb-chat/src/api.rs:19`.
- `HelixChatClient::send_message` und `send_announcement` senden direkt ueber Helix: `rust/crates/tb-chat/src/moderation.rs:113`, `rust/crates/tb-chat/src/moderation.rs:143`.
- `TimeoutTrackingChatApi` dekoriert `send_message`, aber nur als Seiteneffekt fuer Drop-Tracking; `send_announcement` delegiert unveraendert: `rust/crates/tb-chat/src/timeout_tracking.rs:115`, `rust/crates/tb-chat/src/timeout_tracking.rs:175`, `rust/crates/tb-chat/src/timeout_tracking.rs:217`.
- `CombinedSuppression` wird in `chat_wiring.rs` nur an die PromoEngine gereicht: `rust/bin/tb-bot/src/chat_wiring.rs:549`, `rust/bin/tb-bot/src/chat_wiring.rs:554`, `rust/bin/tb-bot/src/chat_wiring.rs:567`.

Normale Promo-Pfade pruefen Suppression vor dem Send:

- Activity-Promo: `rust/crates/tb-chat/src/promos.rs:971`.
- Timeout-Pitch: `rust/crates/tb-chat/src/promos.rs:1017`.

Targeted-Promo nicht:

- Der Targeted-Slot wird vor Activity-Promo aufgerufen: `rust/crates/tb-chat/src/promos.rs:881`, `rust/crates/tb-chat/src/promos.rs:888`.
- User-targeted sendet direkt per `api.send_message`: `rust/crates/tb-chat/src/promos.rs:1790`.
- Global-targeted sendet direkt per `api.send_announcement`: `rust/crates/tb-chat/src/promos.rs:1833`.
- `rg` zeigt `self.suppression.is_muted` in `promos.rs` nur bei den nicht-targeted Pfaden (`972`, `1018`), nicht in `maybe_send_targeted_promo`.

Damit bleibt B1-019 bestaetigt. Zusaetzlich: der User-targeted Pfad schreibt Suppression nach `channel_settings`-Drop (`promos.rs:1792`), aber das ist nur reaktiv; der Global-Announcement-Pfad bekommt nur `bool` und kann keinen Drop-Code persistieren.

## B1-021 Globaler Outbound-Guard

FALSE waere nur haltbar, wenn Rust einen zentralen `ChatApi`-Wrapper haette, der `manual_partner_opt_out` und DB-Suppression vor allen Sends prueft. Den gibt es nicht:

- `TimeoutTrackingChatApi` ist explizit nur Drop-Tracking/Delegation: `rust/crates/tb-chat/src/timeout_tracking.rs:112`, `rust/crates/tb-chat/src/timeout_tracking.rs:175`, `rust/crates/tb-chat/src/timeout_tracking.rs:217`.
- `manual_partner_opt_out` taucht im generischen `ChatApi`/Decorator nicht auf; `rg manual_partner_opt_out` findet Chat-Send-Gating nur spezifisch in `admin_chat_action.rs`.
- Admin-Chat-Action ist ein eigener positiver Spezialfall: `partner_send_allowed` liest `manual_partner_opt_out` und gibt `Denied` zurueck: `rust/crates/tb-dashboard-api/src/handlers/admin_chat_action.rs:184`, `rust/crates/tb-dashboard-api/src/handlers/admin_chat_action.rs:212`.

Downgrade-Grund: `manual_partner_opt_out` ist fuer die normale Chat-Pipeline nicht voellig ungefiltert. Die Partner-State-View setzt `is_partner_active=0`, wenn `manual_partner_opt_out=1`: `rust/migrations/20260623150000_drop_manual_verified_columns.sql:45`. Die Pipeline klassifiziert Partner ueber `is_partner_active` und stoppt bei Non-Partnern nach Tracking: `rust/crates/tb-chat/src/channel_classifier.rs:117`, `rust/crates/tb-chat/src/channel_classifier.rs:129`, `rust/crates/tb-chat/src/pipeline.rs:445`.

Was bleibt:

- Direkte Antworten wie Commands und Fun-Responses gehen nur durch `api.send_message`, ohne DB-Suppression-Check: `rust/crates/tb-chat/src/commands.rs:606`, `rust/crates/tb-chat/src/fun_responses.rs:120`.
- Scam-Warnungen senden direkt im Detektor: `rust/crates/tb-chat/src/scam_pitch.rs:1055`.
- Go-live-Tipps/ReAuth/OAuth-Greeter senden ebenfalls ohne globalen DB-Suppression-Guard: `rust/bin/tb-bot/src/chat_wiring.rs:1148`, `rust/bin/tb-bot/src/reauth_reminder.rs:86`, `rust/bin/tb-bot/src/oauth_followups.rs:262`.
- Andere Spezialpfade haben eigene Suppression-Gates, z. B. Partner-Recruitment und Partner-Raid: `rust/bin/tb-bot/src/partner_recruit.rs:183`, `rust/bin/tb-bot/src/raid_arrival_wiring.rs:414`.

Verdict deshalb: nicht FALSE, aber downgrade der urspruenglichen Breite. Der fehlende zentrale Guard ist real; `manual_partner_opt_out` ist fuer den Haupt-Chatpfad upstream gemindert, DB-Suppression bleibt fragmentiert.

## B1-022 Auto-Ban-Notice

Python sendet die Auto-Ban-Notice ueber `_send_chat_message`, also mit zentralem Opt-out/Suppression-Guard: `bot/chat/moderation.py:1795`, `bot/chat/moderation.py:1796`, `bot/chat/moderation.py:1430`, `bot/chat/moderation.py:1437`.

Rust:

- `ModerationEngine::auto_ban_and_cleanup` sendet nach frischem Ban und `!silent` direkt `self.api.send_message`: `rust/crates/tb-chat/src/moderation.rs:461`, `rust/crates/tb-chat/src/moderation.rs:464`, `rust/crates/tb-chat/src/moderation.rs:468`.
- Dieser `api` ist nur der oben beschriebene `TimeoutTrackingChatApi`/`HelixChatClient`-Pfad; kein zentraler Suppression-Blocker.
- Der Auto-Ban wird aus der Partner-Pipeline aufgerufen: `rust/crates/tb-chat/src/pipeline.rs:456`, `rust/crates/tb-chat/src/pipeline.rs:701`, `rust/crates/tb-chat/src/pipeline.rs:725`.

B1-022 bleibt bestaetigt. Eingrenzung: Channels mit `manual_partner_opt_out=1` sind in der normalen Pipeline in der Regel kein `is_partner`; die konkrete Restbetroffenheit ist vor allem aktive Partner-Channels mit bestehender Outbound-Suppression/Timeout-Mute.

## B1-024 StrongTimeout Text

Python eskaliert mit Timeout und sendet danach den Chattext: `bot/chat/service_pitch_warning.py:940`, `bot/chat/service_pitch_warning.py:954`, `bot/chat/service_pitch_warning.py:958`.

Rust konstruiert den Text:

- `PitchDecision::StrongTimeout { text, duration }` existiert: `rust/crates/tb-chat/src/scam_pitch.rs:349`.
- Der Eskalationstext wird gebaut und in `StrongTimeout` gelegt: `rust/crates/tb-chat/src/scam_pitch.rs:1015`, `rust/crates/tb-chat/src/scam_pitch.rs:1038`.

Aber die Pipeline verwirft ihn praktisch:

- Match ist `PitchDecision::StrongTimeout { .. }`, nicht `text`: `rust/crates/tb-chat/src/pipeline.rs:486`.
- Ausgefuehrt werden `timeout_user` und Alert; kein `send_message` mit dem StrongTimeout-Text: `rust/crates/tb-chat/src/pipeline.rs:492`, `rust/crates/tb-chat/src/pipeline.rs:504`.
- `rg` findet keinen anderen Send von `StrongTimeout.text`; `send_message` in `scam_pitch.rs:1055` gehoert nur zu Public/StrongWarn.

B1-024 ist bestaetigt.

## B3-P1-01 Announcement UI-Config

Produktiver Rust-Pfad:

- `AnnounceConfigStore::load` liest `config_json` direkt aus `twitch_live_announcement_configs`: `rust/crates/tb-monitoring/src/announce/sink.rs:105`.
- Danach wird `AnnouncementConfig::from_json(&parsed)` aufgerufen: `rust/crates/tb-monitoring/src/announce/sink.rs:121`.
- `announce_live` rendert genau diese Config: `rust/crates/tb-monitoring/src/announce/sink.rs:222`, `rust/crates/tb-monitoring/src/announce/sink.rs:281`.

`AnnouncementConfig::from_json` erwartet das Template-Schema:

- Root `content_template`, Root `title_template`, Root `fields`: `rust/crates/tb-monitoring/src/announce/template.rs:126`, `rust/crates/tb-monitoring/src/announce/template.rs:156`, `rust/crates/tb-monitoring/src/announce/template.rs:161`.
- `fields[].name_template/value_template`: `rust/crates/tb-monitoring/src/announce/template.rs:130`, `rust/crates/tb-monitoring/src/announce/template.rs:131`.
- `mentions.use_streamer_ping_role` und `mentions.static_ping_role_ids`: `rust/crates/tb-monitoring/src/announce/template.rs:139`, `rust/crates/tb-monitoring/src/announce/template.rs:190`.

Das UI-/alte Schema liegt anders:

- Rust-`dashboard_config.rs` definiert `content`, `embed.title`, `embed.fields[].name/value`, `button.label`, `mentions.role_id`: `rust/crates/tb-monitoring/src/announce/dashboard_config.rs:16`, `rust/crates/tb-monitoring/src/announce/dashboard_config.rs:17`, `rust/crates/tb-monitoring/src/announce/dashboard_config.rs:26`, `rust/crates/tb-monitoring/src/announce/dashboard_config.rs:31`, `rust/crates/tb-monitoring/src/announce/dashboard_config.rs:48`.
- Dieselbe Datei sagt selbst, dass diese Form spaeter via `to_template_config` abgebildet werden soll: `rust/crates/tb-monitoring/src/announce/dashboard_config.rs:5`.
- `rg dashboard_config|parse_config_json` zeigt nur Modul/Tests; keine produktive Nutzung ausser `pub mod dashboard_config`: `rust/crates/tb-monitoring/src/announce/mod.rs:4`.

Folge:

- `content` wird nicht zu `content_template`; gerendert wird Default `{mention_role}`: `rust/crates/tb-monitoring/src/announce/template.rs:84`, `rust/crates/tb-monitoring/src/announce/template.rs:535`.
- `embed.title` wird nicht zu `title_template`; gerendert wird Default-Titel: `rust/crates/tb-monitoring/src/announce/template.rs:89`, `rust/crates/tb-monitoring/src/announce/template.rs:407`, `rust/crates/tb-monitoring/src/announce/template.rs:491`.
- `embed.fields[].name/value` wird nicht zu Root-`fields[].name_template/value_template`; Default-Felder bleiben: `rust/crates/tb-monitoring/src/announce/template.rs:94`, `rust/crates/tb-monitoring/src/announce/template.rs:497`.
- `mentions.role_id` wird nicht zu `static_ping_role_ids`; Rollenerlaubnis/Ping kommt nur aus `static_ping_role_ids` oder Partner-`live_ping_role_id`: `rust/crates/tb-monitoring/src/announce/template.rs:139`, `rust/crates/tb-monitoring/src/announce/sink.rs:224`, `rust/crates/tb-monitoring/src/announce/sink.rs:231`.
- Teilkorrektur gegen den Ausgangsbefund: `button.label` wird von `AnnouncementConfig::from_json` als Fallback fuer `label_template` gelesen: `rust/crates/tb-monitoring/src/announce/template.rs:185`, `rust/crates/tb-monitoring/src/announce/template.rs:186`, und dann im View-Spec verwendet: `rust/crates/tb-monitoring/src/announce/sink.rs:301`, `rust/crates/tb-monitoring/src/announce/sink.rs:308`.

Python normalisiert dagegen vor dem Rendering:

- Dashboard-Helper `_to_template_config`: `bot/dashboard/live/live_announcement_mixin.py:101`, Mapping `content` -> `content_template` bei `:135`, `embed.title` -> `title_template` bei `:146`, Felder bei `:118`, `mentions.role_id` -> `static_ping_role_ids` bei `:115`.
- Monitoring-Helper `_normalize_live_announcement_config`: `bot/monitoring/embeds_mixin.py:216`, Mapping `content` bei `:253`, Titel bei `:264`, Felder bei `:236`, Role-ID bei `:231`.

B3-P1-01 bleibt bestaetigt, mit der Einschraenkung, dass `button.label` nicht verloren geht.
