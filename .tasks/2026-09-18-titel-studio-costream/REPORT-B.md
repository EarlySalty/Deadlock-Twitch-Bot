# Abschluss Paket B: Nachtrag 3

Stand: 18. September 2026. Implementierung abgeschlossen, fachliches Review und Merge nach main bleiben beim Intent-Thread.

## Arbeitsstand

- Worktree: `/home/nathanael/.worktrees/tb-titel-costream`
- Branch: `feat/titel-studio-costream`
- Bearbeiter: direkte ChatGPT-Sitzung, GPT-6 Astra Pro. Für diese Sitzung ist keine T3-Thread-ID verfügbar. Kein zusätzlicher T3-Thread, kein Unter-Agent und kein Modellwechsel.
- `origin/main` zuerst konfliktfrei gemergt: `a77524fd217fd01e9d2622a0451cc40fe32fbac9`.
- Die Umnummerierung der kollidierenden Viewer-Fairness-Migration auf `20260918124000` stammt aus diesem Merge. Keine bestehende Migration selbst verändert.
- Keine Arbeit im geteilten Checkout, kein Merge/Push nach main, kein Deploy und kein Dienstneustart.

## Umsetzung

Die öffentliche Funktion `steam_lookup::detect_co_streamers_all` bleibt der gemeinsame Einstieg für Dashboard und `!title`. Nach Shared Chat wird jetzt die gleiche Steam-Party ausgewertet, erst danach Discord-Voice.

Der Party-Pfad löst die eigene Discord-ID über `core.steam_links` auf Steam auf. Aus `voice.deadlock_party_members` wird die jüngste frische, nicht leere Party-ID gelesen. Andere Steam-IDs derselben Party werden über `core.steam_links` und `twitch_streamer_identities` auf Twitch-User-IDs aufgelöst. Nur `twitch_live_state.is_live = 1` zählt. Eigene und fremde Präsenz müssen jünger als zehn Minuten sein. Leere und ausschließlich aus Leerzeichen bestehende Party-IDs, fehlende Verknüpfungen und Offline-Kanäle liefern keine Treffer.

Alle Signale behalten Twitch-IDs bis zur gemeinsamen Deduplizierung. Der eigene Twitch-Kanal wird ebenfalls über seine ID ausgeschlossen. Logins dienen nur der Ausgabe, nicht der Zuordnung. Shared Chat hat Vorrang vor Party, Party vor Voice. Es werden höchstens zwei Logins ausgegeben. Innerhalb der SQL-Signale ist die Auswahl deterministisch nach Twitch-ID sortiert. Die SQL-Ergebnisse werden nicht vorzeitig auf zwei gekürzt, damit mehrfach erkannte IDs keinen freien Ausgabeplatz blockieren. Der Ausfall einer lokalen Datenquelle lässt die anderen Signale nutzbar.

Der vorhandene Helix-Client bietet ergänzend `get_shared_chat_users` mit IDs und Logins. `get_shared_chat_logins` bleibt als kompatibler Wrapper bestehen. Keine zweite Shared-Chat-Implementierung, kein neuer Poll, kein EventSub-Abo, kein neuer LLM-Zugang und keine neue Konfigurationsvariable.

Der Party-Hinweis hatte zusätzlich einen nachgewiesenen Typfehler: Die zentrale Tabelle liefert `party_size INTEGER`; das INTEGER-Aggregat ließ sich nicht mit dem vorhandenen i64-Decoder lesen. Ein expliziter BIGINT-Cast repariert diesen Pfad. Der Test mit echten PostgreSQL-Typen war vorher rot und ist jetzt grün.

Doku, Register und Dashboard-Schalter nennen Steam-Präsenz als Party-Quelle. Die ruhige Dashboard-Zeile `Erkannt: du streamst mit @xy` bleibt ohne Quellenangabe. Im Frontend wurde ausschließlich dieser Schaltertext geändert, keine Geschäftslogik ergänzt.

## Geänderte Dateien

- `rust/crates/tb-chat/src/steam_lookup.rs`: Party-Signal, ID-basierte Zusammenführung, robuste unabhängige Signale, INTEGER/BIGINT-Korrektur und sechs neue Tests.
- `rust/crates/tb-transport-twitch/src/client.rs`: Shared-Chat-Teilnehmer mit IDs, bestehender Login-Wrapper und ein neuer HTTP-Test.
- `bot/dashboard_v2/src/pages/TitleGenerator.tsx`: ausschließlich der Text zur Steam-Präsenz.
- `docs/funktionsweise/titel-generator.md`: drei Signale, Reihenfolge, Frische und ID-Auflösung.
- `.tasks/2026-09-18-titel-studio-costream/REGISTER.md`: Bearbeitung und korrigierter Quellenbefund.
- `.tasks/2026-09-18-titel-studio-costream/REPORT-B.md`: dieser Bericht.

Der Main-Merge hat außerdem den Schema-Snapshot auf den gemeinsamen Migrationsstand gebracht. Für Nachtrag 3 wurde keine neue Migration benötigt.

## Tests: Baseline und Endstand

Toolchain: `/home/nathanael/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin`. Rust-Test-, Clippy- und Schema-Läufe mit `set -o pipefail`. Formatierung nur der zwei eigenen Rust-Dateien; abschließender Check mit `skip_children=true`.

