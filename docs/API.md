# API-Dokumentation

Alle HTTP-Routes des Systems. Zugriffslevel: **A** = Admin only, **S** = Streamer (eingeloggt), **P** = Public.

## Seiten (HTML)

### Auth
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/auth/login` | P | auth/auth_mixin.py |
| GET | `/twitch/auth/callback` | P | auth/auth_mixin.py |
| GET | `/twitch/auth/logout` | S | auth/auth_mixin.py |
| GET | `/twitch/auth/discord/login` | P | auth/auth_mixin.py |
| GET | `/twitch/auth/discord/callback` | P | auth/auth_mixin.py |
| GET | `/twitch/auth/discord/logout` | S | auth/auth_mixin.py |

### Dashboard / Analytics UI
| Methode | Pfad | Level | Datei | Bemerkung |
|---------|------|-------|-------|-----------|
| GET | `/twitch/` | P | routes_mixin.py | |
| GET | `/twitch/dashboard` | S | analytics/api_overview.py | Landing, liefert V2-Bundle |
| GET | `/analyse` | S | analytics/api_overview.py | Kanonische Analytics-SPA |
| GET | `/analyse/{path:.*}` | S | analytics/api_overview.py | SPA-Asset-Pfade |
| GET | `/twitch/dashboard-v2` | S | analytics/api_overview.py | Legacy-Alias → 301 auf `/analyse` |
| GET | `/twitch/dashboard-v2/{path:.*}` | S | analytics/api_overview.py | Asset-Durchleitung (kein Redirect) |
| GET | `/twitch/analyse` | S | analytics/api_overview.py | Legacy-Alias → 301 auf `/analyse` |
| GET | `/twitch/analyse/{path:.*}` | S | analytics/api_overview.py | Legacy-Alias → 301 auf `/analyse/{path}` |
| GET | `/twitch/verwaltung` | S | analytics/api_overview.py | |
| GET | `/twitch/pricing` | P | analytics/api_overview.py | |
| GET | `/social-media-admin` | A | analytics/api_overview.py | Eigene SPA |
| GET | `/social-media-admin/{path:.*}` | A | analytics/api_overview.py | Asset-Durchleitung |
| GET | `/twitch/stats` | A | routes_mixin.py | |
| GET | `/twitch/partners` | A | routes_mixin.py | |
| GET | `/twitch/market` | A | routes_mixin.py | |
| GET | `/twitch/demo` | P | analytics/api_overview.py | |
| GET | `/twitch/demo/dashboard-v2/{path:.*}` | P | analytics/api_overview.py | |
| GET | `/streamer/vergleich/` | P | website/vergleich/index.html | Öffentlicher, aggregierter Partner-Vergleich |
| GET | `/twitch/dashboards` | P | routes_mixin.py | Redirect |
| GET | `/twitch/dashboads` | P | routes_mixin.py | Redirect (Tippfehler-Compat) |

### Admin-Panel
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/admin` | A | routes_mixin.py |
| GET | `/twitch/live` | A | routes_mixin.py |
| GET | `/twitch/admin/announcements` | A | routes_mixin.py |
| POST | `/twitch/admin/announcements` | A | routes_mixin.py |
| GET | `/twitch/admin/roadmap` | A | routes_mixin.py |
| POST | `/twitch/admin/chat_action` | A | routes_mixin.py |
| POST | `/twitch/admin/manual-plan` | A | routes_mixin.py |
| POST | `/twitch/admin/manual-plan/clear` | A | routes_mixin.py |

### Streamer-Verwaltung (Admin)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| POST | `/twitch/add_any` | A | routes_mixin.py |
| POST | `/twitch/add_url` | A | routes_mixin.py |
| POST | `/twitch/add_login/{login}` | A | routes_mixin.py |
| POST | `/twitch/add_streamer` | A | routes_mixin.py |
| POST | `/twitch/remove` | A | routes_mixin.py |
| POST | `/twitch/verify` | A | routes_mixin.py |
| POST | `/twitch/archive` | A | routes_mixin.py |
| POST | `/twitch/discord_flag` | A | routes_mixin.py |
| POST | `/twitch/discord_link` | A | routes_mixin.py |
| POST | `/twitch/reload` | A | routes_mixin.py |

