# Fixpaket B03: Audit-Akteur bei mehreren Sitzungscookies

BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-vollreview-audit-akteur

## Ziel und Sollvertrag

Befund `W02-DA01-S003-correctness-1`, Klasse B. Ein altes erstes und gültiges zweites `master_dash_session`-Cookie wird von Authentifizierung und CSRF korrekt behandelt. Das Audit prüft dagegen nur den ersten Wert und speichert bei einer erfolgreichen Admin-Aktion `admin` statt der bekannten Discord-ID. Mehrere Cookie-Header können denselben Fehler auslösen.

Belege am Review-SHA `0ecae1370f1a80d1a101249b5c932663d69be8af`:

- `rust/crates/tb-dashboard-api/src/admin_audit.rs:54-88,110-130`: erste Cookie-Auswahl, Akteursauflösung und Speicherung.
- `rust/crates/tb-dashboard-api/src/auth/level.rs:408-474,872-917`: Authentifizierung probiert vorhandene Admin-Cookies; bestehender Test akzeptiert den späteren gültigen Wert.
- `rust/crates/tb-dashboard-api/src/auth/csrf.rs:55-67,125-144,303-335`: passende CSRF-Verträge und Tests.
- `rust/crates/tb-dashboard-api/src/handlers/admin_audit_log.rs:674-699`: Ausgabe des gespeicherten Akteurs.

Soll: Die bekannte Sitzung, die die erfolgreiche Admin-Aktion legitimiert, bestimmt den Audit-Akteur. Keine nachgewiesene Rechteausweitung. Der Best-effort-Charakter des Audits rechtfertigt nicht die abweichende Cookie-Auswahl.

## Unabhängige Freigabe

- Reviewer `a5d8a2b501d115ffc`, Workflow `wf_cdc4c5ac-9bb`: 61 Sol-Nachrichten; SHA256 `4ee266e1a9504da20f5e2516fec933d6768d9acebfd9c2823a6dc3d5c134fba1`.
- Skeptiker 1 `a21bacec28c4e4452`: BESTÄTIGT, B, Soll eindeutig; 31 Sol-Nachrichten; SHA256 `48cf694987bc9d8d0920ab48efe05f2a91e21cde4723c3d2ce613400823ee801`.
- Skeptiker 2 `ac5c34d004bd0f275`: BESTÄTIGT, B, Soll eindeutig; 37 Sol-Nachrichten; SHA256 `ee85e0effd88b0d645c8b38267d4bbf4e2da81df58ba20cc07a7cd53971d188a`.

Beide Skeptiker liefen in `wf_3b16d4aa-68e` mit frischem unabhängigen Kontext; sie erhielten nur Behauptung und Ort. Astra hat die Modellfelder der abgeschlossenen Transcripts geprüft. Erreichbarkeit, Auth, CSRF, bestehende Cookie-Tests und der mitversionierte Proxy-Vertrag wurden berücksichtigt. Keine Live-Vorfallbehauptung.

## Eigentum und Arbeitsstand

Neuer eigener Worktree `/home/nathanael/.worktrees/tb-vollreview-audit-akteur`, Branch `fix/vollreview-audit-akteur`, von frisch geholtem aktuellem `origin/main`. Falls bei Wiederaufnahme vorhanden, zuerst prüfen und erhalten.

Einziger Schreibbereich: `rust/crates/tb-dashboard-api/src/admin_audit.rs`, einschließlich bestehendem Testmodul. Andere Fixer besitzen `obs/bus.rs`, `obs/ws.rs` beziehungsweise `tb-internal-api/src/handlers/streamers.rs`; diese Dateien nicht ändern. Astra hält die Taskdokumente.

Minimaler Fix der Akteursauswahl. Bestehende Auth-/CSRF-Wege als Verträge verwenden, nicht neu bauen. Rollen, Zugangsprüfung, Cookie-Setzung, gültige Einzelcookies, unbekannte Administratoren und auditierte Routen unverändert lassen. Insbesondere keinen Fix für den getrennten C-Befund zum Audit-Timeout umsetzen. Keine Kommentare, Refactorings, Migrationen, neue Konfiguration oder Abhängigkeit. Benötigt der Fix eine weitere Quelldatei oder Produktentscheidung, an Astra zurückgeben.

## Nachweis und Abschlussgrenzen

Vor Codesuche code-suche und Graphify. Referenzgraph `/home/nathanael/repos/Deadlock-Twitch-Bot/graphify-out/graph.json`; aktuelle Quellen ausschließlich aus dem eigenen Worktree. Prüfen, ob main den Defekt inzwischen behoben hat.

Einen günstigen Regressionstest für veraltetes erstes und gültiges zweites Cookie sowie getrennte Cookie-Header ergänzen. Erfolgreiche Einzelcookie-Zuordnung und bestehende anonyme Fallbacks erhalten. Keine echten Konten, kein produktiver API-Aufruf, keine Produktionsdatenbank.

Prüfregeln aus rolle-test-waechter und OPS-PREFLIGHT.md. Toolchain `RUSTUP_TOOLCHAIN=1.97.1-x86_64-unknown-linux-gnu`, `SQLX_OFFLINE=1`, `/home/nathanael/.local/bin/cargo-slot`, bei Kompilierung `--jobs 1`. Test-DB `TB_TEST_DATABASE_URL='postgres:///tb_bb_test?host=/var/run/postgresql'`, `TB_TEST_REQUIRE_DB=1`; `rust/test-database.json` nur auf Existenz prüfen, keine Zugangsdaten lesen. Eigene Datei formatieren, paketbezogen Formatprüfung, Clippy und bestehende Tests von `tb-dashboard-api`. Vorbestehende fremde Fehler als Baseline dokumentieren, nicht reparieren. Normale eigene Kompilierung nicht nach wenigen Minuten abbrechen; Slotmechanik statt fremder Prozess- oder Dienständerungen.

Git nach rolle-merge-schleuse: ein Git-Schritt je Bash-Aufruf, literale absolute Pfade, nur eigene Datei stagen. Eigener Commit und Sicherung des Arbeitsbranches erlaubt. Trailer `Co-authored-by: GPT 6.1 Sol <gpt-6.1-sol@local>`. Kein Force-Push oder Push nach main.

Finale eigene Sol-Gate-Prüfung:

```text
python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-vollreview-audit-akteur --base origin/main --head HEAD --model gpt-6.1-sol --effort high --timeout 1080
```

Bei BLOCK mit Urteil und SHA zurückgeben; keine zweite Fixrunde im selben Kontext. Kein Modellrückfall oder Gate-Bypass. Danach prüft ein frischer Sol-Kritiker. In dieser Runde kein Merge, Release, Deploy oder Restart. Der Deploy-Wrapper-Konflikt bleibt offen.

## Rückgabe

Basis-/Head-SHA, tatsächlicher Diffumfang, Baseline und Prüfresultate mit echten Exit-Codes, Gate-Urteil und Worktreezustand. Keine Testfreigabe ohne ausgeführte Tests. Keine Markdown-Berichtdateien schreiben; strukturierte Rückgabe an Astra genügt.

Native Workerrolle, ausschließlich `gpt-6.1-sol`. Keine Delegation, Threads, ListAgents/SendMessage, Nutzerfragen, Browser, Secrets/ENV-Dateien oder `ai-coach`. Auftraggeber Astra `c88f4057-c6b3-4c54-8750-addd08b24b42`, Hauptauftraggeber `819f0d87-8d3f-4fbf-8c4c-ada2bf290f5b`.
