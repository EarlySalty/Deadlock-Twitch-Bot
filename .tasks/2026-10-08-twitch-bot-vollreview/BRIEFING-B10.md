# B10: Authstatus, Cookie-Header und Cachezeit

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min als Orientierung, keine Kompilierungs-Abbruchfrist | Worktree: /home/nathanael/.worktrees/tb-vollreview-authstatus

## Auftrag und Eigentum

Frischer Sol-Fixer für zwei vollständig gegengeprüfte B-Claims. Eigener neuer Worktree `/home/nathanael/.worktrees/tb-vollreview-authstatus`, Branch `fix/vollreview-authstatus`, von frisch geholtem origin/main. Falls schon vorhanden, Zustand zuerst prüfen und erhalten. Einziger Schreibpfad: `rust/crates/tb-dashboard-api/src/handlers/auth_status.rs` einschließlich bestehender eingebetteter Tests. Keine andere Rolle schreibt diese Datei. Aktuelles main auf bereits erfolgte Behebung prüfen.

Astra hat Reviewer- und Skeptiker-Modelle, Transcript-Hashes und finale StructuredOutputs geprüft. Originalbefunde in W03-DA03-KANDIDATEN.json; vollständige unveränderte Urteile in W03-DA03-GEGENPRUEFUNG.json beziehungsweise Journal wf_97f7401d-6c8. Feste Reviewbasis 0ecae1370f1a80d1a101249b5c932663d69be8af.

## B10a: Moduscookie im späteren Header

Claim W03-DA03-S004-correctness-2, Ort auth_status.rs:143. Gültige Sitzung der konfigurierten Betreiber-ID im ersten Cookie-Header, tb_admin_mode=2 in einem zweiten: Auth erkennt Admin mit actor, der Handler meldet wegen eigener Ein-Header-Auswertung fälschlich Partneransicht. Kein Rechtegewinn. Bestätigt ist der direkt erreichbare native Backendpfad; Auftreten über einen konkreten öffentlichen Proxy wurde nicht behauptet.

Soll: Statusdarstellung entspricht der bereits legitimierten Identität und dem vorhandenen Modusvertrag. `auth/level.rs:212-240,293-310` berücksichtigt mehrere Cookie-Header; `auth_status.rs:435-467` erwartet aktivierte Admin-Präsentation. Normalen Ein-Header-Fall, bestehende Reihenfolge-/Wertsemantik, Nichtbetreiber und actor=None erhalten. Keine neue Auth-Policy oder Änderung von auth/level.rs.

- Reviewer a339ae26daf5d9852, 82 Sol-Datensätze, SHA256 018839e3e3cea1980a940679ed3679fee28c67baaddbb91cf6ff8f0b5b34d351.
- Skeptiker 1 a7734343019d003ee: BESTÄTIGT-B, Soll true, 46 Sol-Datensätze, SHA256 ebc269baf7eb6ad396ef309383a24990badfe599c4f4234460ab03039b02436f.
- Skeptiker 2 a2b04dfa56523d390: BESTÄTIGT-B, Soll true, 37 Sol-Datensätze, SHA256 a30230eeb384617b055324c6b8acf7820ef60d81acc346f7a36e5e305ad36c8d.

## B10b: veralteter Zeitwert vor dem Cachelock

Claim W03-DA03-S004-concurrency-2, Ort auth_status.rs:161. Request A erfasst Sekunde N vor dem Lock und pausiert; B setzt nach einer Sekundengrenze den Cachezeitpunkt auf N+1; A zieht nach Lockübernahme den neueren Cachezeitpunkt vom älteren eigenen u64-Wert ab. Mit Überlaufprüfung panikt die Antwort. Dafür ist kein rückwärts laufender Systemtakt erforderlich.

Soll: dokumentierte und getestete HTTP-200-Antwort im unauthentifizierten Pfad (`auth_status.rs:72-75,156-168,345-355,569-584`). Die normale Produktions-Releasekonfiguration prüft Überläufe nicht; ein produktiver Prozessabsturz ist nicht belegt und darf nicht behauptet werden. Behebung eng auf das Zeit-/Lockproblem begrenzen. Bestehende Cache-TTL, HTTP-Header und Payload erhalten.

- Reviewer ab74d43b958e5c560, 49 Sol-Datensätze, SHA256 5c3216051e16ddd950cad3f55832694135f75cca29e15cffd9a30719e5c2f8c7.
- Skeptiker 1 a6a679f14a0a819a4: BESTÄTIGT-B, Soll true, 40 Sol-Datensätze, SHA256 16073a33d4265f931170c13b46de3790121969d688b3362051800c3abc97bfd1.
- Skeptiker 2 a3f9ccd055902addc: BESTÄTIGT-B, Soll true, 31 Sol-Datensätze, SHA256 3d2023c2c6991a9a1b166e4253828e6cb0713496d22ad4858b94d1660c666297.

## Ausgeschlossener Nachbarclaim

W03-DA03-S004-correctness-1, HTTP-Cache über Cookiewechsel, erhielt BESTÄTIGT-B und PLAUSIBEL-B und bleibt C/gesperrt. Seine Cache-Control-/Vary-Regeln nicht nebenbei ändern. Keine dritte Gegenprüfung als Ersatz für fehlende Freigabe. Auch andere Auth-Dateien gehören nicht zu B10.

## Prüfungen und Grenzen

Vor Codesuche code-suche und Graphify. Minimaler Rust-Diff, keine Kommentare, Refactorings, neue Konfiguration, Abhängigkeit oder Migration. Günstige gezielte Regressionen für beide Claims, bestehende tb-dashboard-api-Tests, Paket-fmt und Clippy; fremde rote Stellen gegen passende unveränderte Baseline abgrenzen. Eigene Datei gezielt formatieren. Toolchain `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, cargo-slot, bei Kompilierung --jobs 1. Synthetische Testdatenbank `TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql'`, `TB_TEST_REQUIRE_DB=1`. Optionalen DB-Opt-in nicht brechen. Normale Kompilierung fertiglaufen lassen, eigene Aufgaben nicht duplizieren, fremde Prozesse oder Dienste nicht ändern.

Ursprünglicher Nutzerauftrag umfasst lokalen Commit und spätere Integration. Sauberen nichtleeren Fix-Commit für frische Kritik liefern. Git einzeln mit literalen absoluten Pfaden. Abschließender Sol-only-Gate mit --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080. Kein --chain oder Modellfallback. Bei BLOCK oder zusätzlichem Schreibbedarf mit Beleg abgeben, nächste Runde frisch. Kein Main-Merge, Release, Deploy oder Restart. Keine Secrets/ENV-Dateien, Produktionsdatenbank, Browser, Kontoaktionen, ai-coach, weiteren Agenten/Threads/ListAgents/SendMessage oder Nutzerfragen. Astra dokumentiert.

Rückgabe: tatsächliche Basis-/Head-SHAs, sauberer Worktree, geänderte Dateien, Claimstatus, Prüfungen/Exit-Codes, passende Baseline, Gate samt Belegen und offene Blocker. Danach frischer read-only Sol-Kritiker des festen Gesamtdiffs.