### Abo / Billing (Streamer)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/abbo` | S | routes_mixin.py |
| GET | `/twitch/abo` | S | routes_mixin.py (redirect) |
| GET | `/twitch/abos` | S | routes_mixin.py (redirect) |
| GET | `/twitch/abbo/bezahlen` | S | routes_mixin.py |
| POST | `/twitch/abbo/rechnungsdaten` | S | routes_mixin.py |
| GET | `/twitch/abbo/kuendigen` | S | routes_mixin.py |
| POST | `/twitch/abbo/kuendigen` | S | routes_mixin.py |
| GET | `/twitch/abbo/rechnungen` | S | routes_mixin.py |
| GET | `/twitch/abbo/rechnung` | S | routes_mixin.py |
| GET | `/twitch/abbo/stripe-settings` | S | routes_mixin.py |
| POST | `/twitch/abbo/promo-settings` | S | routes_mixin.py |
| POST | `/twitch/abbo/lurker-tax-settings` | S | routes_mixin.py |
| POST | `/twitch/abbo/promo-message` | S | routes_mixin.py |

### Legal (Public)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/legal/access` | P | admin/legal_mixin.py |
| POST | `/twitch/legal/verify` | P | admin/legal_mixin.py |
| GET | `/twitch/impressum` | P | routes_mixin.py |
| GET | `/twitch/datenschutz` | P | routes_mixin.py |
| GET | `/twitch/agb` | P | routes_mixin.py |

Hinweis: Das Human-Gate schützt `/twitch/impressum` und `/twitch/agb`. `/twitch/datenschutz` ist für Nutzer und OAuth-Prüfungen öffentlich erreichbar, trägt aber `noindex` und soll nicht in Suchergebnissen erscheinen. Details zu Secrets, Caddy-Allowlist, CSP und Troubleshooting stehen in [`docs/LEGAL_ACCESS_GATE.md`](LEGAL_ACCESS_GATE.md).

### Raid-Dashboard (Streamer)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/raid/auth` | S | routes_mixin.py |
| GET | `/twitch/raid/go` | S | routes_mixin.py |
| GET | `/twitch/raid/callback` | P | routes_mixin.py |
| GET | `/twitch/raid/requirements` | S | routes_mixin.py |
| GET | `/twitch/raid/history` | S | routes_mixin.py |
| GET | `/twitch/raid/analytics` | S | routes_mixin.py |

### Live-Announcement (Streamer)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/live-announcement` | S | routes_mixin.py |

### Affiliate (Streamer)
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/affiliate` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/links` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/stats` | S | affiliate/affiliate_mixin.py |
| POST | `/twitch/affiliate/links` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/click/{id}` | P | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/track` | P | affiliate/affiliate_mixin.py |
| POST | `/twitch/affiliate/settings` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/dashboard` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/export` | S | affiliate/affiliate_mixin.py |
| GET | `/twitch/affiliate/api/summary` | S | affiliate/affiliate_mixin.py |

---

## Analytics API (JSON)

Endpunkte unter `/twitch/api/v2/` erfordern Streamer-Authentifizierung, sofern sie nicht ausdrücklich unter `/public/` liegen.

### Public
| Methode | Pfad | Parameter | Inhalt |
|---------|------|-----------|--------|
| GET | `/twitch/api/v2/public/streamer-comparison` | `days=7|30|90` | Aggregierte Session-, Reichweiten-, Momentum- und Raid-Wirkungsdaten aktiver Partner |

Der öffentliche Vergleich enthält keine Einnahmen, Abos, Discord-IDs oder einzelnen Zuschauer. Rankings setzen je Zeitraum eine Mindest-Streamzeit voraus; Raid-Wirkung wird als zeitlicher Vorher-/Nachher-Hinweis mit Stichprobengröße ausgegeben, nicht als Kausalitätsbeweis. Zeitlich überlappende Raid-Messfenster werden nicht für die Wirkungswerte verwendet.

### Uebersicht & Stats
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/overview` | api_overview.py |
| GET | `/twitch/api/v2/monthly-stats` | api_overview.py |
| GET | `/twitch/api/v2/weekly-stats` | api_overview.py |
| GET | `/twitch/api/v2/hourly-heatmap` | api_overview.py |
| GET | `/twitch/api/v2/calendar-heatmap` | api_overview.py |
| GET | `/twitch/api/v2/category-comparison` | api_overview.py |
| GET | `/twitch/api/v2/category-leaderboard` | api_overview.py |
| GET | `/twitch/api/v2/category-timings` | api_overview.py |
| GET | `/twitch/api/v2/category-activity-series` | api_overview.py |
| GET | `/twitch/api/v2/rankings` | api_overview.py |
| GET | `/twitch/api/v2/streamers` | api_overview.py |
| GET | `/twitch/api/v2/session/{id}` | api_overview.py |

