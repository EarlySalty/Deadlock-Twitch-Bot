# Auftrag: Twitch-Bot einmal komplett reviewen und sauber machen

Stand: 2026-10-08. Auftraggeber: Nutzer über die Claude-Hauptsession `819f0d87-8d3f-4fbf-8c4c-ada2bf290f5b` (Haupt-Orchestrator). Ausführend: Astra als Orchestrator dieses Auftrags.

## Ziel

Der gesamte Twitch-Bot (Repo `~/repos/Deadlock-Twitch-Bot`, Basis origin/main `0ecae137` oder neuer) wird einmal vollständig und aus allen Blickwinkeln geprüft. Jeder Befund wird kritisch gegengeprüft, bevor er zählt. Danach gilt:

| Klasse | Was | Umgang |
|---|---|---|
| A | bestätigte Sicherheitslücke | automatisch fixen, mergen, deployen |
| B | offensichtlicher Bug, bei dem zu 100 % klar ist, wie es richtig funktionieren soll (aus Code, Tests, Doku, Namen oder Vertrag belegbar) und der Fix nur das Fehlverhalten entfernt | fixen, mergen, deployen |
| C | alles andere: Umbau, Refactoring, Design, Stil, Performance ohne Fehlerbild, Verhaltensänderung, unklare Absicht, Produktfrage, Migration nötig | nur dokumentieren, nicht anfassen |

Im Zweifel ist es C. Der Nutzer will ausdrücklich keine Verhaltensänderungen und keine Umbauten. Ein Fix ändert genau das fehlerhafte Verhalten und sonst nichts.

## Modelle

- Astra (`gpt-6-astra`) orchestriert, plant, verteilt, überwacht und entscheidet. Astra baut nicht selbst.
- Alle Reviewer, Skeptiker, Kritiker und Fixer sind ausschließlich `gpt-6.1-sol`. Kein Opus, kein Fable, kein Sonnet, kein GLM, kein Luna.
- Das Modell jedes Subagenten am Transcript belegen (Feld `message.model` in den Subagent-Transcripts unter `~/.claude/projects/<projekt>/<session>/subagents/`). Läuft ein Agent mit einem anderen Modell, Workflow anpassen und den Agenten wiederholen.

## Ablauf

### 1. Inventar und Schnitt

Bereiche bilden, zum Beispiel je Crate unter `rust/crates/` (26 Crates), dazu `rust/bin/`, `bot/dashboard_v2`, `website/`, Migrationen, `ops/` und `scripts/`. Erst `graphify query` / `affected` / `explain` nutzen (Skill `code-suche`, `graphify-out/` liegt im Repo), dann lesen. Angriffsfläche kartieren: HTTP-Routen und ihre Auth, OAuth-Flows und Callbacks, EventSub- und Webhook-Eingänge samt Signaturprüfung, Chat-Eingaben, LLM-Prompts aus Nutzertext, SQL, Secrets-Zugriff, Prozess- und Dateiaufrufe, Dienst-zu-Dienst-Tokens.

Ergebnis in `PAKETE.md`: Bereich, Dateien, Größe, Angriffsfläche, Priorität (öffentlich erreichbare Pfade zuerst).

### 2. Review: Bereich mal Blickwinkel

Je Kombination aus Bereich und Blickwinkel ein Reviewer-Agent, parallel per Workflow (etwa 14 gleichzeitig sind belegt), strikt read-only.

Blickwinkel:
1. Security: AuthN/AuthZ, IDOR (fremde Twitch-User-ID oder Kanal), Session aus Client statt Server, SSRF, SQL- und Command-Injection, Pfad-Traversal, OAuth-State und Redirect, Webhook-Signaturen, CSRF, Secrets in Logs oder Antworten, Prompt-Injection mit Wirkung, fail-open-Gates.
2. Korrektheit: offensichtliche Logikfehler, falsche Vergleiche, Off-by-one, falsche Einheiten, Identität über Login-Namen statt Plattform-ID.
3. Fehlerbehandlung: verschluckte Fehler, Erfolg ohne Wirkung, `unwrap`/`expect`/Panics auf externen Daten.
4. Nebenläufigkeit und Daten: Races, doppelte Ausführung, Transaktionsgrenzen, Idempotenz.
5. Ressourcen: unbegrenzte Schleifen, Puffer oder Retries, fehlende Timeouts an externen Aufrufen.

