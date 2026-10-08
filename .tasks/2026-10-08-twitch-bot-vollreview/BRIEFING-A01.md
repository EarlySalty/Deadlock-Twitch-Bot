# Fixpaket A01: serverseitiger Sitzungswiderruf

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-vollreview-session-widerruf

## Zwei unabhängig bestätigte Sicherheitsbefunde

### A01a: Session-Refresh nach Logout

ID `W02-DA02-S001-concurrency-1`, `rust/crates/tb-dashboard-api/src/auth/session.rs:2101`.

Ein Request liest eine gültige, seit mehr als 1800 Sekunden nicht persistiert verlängerte Partnersitzung. Ein zweiter Request beendet inzwischen Logout und löscht Zeile sowie Cache. Der erste Request setzt danach seinen gleitenden Refresh per INSERT ON CONFLICT fort und legt dieselbe Sitzung erneut an. Ein vorher kopierter Cookie-Wert wird damit wieder serverseitig gültig, auch nach Ablauf des Kurzzeit-Caches. Ein Cookie-Diebstahl oder die automatische Wiederherstellung des Browsercookies wird nicht behauptet.

Sollbelege am Review-SHA `0ecae1370f1a80d1a101249b5c932663d69be8af`: `handlers/auth_login.rs:724-753`, `auth/session.rs:3506-3544` und Logout-Test `handlers/auth_login.rs:1971-1976`. Der Pfad ist über native Auth-Status-/Logout-Routen erreichbar. Kritische Operationen: `auth/session.rs:1773-1807,1993-2014,2043-2050,2069-2115`. Cache-Locks, Ablauf, Identitätsprüfung und Primärschlüssel verhindern das belegte Interleaving nicht.

Ziel: Ein laufender Refresh darf eine inzwischen widerrufene Sitzung weder dauerhaft wieder anlegen noch durch seinen fehlgeschlagenen Verlängerungsversuch erneut als gültig zwischenspeichern. Bestehende TTLs, Cache-Verträge für gültige Sitzungen, Identität, Fingerprints und legitime bereits laufende Requests nicht pauschal umbauen. Betroffene gemeinsame Aufrufer des Refresh-Helfers prüfen. Keine Migration oder neue Widerrufstabelle.

### A01b: lokale Zulassung trotz ausdrücklicher zentraler Ablehnung

ID `W02-DA02-S002-errors-1`, `rust/crates/tb-dashboard-api/src/auth/level.rs:444`.

Der Broker-Client bildet sowohl technische Fehler als auch die ausdrückliche Antwort `valid=false` auf denselben Fehlerpfad ab. Bei einem vorhandenen lokalen Spiegel fällt die Rechteauflösung anschließend auf die lokale Sitzung zurück und vergibt Adminrechte. Zentrale Widerrufung beziehungsweise Ablehnung wird dadurch umgangen; die lokale Sitzung kann weiter gleitend verlängert werden.

Sollbelege: `auth/level.rs:401-403,421-473`, `auth/discord_admin_login.rs:250-262,1715-1746`, lokale Verlängerung `auth/session.rs:1593-1614`. Der bestehende zentrale Ablehnungstest verlangt eine neue Anmeldung trotz noch gültiger lokaler Kopie. Die beabsichtigte Ausfalltoleranz ist ein eigener Vertrag.

Ziel: Eine ausdrückliche zentrale Ablehnung muss abweisen. Den vorhandenen lokalen Fallback bei echten technischen Broker-Ausfällen erhalten. Keine neue Fallback-Policy, keine pauschale Abschaltung des Offline-Verhaltens und kein Eingriff in legitime lokale beziehungsweise interne Admin-Zugänge.

Beide Nachweise sind statisch. Keine produktive Ausnutzung, kein Live-Request und kein tatsächlicher Cookie-Diebstahl wurde nachgewiesen.

## Freigabekette und Modelle

Astra hat alle vorhandenen `message.model`-Felder der sechs abgeschlossenen Transcripts geprüft: ausschließlich `gpt-6.1-sol`. Skeptiker bekamen nur die jeweilige Behauptung und den Ort, keine Reviewerbegründung oder fremden Urteile.

| Befund und Rolle | Agent | Nachrichten | Transcript-SHA256 |
|---|---|---:|---|
| A01a Reviewer | acbdd6c84fd00b590 | 51 | a594c1b20b7cd17824976ebd8d878b4d1b6fb065c703220faaea69170aede86e |
| A01a Skeptiker 1 | a56f4e255006e7291 | 43 | 800ecaa7f09b6f0b7b03ecef47483d51d3dbf60df1de4ad597a81b503bddc67b |
| A01a Skeptiker 2 | ac61a921899bce902 | 35 | 4a99090ed9a1789d9f431fc2d043cc91fec6b56553bfc1b7fe9fdb8c35eefadd |
| A01b Reviewer | aee0aa071af5d50b8 | 51 | 7f45e9e7db7a99d0d532269526bfad94b925da9176da85c8395afde770018503 |
| A01b Skeptiker 1 | a636373646c18fc85 | 45 | 6bae0e5abd1a7217e518e2abea16e128dc3b8d2efe03eec5c76303b53be1a758 |
| A01b Skeptiker 2 | a2d35f684e4064cfc | 41 | 55a771e4d05156b16b61be970267f42caaf61fb0a28ebb829787ce79577283d9 |

