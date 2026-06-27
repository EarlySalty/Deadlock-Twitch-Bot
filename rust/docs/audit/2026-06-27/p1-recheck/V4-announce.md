# V4 Recheck — P1.15/B3-P1-01 Go-Live-Announcement

Auftrag: Git-Archäologie + Intent-Recheck für die Frage, ob das fehlende UI→Template-Mapping im Rust-Go-Live-Announcement-Pfad Absicht ist oder eine Lücke. Keine Secrets, kein Checkout/Add/Commit/Push, keine Codeänderung.

## Verdict

**FIX-CLEAR.** Nicht `ALREADY-CLEAN`: Das aktuelle Rust-Dashboard schreibt heute **gar kein** Live-Announcement-Config-Schema in die DB; `rg` in `rust/crates/tb-dashboard-api/src` und `bot/dashboard_v2/src` findet für Live-Announcement nur den Redirect in `rust/crates/tb-dashboard-api/src/lib.rs:1187-1191`. Die gespeicherten produktiven Rows stammen damit weiterhin aus der alten/Python-UI-Fläche und sind UI-Schema (`content`, `embed.title`, `embed.fields[]`, `mentions.role_id`, `mentions.enabled`, `button.label`).

Nicht sauber als absichtliche Alt-Config-Aufgabe belegt: Der Builder wurde bewusst entfernt, aber der Sender und die Tabelle wurden nicht migriert oder entfernt. Der produktive Rust-Sendepfad lädt weiterhin `twitch_live_announcement_configs.config_json` und behauptet damit implizit Config-Support (`rust/crates/tb-monitoring/src/announce/sink.rs:105-124`), liest aber nur Template-Schema (`rust/crates/tb-monitoring/src/announce/template.rs:126-192`). Ergebnis: gespeicherte UI-Anpassungen greifen still nicht, außer `button.label`, das bereits als Fallback gelesen wird (`template.rs:185-188`).

## Git-Archäologie

Relevante `git log --oneline -- rust/crates/tb-monitoring/src/announce/`-Kette:

```text
24e336f feat(tb-monitoring/subs): Welle-3 ports (P2.10/56,P1.48)
2b60cdc feat(tb-monitoring): parity ports P1.2/P1.16/P1.17/P1.23/P1.25/P1.48
4fd0a8f tb-monitoring: Inbox-Retry-Wrapper, Offline-Seiteneffekt-Reihenfolge, Thumbnail-Fallback
d747d94 tb-monitoring: live-announcement Dashboard-Config-Helfer (LA-Dashboard Teil 1)
ed2c264 feat(tb-bot): Live-Ping-Rolle beim Go-Live automatisch anlegen
dad806f feat(rust): Schritt 4e — Go-Live-Announcements (Template-Engine + Broker-Sink)
```

`dad806f` brachte `sink.rs` + `template.rs`: `AnnounceConfigStore::load` las von Anfang an `config_json` direkt per `AnnouncementConfig::from_json`, also Template-Schema. `git blame -L 94,125 sink.rs` zeigt diese Ladezeilen komplett auf `dad806f`. Spätere Commits änderten Retry, Live-Ping-Rollenanlage und Mention-Sanitize, aber nicht die Schema-Normalisierung.

`d747d94` brachte `dashboard_config.rs` als "LA-Dashboard Teil 1". Committext und Dateikommentar sagen ausdrücklich: reine UI-Default-/Merge-/Parse-Helfer, **später** via `to_template_config` aufs Template-Schema abbilden (`dashboard_config.rs:3-6`). `git blame` zeigt die ganze Datei auf `d747d94`; es gab danach keine Implementierung des angekündigten Mappers.

`c8ada2a` ist der Cutover-Commit "Go-Live-Builder entfernt". Der Committext sagt: Seite/Whitelist raus, **Auto-Post-Sender (`tb-monitoring/announce`) unangetastet**. Der Diff routet `/twitch/live-announcement` nur zur SPA um (`lib.rs:1187-1191`) und dokumentiert im Backlog: Builder raus, Auto-Post + Rollen-Ping bleiben, DB nicht jetzt droppen (`rust/docs/cutover-backlog.md:25-38`).

`git log -S"_to_template_config"` findet nur alte Dashboard-/Doku-Historie (`a098e3e`, `72021cc`, `cfe3818`, `53f6490`), keinen Rust-Monitoring-Fix. `git log -S"dashboard_config"` findet `d747d94` und später nur Doku. `git log -S"content_template"` findet den ursprünglichen Template-Port (`dad806f`) und spätere Sanitize-/Doku-Änderungen, keinen UI→Template-Mapper.

Gegenindiz: `rust/docs/audit/_work/implementation-plan-2026-06-21.md:721-735` listet P1.15/P2.122 unter DROP, gekoppelt an "Live-Announcement Builder" und "nur EIN Default-Template". Das ist aber kein Code-/Migrationsbeleg, kein späterer Sender-Commit, und widerspricht der aktuellen 2026-06-27 Baseline/Report, die P1.15 wieder als offen/user-sichtbar führt (`00-baseline.md:104`, `REPORT.md:41`).

## Aktuelles DB-Schema / Dashboard-Schreibpfad

Aktuelles Rust-Dashboard:

