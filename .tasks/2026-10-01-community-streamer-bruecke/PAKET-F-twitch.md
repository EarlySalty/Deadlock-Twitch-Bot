# Paket F: Streamer-Scouting, Twitch-Seite (Deadlock-Twitch-Bot)

status: gebaut, Branch `wip/tb-clips-scout`
datum: 2026-10-01

## Was gebaut ist

- Migration `rust/migrations/20261001110000_scout_community_source.sql` (additiv):
  - `twitch_scout_candidates`: `source` (`auto`|`community`, CHECK, Standard `auto`), `suggested_by_discord_id`, `suggestion_reason`, `suggested_at`, `suggestion_count`, `partner_active_since`, `community_updated_at` (eindeutiger Teilindex mit `streamer_login` für `source = 'community'`).
  - `twitch_scout_community_suggestions`: jeder Vorschlag mit `idempotency_key` (UNIQUE), Kanal, Vorschlagendem, Grund und vergebenem Status. GRANTs: `twitchbot` SELECT/INSERT (+ Sequenz), `twitchdash` SELECT. Die bestehende Kandidatentabelle hatte nie eigene GRANTs; die neuen Spalten erben die Tabellenrechte.
- Fachlogik `rust/crates/tb-scout/src/community.rs`:
  - Reine Regeln: Login-Normalisierung (`@name`, `twitch.tv/name`), Discord-ID (15 bis 21 Ziffern), Idempotenz-Schlüssel (1 bis 128 sichtbare ASCII), Grund (Steuerzeichen raus, 500 Zeichen), Status aus dem Listenstand (Partner vor Sperre vor bekannt), Partnerzeit aus `partnered_at` (TEXT, mehrere Schreibweisen), Partnerstand.
  - `vorschlag_einreichen`: eine Transaktion unter Advisory-Lock. Wiederholung desselben Schlüssels liefert die damalige Antwort, Schlüssel mit anderem Kanal/anderer Person ist ein Konflikt. Prüft `twitch_partners` (jeder Status, per ID oder Login), Raid-Blacklist, Partner-Denylist, Pitch-Blacklist, aktive Recruitment-Suppression, globalen Bann (`RaidBlacklistStore::is_hard_banned`, wie Scout und Recruiting), vorhandene Kandidaten (Login oder ID) und laufende Outreach-Sperrfrist. Neu: Kandidat `vorgeschlagen`, `source = community`, erster Vorschlagender mit Grund. Bekannt: `suggestion_count` = verschiedene Vorschlagende, der erste bleibt.
  - `aktualisiere_partnerstand` und `liste_ergebnisse`: gleicht `partner_active_since` mit `twitch_streamers_partner_state` ab (wie Paket B: `is_partner_active = 1`), stempelt geänderte Zeilen streng monoton (`community_updated_at`), Seite ab Cursor.
- Endpunkte im `tb-internal-api` (`rust/crates/tb-internal-api/src/handlers/scout_community.rs`, Router in `lib.rs`), Auth wie alle internen Routen (`X-Internal-Token` + Loopback):
  - `POST /internal/twitch/v1/scout/community-suggestion` mit `{"twitch_login","suggested_by_discord_id","reason","idempotency_key"}`; Login per Helix (vorhandener App-Token-Client) auf die Twitch-User-ID. Antwort `{"status":"created"|"already_known"|"already_partner"|"blocked"|"not_found","twitch_user_id":...}`. 400 Formfehler, 409 `idempotency_conflict`, 503 ohne Helix.
  - `GET /internal/twitch/v1/scout/community-suggestions/outcomes?updated_since=&limit=`: je Community-Kandidat `twitch_user_id`, `twitch_login`, `suggested_by_discord_id` (erster), `suggested_at`, `suggestion_count`, `candidate_status`, `is_partner_active`, `partner_since`, `updated_at`; Cursor wie Paket B (`next_updated_since` unverändert zurückgeben).
- Kein Versand: der Vorschlag legt nur den Kandidaten an. `approved_ohne_dispatch` und damit die Outreach-Kette greifen erst nach der bestehenden Admin-Entscheidung.
- Admin-Ansicht: `GET /twitch/api/admin/scout/candidates` liefert je Kandidat zusätzlich `source`, `suggested_by_discord_id`, `suggestion_reason`, `suggested_at`, `suggestion_count` (`KandidatZeile` in `tb-scout/src/store.rs` erweitert). Ein Admin-Frontend für die Scout-Liste gibt es in diesem Repo nicht; wer die JSON-Liste anzeigt, sieht Quelle und Vorschlagenden.
- Doku: `docs/API.md` (Interne Twitch-API), `docs/DATABASE.md`.