### Audience & Viewer
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/viewer-overlap` | api_overview.py |
| GET | `/twitch/api/v2/viewer-timeline` | api_overview.py |
| GET | `/twitch/api/v2/viewer-profiles` | api_overview.py |
| GET | `/twitch/api/v2/viewer-directory` | api_overview.py |
| GET | `/twitch/api/v2/viewer-detail` | api_overview.py |
| GET | `/twitch/api/v2/viewer-segments` | api_overview.py |
| GET | `/twitch/api/v2/audience-insights` | api_overview.py |
| GET | `/twitch/api/v2/audience-demographics` | api_overview.py |
| GET | `/twitch/api/v2/audience-sharing` | api_overview.py |
| GET | `/twitch/api/v2/follower-funnel` | api_overview.py |
| GET | `/twitch/api/v2/lurker-analysis` | api_overview.py |
| GET | `/twitch/api/v2/{streamer}/viewer-timeline` | api_viewer_timeline.py |
| GET | `/twitch/api/v2/{streamer}/viewer-timeline/profile` | api_viewer_timeline.py |

### Chat-Analyse
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/chat-analytics` | api_overview.py |
| GET | `/twitch/api/v2/chat-hype-timeline` | api_overview.py |
| GET | `/twitch/api/v2/chat-content-analysis` | api_overview.py |
| GET | `/twitch/api/v2/chat-social-graph` | api_overview.py |

### Performance & Coaching
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/tag-analysis` | api_overview.py |
| GET | `/twitch/api/v2/tag-analysis-extended` | api_overview.py |
| GET | `/twitch/api/v2/title-performance` | api_overview.py |
| GET | `/twitch/api/v2/watch-time-distribution` | api_overview.py |
| GET | `/twitch/api/v2/coaching` | api_overview.py |
| GET | `/twitch/api/v2/monetization` | api_overview.py |
| GET | `/twitch/api/v2/retention-curve` | api_overview.py |
| GET | `/twitch/api/v2/loyalty-curve` | api_overview.py |

### Raids
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/raid-analytics` | api_overview.py |
| GET | `/twitch/api/v2/raid-retention` | api_overview.py |

### KI
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/ai/analysis` | api_overview.py |
| GET | `/twitch/api/v2/ai/history` | api_overview.py |

### Stream-Report
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/stream-report` | api_overview.py |
| POST | `/twitch/api/v2/stream-report/rate` | api_overview.py |
| GET | `/twitch/api/v2/stream-report/ab-vote` | api_overview.py |
| POST | `/twitch/api/v2/stream-report/ab-vote` | api_overview.py |

### Roadmap
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/roadmap` | api_overview.py |
| POST | `/twitch/api/v2/roadmap` | api_overview.py |
| PATCH | `/twitch/api/v2/roadmap/{id}` | api_overview.py |
| DELETE | `/twitch/api/v2/roadmap/{id}` | api_overview.py |

### Experimental
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/twitch/api/v2/exp/overview` | api_overview.py |
| GET | `/twitch/api/v2/exp/game-breakdown` | api_overview.py |
| GET | `/twitch/api/v2/exp/game-transitions` | api_overview.py |
| GET | `/twitch/api/v2/exp/growth-curves` | api_overview.py |