Zahlen inklusive Unit-, Integrations- und Doc-Tests, jeweils bestanden / fehlgeschlagen / ignoriert:

| Paket | Baseline nach Main-Merge | Endstand |
|---|---:|---:|
| tb-chat | 960 / 8 / 6 | 966 / 8 / 6 |
| tb-dashboard-api | 1267 / 1 / 6 | 1267 / 1 / 6 |
| tb-transport-twitch | nicht separat gemessen | 114 / 0 / 0 |

Die acht bestehenden Fehler in tb-chat und der eine bestehende Fehler in tb-dashboard-api sind im Endlauf namentlich identisch. Der Gesamttest ist deshalb weiterhin rot, aber ohne Regression gegenüber der gemessenen Baseline. Außerhalb dieses Pakets wurden keine Tests oder Implementierungen angepasst.

Neue Tests: 7 bestanden, 0 fehlgeschlagen. Sie prüfen echte isolierte PostgreSQL-Abfragen, Frische auf beiden Seiten, leere Party-IDs, unterschiedliche Partys, fehlende Verknüpfungen, Offline-Kanäle, Party vor Voice, Shared Chat vor Party, Deduplizierung nach IDs bei unterschiedlichen Logins, Ausschluss des eigenen Kanals, das Zweierlimit, fehlende Discord-Verknüpfungen, Ausfall einzelner SQL-Quellen und den echten INTEGER-Typ des Party-Hinweises. Der Helix-Test prüft App-Token-Header, ID-Auflösung bei abweichenden Anzeigenamen, doppelte Teilnehmer, fehlende Benutzer, leere Sitzung und HTTP-401.

Rot-Nachweise: Vor der Implementierung waren drei neue Tests rot (0 bestanden / 3 fehlgeschlagen). Anschließend wurden für eine Gegenprobe die zusammengeführten Kandidaten sowie die Helix-Teilnehmerausgabe absichtlich geleert: fünf Chat-Tests und ein Transport-Test wurden rot; der bereits separat rot belegte Party-Hinweis-Test blieb grün. Diese Gegenprobenänderungen sind vollständig entfernt. Damit besitzt jeder der sieben neuen Tests einen Rot-Nachweis.

Weitere Prüfungen:

- `cargo build -p tb-chat -p tb-dashboard-api`: erfolgreich, beide Ziel-Crates bauen nach dem Main-Merge und der Implementierung.
- `cargo clippy -p tb-chat -p tb-dashboard-api -p tb-transport-twitch --all-targets`: erfolgreich. Bestehende Warnungen außerhalb der beiden geänderten Rust-Dateien; keine Warnung in diesen Dateien.
- `cargo test -p tb-chat -p tb-dashboard-api -p tb-transport-twitch --no-fail-fast`: obiger Endstand, exakt dieselben neun Fehler wie in der Baseline.
- `fresh_migrations_match_committed_schema_snapshot`: 1 bestanden / 0 fehlgeschlagen, gegen eine neu erstellte isolierte Timescale/PostgreSQL-Datenbank.
- `npm run build`: erfolgreich. Abhängigkeiten ausschließlich mit `npm ci --ignore-scripts --no-audit --no-fund` aus dem bestehenden Lockfile im eigenen Worktree installiert; keine Manifest- oder Lockfile-Änderung.
- `node --import tsx --test tests/titleCommand.test.ts`: 5 bestanden / 0 fehlgeschlagen.

## SQLx-Cache und Schema

Die neue Party-Abfrage und die angepasste Voice-Abfrage sind `sqlx::query_as`-Laufzeitqueries; der Party-Hinweis bleibt eine `sqlx::query`-Laufzeitquery. Es wurden keine SQLx-Makroqueries geändert, daher entstehen dafür keine Offline-Cache-Einträge. Die tatsächlichen Joins und PostgreSQL-Typen sind durch isolierte Datenbanktests abgesichert. Der Schema-Snapshot wurde nach dem Merge mit einem frischen Migrationslauf erfolgreich geprüft und benötigt für Nachtrag 3 keine weitere Änderung.

Die zusätzliche globale Prüfung `rust/scripts/sqlx-prepare.sh --check` hat den Schema-Vertrag erfolgreich geprüft, scheitert aber anschließend beim Online-Check der unveränderten Git-Abhängigkeit `dbrain-builds`: In der vom vorhandenen Skript erzeugten Twitch-Testdatenbank fehlen `brain.*`-Tabellen, unter anderem `brain.hero_catalog` und `brain.item_catalog` (18 Compilerfehler). Keine globale grüne SQLx-Cache-Prüfung behauptet. Keine fremden Brain-Migrationen oder künstlichen Offline-Metadaten ergänzt. `rust/.sqlx` bleibt unverändert.

## Nachweise im Worktree

Die lokal ignorierten Protokolle liegen neben diesem Bericht: `B-baseline.log`, `B-red.log`, `B-focused.log`, `B-counterprobe.log`, `B-clippy.log`, `B-final.log`, `B-build.log`, `B-schema-cache.log` und `B-frontend-build.log`. Die neun bestehenden Fehler sind in `B-baseline.log` und `B-final.log` vollständig dokumentiert.
