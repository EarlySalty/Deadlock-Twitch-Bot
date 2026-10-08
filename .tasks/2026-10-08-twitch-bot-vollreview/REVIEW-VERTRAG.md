# Vertrag für Review und Gegenprüfung

Stand: 2026-10-08. Dieser Vertrag ergänzt AUFTRAG.md, ohne dessen Grenzen zu erweitern.

## Gemeinsame Grenzen

- Auftraggeber: Astra, T3 `c88f4057-c6b3-4c54-8750-addd08b24b42`. Übergeordneter Auftraggeber: `819f0d87-8d3f-4fbf-8c4c-ada2bf290f5b`.
- Review-Code liegt in `/home/nathanael/.worktrees/tb-vollreview-artefakte`, Branch `audit/tb-vollreview-20261008`. Codebasis ist `0ecae1370f1a80d1a101249b5c932663d69be8af`. Spätere Taskdokument-Commits ändern diese Codebasis nicht.
- Modell je Agent: `gpt-6.1-sol`. Modellnachweis erfolgt durch Astra aus `message.model` im nativen Transcript. Keine andere Modellwahl oder Rückfälle.
- Reviewer und Skeptiker arbeiten strikt lesend. Keine Dateien schreiben, keine Builds oder Tests starten, keine Git-Mutationen, keine produktiven Requests mit Wirkung. Ergebnisse kommen strukturiert im Workflow zurück.
- Vor Codesuche den Skill `code-suche` laden und Graphify abfragen. Der bestehende Graph aus dem Hauptrepo darf per `--graph` gelesen werden. Quellcode dann im isolierten Review-Worktree nachlesen. Graphangaben sind Suchhilfen, keine Beweise.
- Referenzen außerhalb des eigenen Pakets dürfen lesend verfolgt werden, damit Aufrufer, Middleware und Datenbank-Constraints geprüft werden. Fremden Hauptcheckout weder als aktuellen Code behandeln noch verändern.
- Keine Secrets, keine ENV-Dateien, keine privaten Kontodaten lesen oder ausgeben. Keine Prod-DB-Schreibvorgänge oder Migrationen. Keine echten Streamer-Kontenaktionen. `ai-coach` ist ausgeschlossen. Python ist lesbare Legacy-Referenz und kein Fixziel.
- Keine eigenen Agenten oder T3-Threads; kein `ListAgents` oder `SendMessage`. Keine Browserarbeit. Sollte sie später ausdrücklich beauftragt werden, gilt ausschließlich Moli nach Lektüre von `agent-browser.md`; Brave ist verboten.
- Keine Nutzerfragen. Unlösbare Grenzen oder unvollständige Abdeckung an Astra im strukturierten Resultat melden. Kein vorzeitiges Urteil „alles geprüft“, wenn Quellen fehlen.

## Reviewer

Jede Kombination aus Paket und Blickwinkel erhält einen frischen Reviewer. Paketdefinitionen liegen in `pakete.json`; die konkrete Paket-ID und der Blickwinkel stehen im Agentenauftrag.

1. Security: Authentifizierung, Rechteprüfung, fremde Identitäten, Sessions, SSRF, SQL- und Command-Injection, Pfade, OAuth, Signaturen, CSRF, Secrets, Prompt-Injection mit Außenwirkung und fail-open-Grenzen.
2. Korrektheit: konkrete falsche Ergebnisse, Einheiten, Vergleiche, Grenzen und stabile Plattformidentitäten.
3. Fehlerbehandlung: Fehlerverlust, fälschlicher Erfolg, Panics durch externe Daten und nicht funktionierende Wiederherstellung.
4. Nebenläufigkeit und Daten: belegte Races, doppelte Wirkung, Transaktionen, Idempotenz und verlorene Änderungen.
5. Ressourcen: unbeschränkte Schleifen, Speicher, Warteschlangen und Wiederholungen sowie fehlende Zeitgrenzen mit konkretem Fehlerszenario.

Der Reviewer liest die dem Paket zugeordneten Quellen vollständig genug für den Blickwinkel. Stichproben, nicht gelesene Dateien und nur strukturell geprüfte Daten sind gesondert zu nennen. Keine feste Befundobergrenze. Ein leeres Ergebnis ist zulässig, aber kein Ersatz für Abdeckung.

Jeder Befund enthält eine kurze eigenständige Behauptung, repo-relative Datei und Zeile, A/B/C-Vorschlag, konkretes Eingabe-/Zustands-Szenario, Codebeleg, belegtes Sollverhalten und bekannte Gegenschutzschichten. Stil, Refactoring, unklare Produktabsicht und Migrationen bleiben C. Performance ohne Fehlerbild ist kein A/B-Befund.

## Unabhängige Skeptiker

Jeder A/B-Kandidat erhält zwei frische Skeptiker. Sie sehen ausschließlich die kurze Behauptung sowie Datei und Zeile. Sie erhalten weder die Begründung des Reviewers noch dessen Szenario, Klasse, Vorschlag oder das Urteil anderer Skeptiker.

Aufgabe ist Widerlegung, nicht Zustimmung: Erreichbarkeit, Aufrufer, Validierung, Proxy, Middleware, Constraints, beabsichtigtes Verhalten und vorhandene Tests unabhängig prüfen. Mögliche Urteile:

- `BESTÄTIGT`: Defekt und konkreter erreichbarer Fehlerfall belegt.
- `PLAUSIBEL`: mindestens eine entscheidende Voraussetzung ist nicht belegt.
- `WIDERLEGT`: konkrete Abwehrschicht, Unerreichbarkeit oder beabsichtigtes Verhalten widerlegt die Behauptung.

Klasse B erfordert zusätzlich ein eindeutiges Sollverhalten mit Fundstelle. Beide Skeptiker müssen `BESTÄTIGT` liefern. Ein fehlendes Ergebnis zählt nicht als Zustimmung. Unterschiedliche Interpretationen werden nicht durch Mehrheitsrhetorik aufgelöst.

## Weitergabe an Fixer

Die Review-Workflows ändern keinen Code. Erst nach Prüfung der Modellnachweise, Zusammenführung doppelter Befunde und Sichtung beider Skeptikerurteile entscheidet Astra über das Fixpaket. Geteilte Schreibpfade bleiben beim selben Fixer. Zwillinge benötigen dieselbe Beweiskette; sie werden nicht beiläufig korrigiert.

Fixer, Fix-Kritiker, lokaler Merge-Gate, Merge, Deploy und Live-Nachweis folgen AUFTRAG.md. Nicht behobene oder unklare Befunde bleiben mit Urteil in BEFUNDE.md.