### Sonstige API
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/api/v2/auth-status` | P | api_overview.py |
| GET | `/twitch/api/v2/internal-home` | A | api_overview.py |
| GET | `/twitch/api/v2/billing/catalog` | S | api_overview.py |
| GET | `/twitch/api/v2/affiliate/portal` | S | api_overview.py |
| GET | `/twitch/api/market_data` | A | routes_mixin.py |

### Live-Announcement API
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/api/live-announcement/config` | S | routes_mixin.py |
| POST | `/twitch/api/live-announcement/config` | S | routes_mixin.py |
| POST | `/twitch/api/live-announcement/test` | S | routes_mixin.py |
| GET | `/twitch/api/live-announcement/preview` | S | routes_mixin.py |

### Billing API
| Methode | Pfad | Level | Datei |
|---------|------|-------|-------|
| GET | `/twitch/api/billing/catalog` | S | routes_mixin.py |
| GET | `/twitch/api/billing/readiness` | A | routes_mixin.py |
| POST | `/twitch/api/billing/stripe/webhook` | P | routes_mixin.py |
| POST | `/twitch/api/billing/checkout-preview` | S | routes_mixin.py |
| POST | `/twitch/api/billing/checkout-session` | S | routes_mixin.py |
| POST | `/twitch/api/billing/invoice-preview` | S | routes_mixin.py |
| POST | `/twitch/api/billing/stripe/sync-products` | A | routes_mixin.py |

## Interne Twitch-API (tb-bot, Loopback)

Auth: `X-Internal-Token` + Loopback-Guard (wie alle Routen unter `/internal/twitch/v1`).

### Community-Punkte (Community-Streamer-Brücke, Paket B)
| Methode | Pfad | Datei |
|---------|------|-------|
| GET | `/internal/twitch/v1/community-points/viewers` | rust/crates/tb-internal-api/src/handlers/community_points.rs |
| GET | `/internal/twitch/v1/community-points/streamers` | rust/crates/tb-internal-api/src/handlers/community_points.rs |

Query: `updated_since=<RFC3339>` (optional, exklusiv), `limit=1..5000`
(Standard 1000, sonst 400). Antwort `{"rows":[...],"next_updated_since":"...","has_more":false}`,
sortiert nach `updated_at`, dann Schlüssel. `next_updated_since` ist das
`updated_at` der letzten Zeile (RFC3339 UTC, Mikrosekunden wenn nötig) und
wird unverändert als nächstes `updated_since` übergeben; bei leerer Seite
bleibt es der übergebene Wert (ohne Wert: `null`). Zeilen sind Tageswerte und
werden beim Lesen idempotent überschrieben.

Zuschauer-Zeile: `twitch_user_id`, `twitch_login`, `channel_twitch_user_id`,
`day` (YYYY-MM-DD, Europe/Berlin), `watch_minutes`, `chat_messages`,
`points_watch`, `points_chat`, `points_discovery`, `updated_at`.

Streamer-Zeile: `streamer_twitch_user_id`, `streamer_login`, `discord_user_id`
(oder `null`), `day`, `viewer_minutes`, `unique_viewers`, `raids_to_partners`,
`updated_at`.

### Streamer-Vorschläge aus der Community (Community-Streamer-Brücke, Paket F)
| Methode | Pfad | Datei |
|---------|------|-------|
| POST | `/internal/twitch/v1/scout/community-suggestion` | rust/crates/tb-internal-api/src/handlers/scout_community.rs |
| GET | `/internal/twitch/v1/scout/community-suggestions/outcomes` | rust/crates/tb-internal-api/src/handlers/scout_community.rs |

`POST` Body `{"twitch_login":"name","suggested_by_discord_id":"123…","reason":"…","idempotency_key":"…"}`
(`reason` optional, unbekannte Felder 400). `twitch_login` darf `@name` oder ein
`twitch.tv/name`-Link sein. Der Login wird per Helix auf die Twitch-User-ID
aufgelöst. Antwort 200 `{"status":"created"|"already_known"|"already_partner"|"blocked"|"not_found","twitch_user_id":"…"|null}`.

