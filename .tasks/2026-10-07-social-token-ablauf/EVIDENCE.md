# Prüfnachweise

## Statusfehler-Fix nach dem zweiten Code-BLOCK

Der weitere frische Fixer hat den vorhandenen Worktree bei `21631a2d` übernommen. Graphify wurde zuerst im Worktree versucht; dort fehlt der lokale Graph. Die globale Graphify-Abfrage und die anschließende Aufrufersuche belegen den Dashboard-Handler als einzigen produktiven Aufrufer des Statuslesers. Der gemeinsame interne Credential-Leser reicht SQL-Fehler als `Result` weiter. Die bestehenden optionalen Credential-Aufrufer behalten ihren Vertrag, der Statusleser reicht dagegen Fehler aus beiden SQL-Abfragen bis zur API durch. Der Handler protokolliert den SQL-Fehler und liefert die vorhandene Antwort HTTP 500 mit `platform_status_failed`, ohne Plattformliste. Kein neuer UI-Pfad und keine neue Versandstrecke.

Die PostgreSQL-Regressionen entfernen gezielt die Metadata-Spalte `refresh_expires_at`, nachdem ein verschlüsselter Zugang erfolgreich gelesen wurde. Für TikTok, YouTube und Instagram bleibt der erste Credential-Zugriff erfolgreich; der Metadata-Zugriff liefert SQLSTATE `42703`, und der echte API-Handler liefert HTTP 500 statt `connected: false`. Ein leeres, lesbares Auth-Verzeichnis bleibt HTTP 200 mit drei getrennten Plattformen. Ein fehlendes Auth-Verzeichnis scheitert bereits beim ersten SQL-Zugriff mit SQLSTATE `42P01` und ebenfalls HTTP 500. Die feste Prüfuhr des Rust-Statuslesers bleibt `2026-10-07T12:00:00Z`.

| Prüfung | Ergebnis |
| --- | --- |
| Social-Media- und DB-Suites | 357 passed, 0 failed, 0 ignored, 0 filtered, Exit 0 |
| Vollständige API-Suite | 1310 passed, 22 failed, 0 ignored, 0 filtered, Exit 101, Laufzeit 1011,28 Sekunden; identische Fehlerliste zur erhaltenen Baseline 1309/22, ein neuer bestandener API-Test |
| Betroffene Social-Media-Handler in der Vollsuite | 54 passed, 0 failed; einschließlich gezielt fehlgeschlagenem Metadata-Lesezugriff |
| Bot-Adapter mit PostgreSQL und lokalem Broker | 2 passed, 0 failed, 0 ignored, 322 filtered, Exit 0 |
| Online-SQLx-Prüfung für tb-social-media, alle Targets | Exit 0 gegen dieselbe Wegwerf-DB |
| Clippy, dieselben vier Pakete, alle Targets | Exit 0, Laufzeit 7 Minuten 20 Sekunden; vorhandene Warnungen bleiben |
| Dashboard-Vertrag und i18n | 27 passed, 0 failed, 0 skipped, Exit 0 |
| Formatprüfung der drei betroffenen Rust-Dateien | Exit 0 |
| Workspace `cargo fmt --all --check` | Exit 1, dieselben 148 nicht geänderten Dateien; keine der drei betroffenen Dateien |
| Dashboard-Build | Exit 0, Artefakt `index-C1sEecg0.js` |
| Chromium am gebauten Dashboard, fünf Zustände | Exit 0; drei unbekannte Plattformzustände nach HTTP 500, keine Verbindungs- oder Trennaktionen, keine Seitenfehler, Dokumentbreite 1440 bei Viewport 1440 |

Neue Browserprobe: `metadata-read-error.png`. Die vier bisherigen Ablaufproben wurden mit demselben Skript erneut geprüft und gespeichert. `ui-proof.json` enthält fünf DOM-Proben; kein Produktionszugang wurde verändert. Reproduktion: `node .tasks/2026-10-07-social-token-ablauf/ui-proof.mjs`.