Jeder Befund braucht: Datei:Zeile, Klasse (A/B/C-Vorschlag), konkretes Fehlerszenario (Eingabe oder Zustand führt zu falschem Ergebnis), Beleg aus dem Code. Ohne Fehlerszenario kein Befund.

### 3. Gegenprüfung (Skeptiker)

Jeder A- und B-Befund geht an zwei frische, voneinander unabhängige Sol-Skeptiker. Sie bekommen nur die Behauptung und den Ort, nicht die Begründung des Reviewers, und versuchen, den Befund zu widerlegen: Wird der Pfad wirklich erreicht? Fängt eine andere Schicht (Caddy-Allowlist, Middleware, DB-Constraint, Aufrufer) das ab? Ist das Verhalten vielleicht gewollt? Urteil je Skeptiker: BESTÄTIGT, PLAUSIBEL oder WIDERLEGT, mit Beleg (Codepfad, Test oder reproduzierter Fall).

- Beide BESTÄTIGT: Befund gilt.
- Einer WIDERLEGT: Befund wird C oder fällt weg, Begründung in `BEFUNDE.md`.
- B-Befunde zusätzlich: Ist das gewollte Verhalten zu 100 % belegt? Wenn nicht, C.

Doppelte Befunde zusammenführen und Zwillinge suchen (dasselbe Muster an anderer Stelle).

### 4. Fixen

- Je zusammenhängendem Fix-Paket ein frischer Sol-Fixer, eigener Worktree unter `~/.worktrees/tb-vollreview-<paket>`, Branch `fix/vollreview-<paket>` von aktuellem origin/main. Getrennte Schreibpfade, nie zwei Fixer an derselben Datei.
- Minimaler Diff. Keine Umbenennung, kein Refactoring, keine Aufräumarbeit nebenbei, keine neue Konfiguration, kein Modellwechsel, keine neue Abhängigkeit ohne Not. Keine Code-Kommentare.
- Braucht ein Fix eine Migration, eine Verhaltensänderung für korrekte Eingaben oder eine Produktentscheidung: nicht fixen, als C mit Empfehlung dokumentieren.
- Python-Code ist Legacy: dort nichts fixen, Befunde nur dokumentieren.
- Nach dem Fix `cargo fmt`, `cargo clippy` und die bestehenden Tests der betroffenen Crates (Konventionen siehe unten). Wer Tests bricht, zieht sie nach. Neue Tests nur, wo sie den Fix billig belegen.

### 5. Fix-Kritiker

Je Fix-Paket ein frischer Sol-Kritiker, der den Diff gegen den Befund prüft: Ist der Defekt wirklich weg? Ändert sich Verhalten über den Defekt hinaus? Neuer Bug? Zwilling übersehen? Bei Mangel ein frischer Fixer, dann erneut Kritiker. Der Fixer prüft sich vor der Abgabe selbst mit `gate_hook.py --review`.

### 6. Merge, Deploy, Live

- Lokaler Merge-Gate ist das einzige Merge-Gate. Bei BLOCK je Runde ein frischer Sol-Fixer, bis ALLOW. Skill `rolle-merge-schleuse` laden. Ein Git-Schritt je Bash-Aufruf, literale absolute Pfade, `git push origin HEAD:main`.
- Kleine Merges je Paket statt eines Riesen-Diffs (unter etwa 150 KB je Gate-Lauf).
- Deploy über `/usr/local/bin/deploy-twitch-release <sha>` vom aktuellen origin/main-SHA, Release nur im eigenen Worktree bauen. Danach Dienste prüfen (`deadlock-twitch-bot-rust`, `deadlock-twitch-dashboard-rust`), Journal auf Fehler lesen, betroffene Pfade live prüfen. Skill `rolle-deploy-verifizierer`.
- Danach Branches und Worktrees löschen (vorher `git merge-base --is-ancestor` mit Exit-Code prüfen).

## Harte Grenzen

- Keine Verhaltensänderung, kein Umbau, keine neuen Features.
- Keine schreibenden Prod-DB-Befehle, keine Migration, keine Änderung an schon angewandten Migrationen.
- Secrets nie lesen oder ausgeben. Keine ENV-Dateien.
- Keine Live-Tests mit echten Streamer-Konten (kein Trennen, kein Rotate, keine Bans).
- `ai-coach` nicht anfassen. Python nicht erweitern.
- Keine Session-zu-Session-Koordination: kein `ListAgents`, kein `SendMessage`.
- Browserprüfungen nur mit Moli (`~/.local/bin/moli`), nie Brave; vorher `~/Documents/claude-config/wissen/agent-browser.md` lesen.
- Keine weiteren T3-Threads anlegen; Unteragenten nur als native Subagenten oder Workflows in dieser Session.