Reviewer-Workflow `wf_cdc4c5ac-9bb`, Skeptiker-Workflow `wf_9eb7b672-f97`. Alle vier Skeptiker urteilen BESTÄTIGT, Klasse A, Soll belegt.

## Eigentum und Arbeitsstand

Neuen eigenen Worktree `/home/nathanael/.worktrees/tb-vollreview-session-widerruf`, Branch `fix/vollreview-session-widerruf`, von frisch geholtem aktuellem `origin/main` anlegen. Zunächst prüfen, ob aktuelles main einen Befund bereits beseitigt hat. Bei Wiederaufnahme vorhandenen eigenen Stand erhalten statt erneut anzulegen.

Ein gemeinsamer Fixer besitzt ausschließlich diese drei Dateien unter `rust/crates/tb-dashboard-api/src/auth/`:

- `session.rs`
- `level.rs`
- `discord_admin_login.rs`

Andere Fixer bearbeiten `obs/bus.rs`, `obs/ws.rs`, `admin_audit.rs` oder den internen Streamer-Wrapper. Keine Überschneidung. Falls eine weitere Quelldatei unvermeidlich benötigt wird, mit Beleg an Astra zurückgeben. Keine allgemeine Auth-Modernisierung, Umbenennung, neue Kommentare, Abhängigkeiten, Konfiguration, Migrationen oder weitere Bugs nebenbei. Nicht aufgrund verwandter unbestätigter Hypothesen neue Sicherheitsregeln einführen.

## Beweise und Gitgrenzen

Vor Codesuche code-suche und Graphify, anschließend aktueller eigener Worktree oder fest gebundener Commit. Graph: `/home/nathanael/repos/Deadlock-Twitch-Bot/graphify-out/graph.json`. Keine Secrets/ENV-Dateien, keine Kontenaktionen, kein Live-Logout, kein Browser, keine Produktionsdatenbank oder `ai-coach`.

Günstige deterministische Regressionen ergänzen: Read/Logout/Refresh-Reihenfolge ohne wiederangelegte beziehungsweise neu gecachte gelöschte Sitzung; tatsächlicher Broker-Client mit `valid=false` bei lokal vorhandenem Spiegel; technischer Ausfall mit erhaltenem bisherigem Fallback. Bestehende relevante Sitzungs- und Login-Tests nachziehen, ohne deren Sicherheitsverträge abzuschwächen.

Prüfregeln aus rolle-test-waechter und OPS-PREFLIGHT.md. `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, vorhandenes `/home/nathanael/.local/bin/cargo-slot`, bei Kompilierung `--jobs 1`. Test-DB `TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql'`, `TB_TEST_REQUIRE_DB=1`; `rust/test-database.json` nur auf Existenz prüfen, keine Zugangsdaten lesen. Eigene Dateien formatieren; paketbezogen Formatprüfung, Clippy und bestehende `tb-dashboard-api`-Tests. Vorbestehende fremde Fehler gesondert nachweisen. Keine fremden Builds, Prozesse oder Dienste ändern und keine normale eigene Kompilierung nach wenigen Minuten abbrechen.

Rolle-merge-schleuse vor Git: ein Git-Schritt je Bash-Aufruf, literale absolute Pfade, nur eigene Dateien stagen. Eigener Commit und Sicherung des Arbeitsbranches erlaubt; Trailer `Co-authored-by: GPT 6.1 Sol <gpt-6.1-sol@local>`. Kein Force-Push oder Push nach main.

Eigene finale Gate-Prüfung:

```text
python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-vollreview-session-widerruf --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080
```

Bei BLOCK Urteil, SHA und Stand zurückgeben; die nächste Fixrunde bekommt frischen Kontext. Keine Rückfallmodelle, kein `--chain`, keine Hook- oder Zustandsänderung. Ein frischer Sol-Kritiker folgt nach Abgabe. Noch kein Merge, Release, Deploy oder Restart. Der offene Konflikt zwischen Deploy-Wrapper und Produktionsdatenbank-Schreibverbot wird nicht umgangen.

## Rückgabe

Je Teilbefund tatsächlicher Fixstatus, Basis-/Head-SHA, Dateiänderungen, Baseline und Prüfaufrufe mit Exit-Codes, Gate-Urteil sowie verbleibende Grenzen. Keine Testfreigabe ohne ausgeführte Tests. Keine Markdown-Berichtdateien, Astra schreibt die Artefakte.

Ausschließlich native Workerrolle mit `gpt-6.1-sol`. Keine weiteren Agenten oder Threads, kein ListAgents/SendMessage, keine Nutzerfragen. Auftraggeber Astra `c88f4057-c6b3-4c54-8750-addd08b24b42`, Hauptauftraggeber `819f0d87-8d3f-4fbf-8c4c-ada2bf290f5b`.