Die Rust-Aufrufe entsprechen den unten erhaltenen Prüfzeilen mit `/home/nathanael/.cargo/bin/cargo --manifest-path rust/Cargo.toml`, `SQLX_OFFLINE=true`, beiden DSNs für `tb_social_token_ablauf` auf Port 33045, `-j 2`, `--include-ignored --test-threads=1`. Neue Logs beginnen mit `status-fixer-`. Die DB-Verfügbarkeit wurde vor dem Start mit `SELECT current_database(), current_user` geprüft. Migration und `PROD-MIGRATION.sql` sind bytegleich zum übernommenen Stand. SHA384 der Migration: `8b50646d1227c0276ea13fe20bd6315d9703aaf20b35b268daaad9424b646b6e82981ef65f70f861099aac9032e13ca0`.

Der unveränderte Merge-Gate hat HEAD `4305407d` mit `gpt-6.1-sol` freigegeben: `[gpt-6.1-sol] ALLOW: No blocking defect found in the supplied changes.` Log: `gate-status-fixer-round-3.log`. Die nicht blockierende Anmerkung zum veralteten TODO ist behoben. Die neue Nutzerregel vom 7. Oktober 2026 (`claude-config` main `60137bf`, Ablauf Schritt 7) bleibt für weitere BLOCK-Runden gültig: je ein frischer nativer Subagent, dasselbe Gate-Modell, kein neuer T3-Thread, spätestens nach fünf erfolglosen Runden ein echter Blocker. Der abweichende Remote-Feature-Branch wurde nicht überschrieben; der Fix ist auf `fix/social-token-ablauf-r2` gesichert. Merge, Produktionsmigration und Deploy folgen jetzt gemäß ursprünglichem Auftrag.

TESTNACHWEIS[TW-1]: 1696 passed, 0 ignored | Baseline: 22 rot
WIRKUNGSPRUEFUNG[WP-1]: 1 Befund behoben | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 1/1 geprüft
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: technische Nachweise und Commit

## Folgefix nach dem ersten Code-BLOCK

Der frische Fixer hat den bestehenden Worktree übernommen. Sweep, Statusleser und Dashboard verwenden für nicht erneuerbare TikTok-/YouTube-Verbindungen das Access-Ende. Das Anbieterfeld `refresh_expires_at` wird nicht ersetzt. Migration und Produktionshilfe sind unverändert. Die Folgerunde des bestehenden Merge-Gates hat mit demselben Modell `gpt-6.1-sol` erneut BLOCK geurteilt, diesmal wegen versteckter Datenbankfehler im Metadata-Statusleser. Der bestätigte Befund steht in `REVIEW.md`, die Übergabe an einen weiteren frischen Fixer in `BRIEFING-FIXER-STATUS.md`. Produktion ist noch unverändert.

| Prüfung | Ergebnis |
| --- | --- |
| Social-Media- und DB-Suites vor und nach Rebase | 355 bzw. 356 passed, jeweils 0 failed, 0 ignored, 0 filtered; vier zusätzliche Tests im Folgefix, ein weiterer Test aus aktuellem main |
| Bot-Adapter mit Sweep, PostgreSQL und lokalem Broker | 2 passed, 0 failed, 0 ignored, 322 filtered über beide Test-Binaries |
| Dashboard-Vertrag und i18n | 27 passed, 0 failed, 0 skipped |
| Clippy für dieselben vier Pakete, alle Targets, vor und nach Rebase | jeweils Exit 0; Nachlauf 6 Minuten 9 Sekunden |
| Formatprüfung der drei im Folgefix geänderten Rust-Dateien | Exit 0 |
| Dashboard-Build vor und nach Rebase | jeweils Exit 0; aktuelle Bild- und DOM-Probe am Artefakt `index-C1sEecg0.js` |
| Chromium am gebauten Artefakt, vier Zustände | Exit 0, keine Seitenfehler, keine horizontale Überbreite |
| Vollständige API-Suite | Nachlauf nach 600 Sekunden vom Harness beendet, ohne Endergebnis; keine neue Vollsuite-Baseline behauptet |
| Betroffene API-Handler nach Rebase | 53 passed, 0 failed, 0 ignored, 1278 filtered, Exit 0 |