## Repo-Wissen (vor dem Bauen lesen)

- `~/repos/Deadlock-Twitch-Bot/CLAUDE.md`, `AGENTS.md`, `WORKFLOW.md`, `SECURITY.md`
- `~/Documents/Docs/workspace/twitch-bot.md`, `~/.claude/wissen/repos-und-git.md`
- Memories unter `~/.claude/projects/-home-nathanael-Documents/memory/`: `tb-bot-build-toolchain.md` (rustc 1.97, nicht `/usr/bin/cargo`), `twitch-release-deploy-weg.md`, `twitch-deploy-wrapper-codex-mcp.md`, `merge-gate-lokaler-main-zeiger.md`, `twitch-titel-seite-und-checkout-drift.md` (geteilter Checkout driftet, aus eigenem Worktree oder frischem Klon arbeiten), `cargo-slot-statt-flock-schleife.md`
- Test-DB: tb-db- und tb-raid-Tests gegen den Docker-Container aus `rust/scripts/test_db.sh`, übrige Crates mit `TB_TEST_DATABASE_URL=postgres:///tb_bb_test?host=/var/run/postgresql` und `SQLX_OFFLINE=1`. Vorbestehende rote Tests als Baseline messen, nicht als neuen Fehler melden.
- Neue öffentliche Pfade gibt es in diesem Auftrag nicht; Caddy nicht ändern.

## Artefakte (in diesem Ordner, committen und pushen)

- `REGISTER.md`: Session-Register, Workflows, Worktrees, Branches, SHAs, Status.
- `PAKETE.md`: Bereiche und Angriffsfläche.
- `BEFUNDE.md`: jeder Befund mit ID, Ort, Klasse, Szenario, Skeptiker-Urteilen, Status (gefixt mit Commit, dokumentiert, widerlegt).
- `REVIEW.md`: Fix-Kritiker- und Gate-Runden.
- `BERICHT.md` am Ende.

## Abschlussbericht an den Haupt-Orchestrator

Kurz: Anzahl Befunde je Klasse, gefixte Befunde mit Commit und Deploy-SHA, Live-Nachweis, widerlegte Befunde (Zahl), offene C-Befunde mit Urteil und Empfehlung, geordnet nach Risiko. Dazu `MERGEPROTOKOLL[MS-1]`- und `TESTNACHWEIS[TW-1]`-Zeile. Nutzertexte ohne Em-Dashes.

Rückfragen nur bei echten Produkt- oder Architekturentscheidungen. Alles andere selbst entscheiden und im Bericht begründen. Steckt der Auftrag fest (Kontingent, kaputter Build auf main, Gate liefert dauerhaft kein Urteil), Stand in `REGISTER.md` sichern, pushen und melden.

## Nachtrag 2026-10-08: Bauqualität bewerten

Der Nutzer will zusätzlich wissen, wie gut der Code gebaut ist. Das ist reine Bewertung, kein Bauauftrag: daraus entsteht keine Änderung, alles läuft als C.

Je Bereich ein sechster Blickwinkel „Bauqualität“, von Sol-Reviewern bewertet und von einem Sol-Kritiker gegen Übertreibung geprüft:

- Struktur: klare Zuständigkeiten je Crate, Kopplung, Schichtung, zyklische oder versteckte Abhängigkeiten.
- Doppelte Pfade: dasselbe zweimal gebaut (zwei Wege für dieselbe Aufgabe, parallele Clients, verwaiste Module und tote Features).
- Fehler- und Zustandsmodell: einheitliche Fehlertypen, Konfiguration statt harter Konstanten, Altlasten wie ENV-Konfiguration.
- Testbarkeit und Testlage: welche kritischen Pfade ungetestet sind.
- Wartbarkeit: Riesendateien und Riesenfunktionen, Namensklarheit, Python-Reste im Produktivpfad.

Ergebnis in `QUALITAET.md`: je Bereich eine Note von 1 bis 5 mit kurzer, belegter Begründung (Datei:Zeile), dazu eine Gesamteinschätzung und die fünf Umbauten mit dem größten Nutzen, geordnet nach Nutzen und Risiko. Nur Empfehlung, nichts davon umsetzen. Die Zusammenfassung kommt in den Abschlussbericht.