- `created`: neuer Scout-Kandidat (`status = vorgeschlagen`, `source = community`), Vorschlagender ist der erste.
- `already_known`: Kanal ist schon Kandidat (auch aus der Scout-Erkennung) oder in einer laufenden Outreach-Sperrfrist; der Vorschlag wird gezählt.
- `already_partner`: Kanal steht in `twitch_partners` (egal welcher Status).
- `blocked`: Raid-Blacklist, Partner-Denylist, Pitch-Blacklist, aktive Recruitment-Suppression oder globaler Bann.
- `not_found`: Helix kennt den Login nicht (nichts gespeichert).

Gleicher `idempotency_key` liefert die damalige Antwort; derselbe Schlüssel für
einen anderen Kanal oder eine andere Person: 409 `idempotency_conflict`.
Formfehler 400, Helix nicht verfügbar 503. Es wird nichts versendet; die
Admin-Freigabe (`/twitch/api/admin/scout/candidates`) bleibt der einzige Weg in
die Outreach-Kette.

`GET …/outcomes?updated_since=<RFC3339>&limit=1..5000`: Cursor wie bei den
Community-Punkten (`{"rows":[...],"next_updated_since":"…","has_more":false}`).
Je Community-Kandidat eine Zeile: `twitch_user_id`, `twitch_login`,
`suggested_by_discord_id` (erster Vorschlagender), `suggested_at`,
`suggestion_count`, `candidate_status`, `is_partner_active`, `partner_since`
(Partnerzeit aus `twitch_partners.partnered_at`, sonst Zeitpunkt der ersten
Beobachtung; `null`, solange kein aktiver Partner), `updated_at`. Der Aufruf
gleicht vorher den Partnerstand mit `twitch_streamers_partner_state` ab und
stempelt geänderte Zeilen neu; eine Zeile kommt also wieder, sobald der Kanal
aktiver Partner wird (oder es nicht mehr ist).

## Clip-Contest aus Twitch (Community-Streamer-Brücke, Paket E)

Streamer reichen Clips ihres Kanals für den wöchentlichen Clip-Contest im
Discord ein. Ein Dienst für beide Wege:
`rust/crates/tb-chat/src/clip_contest_submit.rs`.

| Weg | Auslöser | Datei |
|-----|----------|-------|
| Chat | `!clipcontest [clip-url]` im eigenen Kanal (nur Broadcaster und Mods) | rust/crates/tb-chat/src/commands.rs |
| Dashboard | `POST /social-media/api/clips/{clip_db_id}/clip-contest` (Social-Studio, Knopf "Für Clip-Contest einreichen") | rust/crates/tb-dashboard-api/src/handlers/social_media_clip_contest.rs |

Weitergabe an den Master-Broker von Deadlock-Bots über den vorhandenen
`BrokerRelay` (Basis-URL der Betriebskonfiguration, bestehendes interne Token):

`POST /internal/master/v1/clips/submit`
`{"source":"twitch","clip_url":"https://clips.twitch.tv/<id>","streamer_twitch_user_id":"456","streamer_login":"name","submitted_by_twitch_user_id":"456","title":"...","idempotency_key":"twitch-clip-<clip_id>"}`
mit `X-Idempotency-Key` = `idempotency_key`. Erwartet wird der Broker-Envelope
`{"ok":true,"result":{"status":"accepted"|"duplicate"|"rejected","submission_id":123,"reason":null}}`.

Regeln: Kanal aktiver Partner; Clip-URL nur `clips.twitch.tv/<slug>` oder
`(www.|m.)twitch.tv/<kanal>/clip/<slug>`; Clip muss per Helix `GET /clips?id=`
existieren und zum Kanal gehören; ohne URL der jüngste Clip der laufenden
Session (`twitch_clip_command_events`, `twitch_clips_social_media`); höchstens
3 Einreichungen je Kanal und Berliner Tag; eine laufende Einreichung desselben
Clips wird nicht doppelt gesendet.

Dashboard-Antwort `{"status":"...","message":"...","clip_url":...}`. HTTP 200
für `accepted` und `already_in`; 422 `rejected`/`not_twitch_clip`, 404
`clip_not_found`, 403 `foreign_clip`/`not_partner`, 429 `rate_limited`, 409
`in_flight`, 503 `broker_unavailable`/`twitch_unavailable`/`unavailable`.
