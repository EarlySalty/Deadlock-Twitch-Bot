# Nachweise Auftrag F

## Verhalten und Bestand

BESTAND[BS-1]: ja | Fundort: rust/crates/tb-social-media/src/clip_templates.rs:260 | Anknüpfung: clip_last_hashtags nach Twitch-User-ID, vorhandene manuelle Plattformfelder

WIRKUNGSPRUEFUNG[WP-1]: Bot-Start und manueller Start-Endpunkt rufen keine Clip-Anreicherung mehr auf; clip_context_learn ruft keinen Harvest mehr auf. Bibliothekscode, gespeicherte Texte, Tabellen und alte Vorschau-Dateien bleiben erhalten. Freigabe nimmt auch Deadlock-Clips mit ausstehendem Enrichment auf. Gemeinsames Rendern enthält Branding und manuelle Titel, keine gesprochenen Transkript-Untertitel.

TEXTNACHWEIS[DR-1]: Editor und Menü verwenden „Titel, Beschreibung & Hashtags“. Keine Regenerierung, Transkriptanzeige, Modellangaben oder automatischen Vorschläge im Editor. Neue Texte sind in der englischen Übersetzung hinterlegt. Zuletzt gespeicherte Hashtags werden ausdrücklich per Klick eingefügt, dabei normalisiert und ohne Duplikate übernommen.

## Rust

- `SQLX_OFFLINE=true TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33078/postgres TB_TEST_REQUIRE_DB=1 /home/nathanael/.cargo/bin/cargo test --manifest-path /home/nathanael/.worktrees/tb-social-anreicherung-aus/rust/Cargo.toml -p tb-social-media --lib -j 2 -- --include-ignored`: 301 passed, 0 failed, 0 ignored, 0 filtered; echte isolierte PostgreSQL-Datenbank, Laufzeit 8,17 s nach Kompilierung.
- Der neue Freigabetest beweist die Upload-Warteschlange ohne Transkript. Hashtag-Test beweist Speicherung und unbekannte Identität. Vorschautest beweist logische Veraltung ohne Änderung der gespeicherten alten Datei. Bestehender Upload-Render-Test erwartet jetzt ausdrücklich keine Transkriptworte.
- `SQLX_OFFLINE=true TB_TEST_DATABASE_URL=postgres://postgres:tbtest@127.0.0.1:33078/postgres TB_TEST_REQUIRE_DB=1 /home/nathanael/.cargo/bin/cargo test --manifest-path /home/nathanael/.worktrees/tb-social-anreicherung-aus/rust/Cargo.toml -p tb-dashboard-api --lib handlers::social_media::tests -j 2 -- --include-ignored`: 48 passed, 0 failed, 0 ignored, 1283 filtered; Laufzeit 3,81 s nach 7 min 11 s Kompilierung. Beide geänderten Handler-Regressionsfälle laufen: abgeschalteter Start mit force=true und ohne Seiteneffekt; manuelles Speichern mit Wiederverwendung nach Loginwechsel und Trennung fremder Twitch-IDs.
- `SQLX_OFFLINE=true cargo clippy -p tb-social-media -p tb-dashboard-api -p tb-bot -p tb-dashboard --all-targets -j 2`: Exit 0, mit vorhandenen Warnungen; 3 min 6 s. `-- -D warnings` ist nicht grün: Abbruch in tb-raid/src/signup_denylist.rs:71, clippy::result_unit_err. Separater Lauf `SQLX_OFFLINE=true cargo clippy -p tb-raid --lib -j 2 -- -D warnings`: ebenfalls Exit 101, genau 1 Fehler an derselben Stelle. `git diff --exit-code origin/main -- rust/crates/tb-raid rust/Cargo.lock`: Exit 0. Damit ist dieser einzelne Abhängigkeitsbefund als Baseline gemessen, nicht ein kompletter strenger Workspace-Lauf.
- Alle acht geänderten Rust-Dateien bestehen `rustfmt --edition 2021 --config skip_children=true --check`.
- `cargo fmt --all -- --check`: Exit 1, 148 unveränderte Dateien. Für jede dieser 148 Dateien wurde der Inhalt aus HEAD separat durch rustfmt geprüft: 148 bereits unformatiert, 148 bytegleich mit dem Arbeitsbaum, keine Formatterfehler. Keine dieser Dateien wurde geändert.