Rust-Prüfbefehle und Datenbank entsprechen den unten dokumentierten Aufrufen, Cargo wurde über `/home/nathanael/.cargo/bin/cargo` gestartet. Der abgeschlossene API-Lauf war `cargo test -j 2 -p tb-dashboard-api --lib handlers::social_media -- --include-ignored --test-threads=1` mit denselben beiden Test-DSNs und `SQLX_OFFLINE=true`. Logs: `fixer-social-tests.log`, `fixer-rebased-social-tests.log`, `fixer-bot-tests.log`, `fixer-clippy.log`, `fixer-rebased-clippy.log`, `fixer-dashboard-tests.log`, `fixer-rebased-dashboard-tests.log`, `fixer-dashboard-build.log`, `fixer-rebased-dashboard-build.log`, `fixer-api-tests.log`, `fixer-api-social-tests.log`. Browseraufruf: `node .tasks/2026-10-07-social-token-ablauf/ui-proof.mjs`. Der SHA256 des aktuellen Dashboard-Artefakts lautet `17c4057764469637d1d8dacfd41578c3b7ff9a5cf1529864bf5f1566e4fed464`.

Die feste Uhr lautet `2026-10-07T12:00:00Z`. Beide Plattformen ohne Refresh-Wert warnen bei sechs Tagen, eskalieren genau beim Ablauf und behalten über Neustart denselben Vorfall und Versandnachweis. Der Broker bekommt pro Plattform einen Request. Ein gesunder Instagram-Zugang mit 20 Tagen Restzeit zeigt das beauftragte Datum; das 30-Tage-Fenster bleibt unverändert. Screenshots und DOM: `instagram-20-days.png`, `nonrenewable-6-days.png`, `renewable-access-ended.png`, `nonrenewable-ended.png`, `ui-proof.json`. Die Proben verwenden eigene Fixtures und keine Produktionszugänge.

TESTNACHWEIS[TW-1]: 438 passed, 0 ignored | Baseline: 22 rot
WIRKUNGSPRUEFUNG[WP-1]: 1 Befund behoben | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 1/1 im Folgefix geprüft

## Stand des ersten Implementierers am 7. Oktober 2026

Implementierung im eigenen Worktree. Nach Host-Reparatur und unverändertem Rebase hat der lokale Merge-Gate ein inhaltliches BLOCK gefällt: Verbindungen ohne Refresh-Möglichkeit erzeugen noch keinen Ablaufvorfall. Der bestätigte Befund und das Modell `gpt-6.1-sol` stehen in `REVIEW.md`; ein frischer Fixer ist erforderlich. Die folgenden Testzahlen bleiben gültig, decken diesen Fall aber noch nicht ab. Merge, Produktionsmigration und Live-Prüfung stehen aus. Produktionszugänge wurden nicht für eine Probe verändert. Es wurden keine weiteren Worker gestartet.

## Abgeschlossene Prüfungen

| Prüfung | Ergebnis |
| --- | --- |
| `tb-social-media` vor der Änderung | 300 passed, 0 failed, 0 ignored |
| `tb-dashboard-api` vor der Änderung | 1309 passed, 22 failed, 0 ignored |
| `tb-dashboard-api` nach der Änderung | 1309 passed, 22 failed, 0 ignored, 0 filtered; dieselben 22 Fehler, keine neuen |
| Neue Social-Media- und DB-Suites | 351 passed, 0 failed, 0 ignored, 0 filtered |
| Frontend-Vertrag und i18n | 26 passed, 0 failed, 0 skipped |
| Bot-DM-Adapter, echte DB und lokaler Test-Broker | 2 passed, 0 failed, 0 ignored, 322 filtered |
| Clippy für `tb-social-media`, `tb-dashboard-api`, `tb-bot`, `tb-db`, alle Targets | Exit 0, vorhandene Warnungen bleiben |
| Online-SQLx-Prüfung für `tb-social-media`, alle Targets | Exit 0, neue Offline-Metadaten erzeugt |
| `npm ci` und `npm run build` im Dashboard | jeweils Exit 0 |
| Admin-Dashboard und Website als erforderliche Release-Artefakte | Installation und Build jeweils Exit 0 |
| Gebautes Dashboard `index-Ck4pHALD.js` | Neue Texte und Statusfelder vorhanden, SHA256 `b0eb867d1034108440a4286c3de8a6319c7bbf63fa37b150bf86472661f2bf2b` |
| Formatprüfung der neun geänderten Rust-Dateien | Exit 0 |
| `cargo fmt --all --check` | Exit 1, Abweichungen in 148 nicht geänderten Dateien |
| Mechanischer UI-Detektor | 0 Befunde |