- `rg "twitch_live_announcement_configs|live-announcement|content_template|title_template" rust/crates/tb-dashboard-api/src bot/dashboard_v2/src` findet nur `rust/crates/tb-dashboard-api/src/lib.rs:1189`.
- Es gibt keinen nativen `GET/POST /twitch/api/live-announcement/config`-Writer mehr.
- Admin-`announcements` ist ein anderer Pfad: globaler Promo-Text via `tb_analytics::promo_mode`, nicht Go-Live-Announcement-Config (`rust/crates/tb-dashboard-api/src/handlers/admin_announcements.rs`).

Legacy/Python-Schreibpfad:

- `bot/dashboard/live/live_announcement_mixin.py:314-334` merged `raw_cfg` in die UI-Default-Config und speichert genau dieses `cfg`.
- `_la_save` serialisiert `cfg` direkt als `config_json` (`live_announcement_mixin.py:632-640`).
- `_to_template_config` wird für Validierung/Preview genutzt, aber nicht vor INSERT persistiert (`live_announcement_mixin.py:101-180`).
- Python-Monitoring normalisiert beim Rendern erneut (`bot/monitoring/embeds_mixin.py:216-305`).

DB-Tabelle: `rust/migrations/20260601000000_baseline_schema.sql:998-1004` hat nur `config_json text`, also keinen Schema-Indikator. Rust muss daher beim Laden kompatibel entscheiden.

## Intent-Recheck

Bestätigt:

- Builder/UI bewusst entfernt: `rust/docs/cutover-backlog.md:25-31`, `rust/docs/audit/2026-06-27/00-baseline.md:45`, Commit `c8ada2a`.
- Auto-Post + Rollen-Ping bleiben: `rust/docs/cutover-backlog.md:27-32`.
- Core-Rendering bleibt live: `rust/docs/audit/_work/grillme-decisions-2026-06-15.md:421-422`.

Nicht bestätigt:

- Keine Stelle sagt belastbar: "existierende `twitch_live_announcement_configs` im UI-Schema werden absichtlich ignoriert" oder "alle alten Configs wurden/müssen auf Template-Schema migriert".
- Es gibt keine Rust-Dashboard-Schreibfläche, die heute Template-Schema erzeugt.
- Die Tabelle wird weiterhin von Rust-Sender und Internal-API gelesen; Internal-API liest sogar bewusst beide Button-Label-Varianten (`rust/crates/tb-internal-api/src/handlers/telemetry_routes.rs:248-287`, Tests `:1238-1247`).

Damit ist "Builder weg" kein ausreichender Fix-/FP-Beleg für P1.15. Die konkrete Lücke ist der Ladepfad, nicht die entfernte Builder-UI.

## Fix-Spec

Minimaler Fix ohne DB-Migration:

1. In `rust/crates/tb-monitoring/src/announce/dashboard_config.rs` einen reinen `to_template_config`/`normalize_live_announcement_config`-Helper implementieren.
2. In `AnnounceConfigStore::load` nach JSON-Parse:
   - Wenn `raw` ein UI-Schema ist (`embed` vorhanden), zuerst `parse_config_json(&text)` über UI-Defaults mergen, dann nach Template-Schema normalisieren.
   - Wenn kein `embed` vorhanden ist, Template-Schema unverändert an `AnnouncementConfig::from_json` geben.
3. Mapping portieren:
   - `content` → `content_template`, inklusive `{rolle}` → `{mention_role}`.
   - `embed.color` → `color`.
   - `embed.author.name/icon_mode/link_to_channel` → `author.name_template/icon_mode/link_to_stream` (`twitch_logo` → `twitch`).
   - `embed.title/title_link_enabled/description_mode/description/shorten` → `title_template/title_link_to_stream/description_mode/description_template/short_description`.
   - `embed.fields[].name/value/inline` → `fields[].name_template/value_template/inline`.
   - `embed.thumbnail.mode/custom_url` → `images.thumbnail_mode/thumbnail_url_template` (`custom_url` → `custom`).
   - `embed.image.use_stream_thumbnail/custom_url/format/cache_buster` → `images.image_mode/image_url_template/image_ratio/cache_buster`.
   - `embed.footer.text/icon_mode/timestamp_mode` → `footer.text_template/icon_mode/timestamp_mode`.
   - `button.enabled/label` → `button.enabled/label_template`; `url_template` bleibt im Rust-Renderer faktisch durch `referral_url` ersetzt.
   - `mentions.enabled` → `mentions.use_streamer_ping_role`; numerische `mentions.role_id` → `mentions.static_ping_role_ids`.
   - `allowed_editor_role_ids` ist Runtime-unrelevant für `AnnouncementConfig`, kann aber für spätere Dashboard-Ports weiter im UI-Schema bleiben.
4. Tests:
   - Pure Mapping-Test in `dashboard_config.rs` für Content/Titel/Felder/Image/Mentions/Button.
   - Integrationstest in `rust/crates/tb-monitoring/tests/announce.rs`, der eine echte UI-Schema-Row einfügt und beweist: Title/Content/Felder und `mentions.role_id` landen im Sendepayload/`allowed_role_ids`.
   - Regressionstest, dass bereits vorhandenes Template-Schema weiter unverändert funktioniert.

Optional später: einmalige DB-Migration/Backfill auf Template-Schema. Für den Go-Live-Fix ist der Ladepfad sicherer, weil die Tabelle keinen Schema-Tag hat und der Sender dann beide historischen Formen toleriert.
