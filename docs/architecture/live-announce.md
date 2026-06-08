# live_announce/ — Architektur & Funktionsreferenz

> Pfad: `bot/live_announce/` · Stand: 2026-06-08 · 2 Dateien, ~776 Zeilen
>
> Teil der [Architektur-Doku](README.md). Verwandt: [monitoring.md](monitoring.md) (`_EmbedsMixin` rendert hiermit), [dashboard.md](dashboard.md) (Streamer konfiguriert das Template im Dashboard).

## 1. Zweck & Abgrenzung

`live_announce/` ist eine **reine Template-Engine** für konfigurierbare Discord-Go-Live-Ankündigungen. Sie nimmt eine vom Streamer gespeicherte Konfiguration (Embed-Felder, Buttons, Bilder, Mentions, Platzhalter) plus einen Stream-Kontext und rendert daraus das fertige Embed-Payload — inklusive Platzhalter-Ersetzung, Längen-Limits und Validierung.

Abgrenzung: Das Subsystem **rendert nur** — es spricht **nicht** mit Discord oder Twitch. Das eigentliche Posten/Tracking macht `monitoring/embeds_mixin.py`; die Konfiguration kommt aus dem Dashboard. Hier liegt keine I/O, keine DB, kein State — pure Funktionen + Dataclasses.

## 2. Einordnung & Abhängigkeiten

| Richtung | Beziehung |
|----------|-----------|
| **Wird genutzt von** | `monitoring/embeds_mixin.py` (rendert das Go-Live-Embed), Dashboard-Live-Konfig (Validierung/Default). |
| **Nutzt** | nur Standardbibliothek (`datetime`, `re`) + Konstanten; keine externen Dienste, keine DB. |
| **Externe Dienste** | keine. |

## 3. Dateien im Überblick

| Datei | Zeilen | Rolle |
|-------|-------:|-------|
| `template.py` | 728 | Template-Engine: Config-Dataclasses, Platzhalter-Rendering, Validierung, Defaults, Merge. |
| `__init__.py` | 48 | Re-Export der öffentlichen Template-Helfer. |

## 4. Datenfluss / Lebenszyklus

1. Der Streamer speichert im Dashboard eine Announcement-Konfig (JSON). `parse_config_json` + `LiveAnnouncementConfig.from_dict` machen daraus ein typisiertes Objekt; fehlende Felder kommen aus `default_live_announcement_config()` (via `deep_merge_config`).
2. Beim Go-Live baut `monitoring` einen Kontext (`build_template_context`/`build_stream_context`: Login, Titel, Spiel, Viewer, Uptime, Thumbnail-URL inkl. Cache-Buster).
3. `render_announcement_payload(config, context)` ersetzt Platzhalter (`{login}`, `{title}`, …) in allen Feldern, kappt auf die Discord-Limits (`_MAX_TITLE`, `_MAX_FIELDS`, …) und liefert das fertige Embed-Payload-Dict.
4. `validate_live_announcement_config` / `validate_config` prüfen die Konfig vorab (für das Dashboard) und melden Probleme als Pfad+Meldung.

## 5. Funktionsreferenz

### template.py
Konstanten: `TWITCH_BRAND_COLOR`, `TWITCH_ICON_URL`, `_PLACEHOLDER_RE`, Limits `_MAX_TITLE`/`_MAX_DESCRIPTION`/`_MAX_FIELDS`/`_MAX_FIELD_NAME`/`_MAX_FIELD_VALUE`.

*Config-Dataclasses* (je mit `from_dict`):
- `LiveAnnouncementConfig` (`from_dict`, `to_dict`) — die Gesamt-Konfig.
- Teile: `AnnouncementAuthor`, `AnnouncementField`, `AnnouncementFooter`, `AnnouncementButton`, `AnnouncementImages`, `AnnouncementMentions`.

*Rendering:*
- `build_template_context(streamer_login, stream, *, now=None) -> dict[str,str]` / `build_stream_context(*, login, stream, mention_role="", now=None)` — Platzhalter-Kontext aus dem Stream bauen.
- `render_placeholders(template, context) -> str` — `{…}`-Platzhalter ersetzen.
- `render_announcement_payload(config, context, *, now=None, cache_buster_seed=None) -> dict` — fertiges Embed-Payload.
- `shorten_text(text, max_length, *, suffix="...")` — auf Discord-Limit kürzen.
- `parse_embed_color(value, *, fallback=TWITCH_BRAND_COLOR) -> int` — Farbwert robust parsen.
- `_fmt_uptime(started_at, now)` / `_parse_started_at(value)` — Uptime-Anzeige.
- `_stream_thumbnail_url(raw_url, *, ratio, cache_buster, now, cache_buster_seed=None)` / `_stable_cache_buster_value(seed, *, now)` — Thumbnail-URL mit Cache-Buster (gegen Discord-Caching alter Vorschaubilder).
- `is_valid_http_url(url) -> bool`.

*Konfig-Handling:*
- `default_live_announcement_config() -> dict` — Standard-Template.
- `deep_merge_config(base, override) -> dict` — Streamer-Override über den Default mergen.
- `parse_config_json(raw) -> dict` — JSON robust parsen.
- `validate_live_announcement_config(config, *, context=None) -> list[str]` / `validate_config(config) -> list[_ValidationIssue]` — Konfig validieren (`_ValidationIssue` = Pfad + Meldung); `_to_template_compatible_dict` als Brücke.
- Coerce-Helfer: `_coerce_str`, `_coerce_bool`, `_coerce_int`.

## 6. Datenbank & externe Schnittstellen

Keine. Die gerenderte Konfig wird vom Dashboard gespeichert/geladen und von `monitoring` an Discord gesendet — `live_announce/` selbst ist seiteneffektfrei.

## 7. Stolperfallen / Besonderheiten

- **Reine Funktion, kein I/O:** Gut testbar; wer einen Bug im Embed sucht, prüft zuerst `render_announcement_payload` mit einem festen `now`/`context`, nicht das Discord-Posting.
- **Cache-Buster ist nötig:** Ohne den stabilen Cache-Buster im Thumbnail-URL zeigt Discord oft das alte Vorschaubild des vorherigen Streams — daher `_stable_cache_buster_value`.
- **Discord-Limits werden hart gekappt:** `_MAX_*`-Limits schneiden zu lange Felder ab; ein Template, das im Editor passt, kann nach Platzhalter-Ersetzung über das Limit laufen — `validate_*` warnt vorab.
- **Default + Merge:** Streamer-Konfigs sind Overrides über `default_live_announcement_config()`. Ein fehlendes Feld heißt „nimm den Default“, nicht „leer“.