Die neue Rust-Prüfung lief mit Rust 1.99.0, `SQLX_OFFLINE=true`, `-j 2`, `--include-ignored --test-threads=1`. Die Wegwerf-Datenbank heißt `tb_social_token_ablauf`, Port 33045. Die bestehenden Produktionsvertragstests ersetzen keine Live-Prüfung.

```text
cd /home/nathanael/.worktrees/tb-social-token-ablauf/rust
PATH=/home/nathanael/.cargo/bin:$PATH SQLX_OFFLINE=true \
TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33045/tb_social_token_ablauf \
TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33045/tb_social_token_ablauf \
cargo test -j 2 -p tb-social-media -p tb-db --no-fail-fast -- --include-ignored --test-threads=1

PATH=/home/nathanael/.cargo/bin:$PATH SQLX_OFFLINE=true \
TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33045/tb_social_token_ablauf \
TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33045/tb_social_token_ablauf \
cargo test -j 2 -p tb-bot social_reauth -- --include-ignored --test-threads=1

PATH=/home/nathanael/.cargo/bin:$PATH SQLX_OFFLINE=true \
TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33045/tb_social_token_ablauf \
TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33045/tb_social_token_ablauf \
cargo test -j 2 -p tb-dashboard-api --lib -- --include-ignored --test-threads=1

PATH=/home/nathanael/.cargo/bin:$PATH SQLX_OFFLINE=true \
cargo clippy -j 2 -p tb-social-media -p tb-dashboard-api -p tb-bot -p tb-db --all-targets

cd /home/nathanael/.worktrees/tb-social-token-ablauf/bot/dashboard_v2
npm ci
npm run build
node --import tsx --test tests/socialMediaContract.test.ts tests/i18n.test.ts
```

## Geprüfte Wirkung

Der echte PostgreSQL-Zustand wurde gegen Neustart, gleichzeitige Sweeps, Eskalation, erneute Vorfälle und verspätete Fehler geprüft. HTTP-400-Fehler mit `invalid_grant` markieren den Zugang endgültig, 429 und 503 bleiben wiederholbar. Ein abgelaufenes Instagram-Recht wird nicht mehr zum Anbieter geschickt. Ein erster TikTok-Refresh speichert die tatsächlich gelieferte Refresh-Frist. Neu-Verbinden setzt den Vorfall zurück. Das Dashboard betrachtet das Ende der Verbindung, nicht den kurzlebigen Access-Ablauf.

Der Discord-Adapter verwendet die bestehende ID-Zuordnung ohne Login-Fallback und den bestehenden Broker-DM-Port. Der vorhandene Broker bildet einen deterministischen Schlüssel aus dem Payload. Plattform, Empfänger und Nachricht bleiben innerhalb eines Vorfalls gleich; ein neuer Vorfallszeitpunkt ändert den Link. Ein erfolgreich bestätigter Versand wird unter einer PostgreSQL-Zeilensperre gespeichert. Der Adaptertest lief gegen PostgreSQL und den lokalen Test-Broker. Zwei Twitch-IDs mit demselben historischen Login erreichen die jeweils ID-gebundene Discord-Zuordnung. Eine fehlende Zuordnung erzeugt keinen Request. Eine negative Broker-Antwort wird nicht als Zustellung gespeichert.

Es gibt keinen zusätzlichen Discord-Connector. Die sechs Anbieterpfade für Tausch und Refresh verwenden dieselbe strukturierte Fehlerklassifikation. Der DM-Pfad prüft das Erfolgsfeld und die Nachrichten-ID. Vollständige Anbieterantworten werden nicht in den neuen Fehlertext übernommen.