## SQLx-Metadaten

Der komplette frische Migrationslauf stimmt nach Provisionierung der vorausgesetzten Datenbank twitch_analytics im Wegwerfcontainer mit dem bestehenden Schema-Snapshot überein: 1 passed, 0 failed, 0 ignored. Keine Migration geändert.

`rust/scripts/sqlx-prepare.sh` konnte keinen weiteren Container starten, weil Docker dessen cgroup-Mount wegen fehlendem Speicher ablehnte. Deshalb dieselben Schritte gegen den bereits vorhandenen eigenen Container ausgeführt: Schema-Vertrag, vollständige Migrationen, `cargo sqlx prepare --workspace -- --all-targets -j 2`.

Die Workspace-Vorbereitung scheiterte später an fehlenden fremden brain.*-Tabellen. Der neue Freigabe-Cache wurde dabei tatsächlich gegen das verifizierte Migrationsschema erzeugt. 336 von der Vorbereitung entfernte, unveränderte Cache-Dateien aus HEAD wiederhergestellt. Einziger neuer Cache: query-ca7704a85b86caa92bffa837b16e1af9745ae6369f761ab6509d37ef0225ddd6.json. Der folgende Offline-Clippy-Lauf aller vier betroffenen Pakete besteht. Keine Metadaten von Hand erstellt.

## Dashboard

- `npm ci`: Exit 0.
- `npm run build`: Exit 0, erneuter Build nach Hashtag-Normalisierung, Vite 3,99 s.
- `node --import tsx --test tests/socialMediaContract.test.ts`: 17 passed, 0 failed, 0 skipped.
- Impeccable-Detektor für EnrichmentPanel.tsx und SocialMedia.tsx: keine Funde.
- T3-Vorschauhost nicht verfügbar. Stattdessen tatsächlichen Editor mit lokalem API-Prüfdatensatz in Chromium gerendert. 10 Prüfungen bestanden: manuelle Änderungen, explizites Einfügen, Normalisierung älterer Tags, kein Doppel-Hashtag, kein Regenerierungsaufruf, Plattformwechsel, sichtbarer Speicherfehler mit erhaltenem Entwurf, keine automatischen Inhalte, kein JavaScript-Fehler, mobile Breite 390/390 px.
- Screenshots: /tmp/tb-social-f-editor-desktop.png und /tmp/tb-social-f-editor-mobile.png. Bildschirmbild geprüft. Temporäre Prüfseite und eigener Vite-Prozess entfernt.
- Browser-Prüfdatensatz beweist keine echte Speicherung. Diese wird separat durch Rust-DB-Tests geprüft. Produktive Benutzerdaten wurden dafür nicht verwendet.

## Bewusste Grenzen

Manuelle Metadaten und zuletzt verwendete Hashtags werden getrennt gespeichert, nicht in einer gemeinsamen Transaktion. Ein Fehler bei der Hashtag-Speicherung liefert einen sichtbaren Fehler; der Entwurf bleibt für Wiederholung erhalten. Werden mehrere Plattform-Hashtagfelder zugleich gespeichert, wird die erste geänderte Liste in der Reihenfolge YouTube, TikTok, Instagram als letzte Liste wiederverwendet. Die vorhandene Lesefunktion für letzte Hashtags liefert bei Datenbankfehlern weiterhin eine leere Liste.

## Stand vor Integration und Live

TESTNACHWEIS[TW-1]: 367 passed, 0 ignored | Baseline: Clippy-Abhängigkeit 1 rot

Die 367 Tests sind 301 Social-Media-, 48 Handler-, 17 Dashboard-Vertrags- und 1 Schema-Vertragstest. Kein Vorher-Nachher-Testbaseline-Lauf behauptet. Die Baseline-Angabe bezieht sich ausschließlich auf die unveränderte, separat gemessene Clippy-Abhängigkeit.

Gate-Runde 1 für Implementierungscommit 8c58ea2c: ALLOW, kein blockierender Mangel. Vollständige Hinweise in REVIEW.md. Diese Datei dokumentiert den Stand vor dem Main-Push und Release. Live- und Abschlussbericht werden nach der Aktivierung separat gesichert; ein Deployment wird hier noch nicht behauptet.
