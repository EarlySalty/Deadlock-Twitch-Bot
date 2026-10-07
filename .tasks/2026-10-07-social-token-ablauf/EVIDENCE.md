# Prüfnachweise

## Stand am 7. Oktober 2026

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