## Bestehende rote API-Tests

Die unveränderte Baseline und der Nachlauf enthalten dieselben 22 Fehler. Beide Läufe haben 1309 bestandene Tests. Der Nachlauf endet mit Exit 101, Laufzeit 436,90 Sekunden. Die neuen und die aufgelösten Fehlerlisten sind leer. Die betroffenen Social-Media-Handler-Tests sind bestanden.

```text
auth::idor_e2e_tests::discord_admin_ohne_mode_cookie_hat_im_public_dashboard_partner_scope
auth::idor_e2e_tests::partner_pfad_e2e_auth_status_scope_und_csrf
auth::level::tests::master_session_ist_nur_mit_admin_kontext_oder_mode_cookie_admin
handlers::admin_manual_plan::tests::set_manual_plan_refreshes_raid_score
handlers::admin_partner_signup_tag_block::tests::add_entry_backfillt_historische_sessions_sofort
handlers::admin_partner_signup_tag_block::tests::liste_gibt_eintraege_als_items_zurueck
handlers::admin_partner_signup_tag_block::tests::remove_meldet_entfernung_oder_fehlen
handlers::affiliate::tests::claim_api_blockt_aktive_partner
handlers::affiliate::tests::claim_api_blockt_inaktives_konto_mit_bestehender_session
handlers::affiliate::tests::claim_api_legt_claim_an_und_liefert_claims
handlers::engagement_settings::tests::admin_toggle_schaltet_versand_und_lesepfad_atomar_profil_bleibt_erhalten
handlers::exp_analytics::tests::partner_fremder_streamer_ist_forbidden
handlers::follower_funnel::idor_tests::partner_fremder_streamer_ist_forbidden
handlers::leaderboard::tests::load_category_trennt_partner_und_rest
handlers::loyalty_curve::tests::partner_fremder_streamer_ist_forbidden
handlers::lurker_analysis::tests::partner_fremder_streamer_ist_forbidden
handlers::monetization::tests::partner_fremder_streamer_ist_forbidden
handlers::retention_curve::tests::partner_fremder_streamer_ist_forbidden
handlers::streamers::tests::twitch_admin_login_gets_200
handlers::title::tests::live_kontext_kommt_aus_postgres
handlers::title::tests::rang_kommt_aus_postgres_und_folgt_primaerem_konto
uplink_config::tests::migration_environment_probe_options_rejected
```

## Prüfzeilen

TESTNACHWEIS[TW-1]: 1688 passed, 0 ignored | Baseline: 22 rot
TEXTNACHWEIS[DR-1]: Gedankenstriche 0 | ae/oe/ue/ss-Ersatz 0 | Absolutwörter 0 belegt | Senke: Dashboard und Social-Media-DM
WIRKUNGSPRUEFUNG[WP-1]: 1 Befund | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 7/7 geprüft

Die Testsumme umfasst den abgeschlossenen Rust-Lauf, den Bot-DM-Adapter, die beiden Frontend-Dateien und den API-Nachlauf. Die vollständige API-Suite ist wegen der unveränderten 22 Baseline-Fehler weiterhin rot. Die technische Fremddienstprüfung ist keine Behauptung über einen erfolgreichen Produktionsversand.

## Migration und Betrieb

Neue Migration: `20261007120000_social_connection_expiry.sql`. SHA384: `8b50646d1227c0276ea13fe20bd6315d9703aaf20b35b268daaad9424b646b6e82981ef65f70f861099aac9032e13ca0`.

`PROD-MIGRATION.sql` ist der beauftragte manuelle PostgreSQL-Schritt vor dem Neustart. Er übernimmt die unveränderte Migration, trägt ihre Checksumme ein und setzt die Tabellenrechte für `twitchbot` und `twitchdash`. Eine bereits eingetragene Migration wird auf dieselbe Checksumme geprüft.

Release und Live-Nachweis sind noch offen. Build-Herkunft wird über `.twitch_build` geprüft. Ein echter Zugang wird für den DM-Nachweis nicht entwertet.
