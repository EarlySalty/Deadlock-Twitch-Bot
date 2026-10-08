# A01: dritte Fixrunde

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min als Orientierung, keine Kompilierungs-Abbruchfrist | Worktree: /home/nathanael/.worktrees/tb-vollreview-session-widerruf

## Verbindlicher Umfang

Grundbriefing BRIEFING-A01.md sowie BRIEFING-A01-R2.md gelten weiter. Frischer Sol-Fixer nach beendetem Fixer und Kritiker der Runde 2. Deren Head ist `e16fab5b337283b748b2549b93ca11a042f7fee0`, Basis `a8b5b5e986a1de0b8e2f981651f83bda9cf400dd`. Erhaltenen eigenen Zustand prüfen, ausschließlich eigene Commits an aktuelles origin/main anpassen. Branch `fix/vollreview-session-widerruf`.

Schreibrechte bleiben auf `auth/session.rs`, `auth/level.rs` und `auth/discord_admin_login.rs` unter rust/crates/tb-dashboard-api/src beschränkt. Keine weiteren Auth-Befunde oder historischen Fehler nebenbei beheben. Fremde Dateien, Worktrees und Prozesse unverändert lassen. Astra pflegt die Dokumentation.

## Zwei noch offene Mängel aus Runde 2

1. **Ausdrückliche zentrale Ablehnung über den tatsächlichen Login-Aufrufer:** `auth/discord_admin_login.rs:261-263` macht aus valid=false weiter einen allgemeinen Fehler. Der Login-Aufrufer an `:404-412` löscht dann das Browsercookie, aber weder lokalen Spiegel noch Zulassungscaches. Ablauf bei erreichbarer Datenbank: lokaler gültiger Spiegel, ausdrückliche zentrale Ablehnung auf `/twitch/auth/discord/login`, später alter aufbewahrter Cookiewert bei technischem Brokerausfall. `auth/level.rs:448-471` erlaubt erneut Adminzugriff. Den bereits bestätigten A01b-Vertrag auch im Login-Aufrufer erfüllen. Beide tatsächlichen Broker-Validierungsaufrufer prüfen. Normalen technischen Ausfallfallback anderer gültiger Sitzungen erhalten. Cookie-Löschung allein ist kein serverseitiger Widerruf.
2. **Sechs neue Tests verletzen den bestehenden Opt-in-Vertrag:** Ohne `TB_TEST_REQUIRE_DB=1` liefern die unveränderten maybe_pool-Helfer None. Die neuen expect-Aufrufe paniken trotzdem. Fundorte am festen Head: `auth/session.rs:3523-3525,3598-3600,3641-3643`, `auth/discord_admin_login.rs:1502-1504,1616-1618`. Den bisherigen Skip ohne ausdrücklich aktivierte Datenbanktests erhalten. Bei ausdrücklich aktiviertem Testlauf und fehlgeschlagener Einrichtung darf weiterhin hart gescheitert werden. Keine allgemeine Abschwächung älterer Tests. Diesen Mangel in dieser Runde tatsächlich bearbeiten und den Lauf ohne Opt-in belegen; bisherige grüne Regressionen mit aktivierter Datenbank prüfen ihn nicht.

A01a ist laut Kritiker im untersuchten Refreshpfad behoben. UPDATE statt UPSERT und Cachegenerationen erhalten. Keine neue dauerhafte Widerrufsspeicherung oder Fallback-Policy für den separat dokumentierten gleichzeitigen lokalen DELETE-Ausfall bauen. Der konkrete Login-Restfehler benötigt keinen Datenbankausfall.

## Geprüfte Rollen und bestehende Nachweise

- Runde-2-Fixer `abab9423d389470ae`: 153 echte Sol-Datensätze, SHA256 `0baf6a0a367bed5e0f57ad9ad22fc4785cb1afda4aa421dfb5739945e6a1e2e4`.
- Runde-2-Kritiker `ad545da2eb734e8e3`: BLOCK, 55 echte Sol-Datensätze, SHA256 `3a69b1cb37ff0c1ed2fbc635d8dd417bcb10a9b5ef4c4eb35d28e69bc5a1261b`.
- Modelle/Hashes hat Astra an fertigen Transcripts geprüft. Vollständige Urteile in `wf_6b030f84-2a4/journal.jsonl`.
- Sol-Gate Runde 2: ALLOW. Dieses Urteil ersetzt die blockierende fachliche Kritik nicht. Sechs neue Regressionen bestanden; vollständiger gemeldeter Fixlauf 1325 bestanden und 22 fehlgeschlagen, Baseline 1319 bestanden und dieselben 22 fehlgeschlagen. Paket-fmt und Clippy bleiben mit belegten fremden Bestandsfehlern rot. Details und Logs in REVIEW.md. Keine unabhängige Testausführung durch den Kritiker.

## Prüfung und Rückgabe

Vor Codesuche code-suche/Graphify, Quellen aus eigenem Worktree. Toolchain `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, `/home/nathanael/.local/bin/cargo-slot`, Kompilierung `--jobs 1`. Vorgegebene synthetische Testdatenbank aus BRIEFING-A01.md verwenden, keine Zugangsdaten lesen. Eigene Dateien formatieren, Paket-fmt, Clippy und bestehende Crate-Tests mit passender unveränderter Baseline vergleichen. Reguläre Kompilierung fertiglaufen lassen. Zusätzlich neue Regressionen ohne Datenbank-Opt-in ausführen. Vorhandene eigene Baseline-Worktrees aus REVIEW.md erst auf Zustand prüfen; fremde oder laufende Arbeit nicht verändern.

Minimaler Rust-Fix ohne neue Kommentare, Konfiguration, Abhängigkeiten, Umbau oder Migration. Keine Secrets/ENV-Dateien, Produktionsdatenbank, Browser, Kontenaktionen oder ai-coach. Nur gpt-6.1-sol; keine Delegation, Threads, ListAgents/SendMessage oder Nutzerfragen.

Ein Git-Schritt je Bash-Aufruf, literale absolute Pfade. Eigener Commit erlaubt. Sol-only-Gate wie im Grundbriefing ausführen. Bei BLOCK abgeben, keine vierte Runde im eigenen Kontext. Kein Merge nach main, Release, Deploy oder Restart. Neuer frischer Kritiker prüft danach den festen Gesamtdiff einschließlich beider hier genannten Mängel.

Strukturierte Rückgabe: Basis/Head, Dateien, Wirkung je Mangel, Prüfungen und Exit-Codes, Baseline, Gate, Worktreezustand und Blocker. Keine Markdown-Berichtdateien schreiben.