## Nebenbei behoben

- Scout-Erkennung (`tb-scout/src/detector.rs`): `twitch_partner_outreach.cooldown_until` ist im migrierten Schema `TEXT`. Der Vergleich `cooldown_until > NOW()` scheiterte deshalb gegen die echte DB mit `operator does not exist: text > timestamp with time zone` (die Admin-Liste führt vor dem Lesen einen Scan aus). Jetzt `NULLIF(BTRIM(cooldown_until), '')::timestamptz > NOW()` wie in `tb-engagement` (`smalltalk_live_store`). Die handgeschriebenen Test-Tabellen in `tb-scout/tests/pg_tests.rs` und `admin_scout.rs` hatten `TIMESTAMPTZ` und haben das verdeckt; sie stehen jetzt auf `TEXT` wie in Produktion. Ein neuer Test lässt den Scan gegen das voll migrierte Schema laufen.

## Für Paket C (Deadlock-Bots)

- Punkte bekommt `suggested_by_discord_id` der Ergebniszeile, sobald `is_partner_active = true`. Das einmalige Vergeben (Ledger-Schlüssel z. B. `scout-partner-<twitch_user_id>`) liegt bei Paket C; eine Zeile kann mehrfach kommen (Partner, Pause, wieder Partner), `partner_since` ist dann neu.
- Nur Kandidaten mit `source = community` erscheinen. Wer einen schon von der Scout-Erkennung gefundenen Kanal vorschlägt (`already_known`), wird gezählt, aber nicht als Entdecker geführt.
- `partner_since` kann vor `suggested_at` liegen, wenn `partnered_at` in `twitch_partners` älter ist (z. B. reaktivierter Altpartner, der zum Zeitpunkt des Vorschlags nicht in `twitch_partners` stand). Paket C sollte `partner_since >= suggested_at` prüfen, falls das gewünscht ist.

## Tests

Gegen Wegwerf-Timescale (`timescale/timescaledb:2.17.2-pg16`), `TB_TEST_DATABASE_URL`, `TB_TEST_REQUIRE_DB=1`:

- `cargo test -p tb-scout`: 6 Unit (inkl. neu: Statusreihenfolge, Login-Eingaben, Discord-ID/Schlüssel/Grund, Partnerzeit und Partnerstand) + 7 PG-Tests (bestehend, mit Prod-Spaltentypen) grün.
- `cargo test -p tb-internal-api --lib scout_community`: 5 passed. Body-Prüfung; Auth 401/403, Formfehler 400, ohne Helix 503, Cursor-`limit` 400; Vorschlag legt Kandidaten an (Status, Quelle, Vorschlagender, Grund), Wiederholung idempotent, zweite Person zählt, gleiche Person nicht doppelt, Schlüssel-Konflikt 409, keine Outreach-Zeile und keine Freigabe, Scout-Scan gegen echtes Schema; Partner (auch archiviert), Denylist, globaler Bann, bekannter Kandidat bleibt `uebersprungen`/`auto`, unbekannter Login `not_found`; Ergebnisse mit Cursor über zwei Seiten, Partnerschaft erzeugt neue Zeile mit `partner_since` aus `partnered_at`, leere Folgeseite behält Cursor. Helix per wiremock.
- `cargo test -p tb-dashboard-api --lib admin_scout`: 7 passed (neu: Quelle und Vorschlagender sichtbar).
- `cargo test -p tb-db` (`--test-threads=1`): Schema-Snapshot ergänzt, alle grün.

## Offene Punkte

- Admin-Entscheidungen stempeln `community_updated_at` nicht neu; `candidate_status` in den Ergebnissen ist deshalb nur informativ und kann veraltet sein, bis sich der Partnerstand ändert.
- Wird ein Kanal umbenannt, sucht der Vorschlag Kandidaten per Login und ID; der Kandidaten-Schlüssel bleibt der alte Login.
- `not_found` wird nicht gespeichert; eine Wiederholung fragt Helix erneut.
