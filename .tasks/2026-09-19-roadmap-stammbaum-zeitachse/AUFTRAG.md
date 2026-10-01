# Auftrag: Roadmap-Stammbaum mit echter Zeitachse

Stufe mittel. Worktree `/home/nathanael/.worktrees/tb-roadmap-stammbaum`, Branch `feat/roadmap-stammbaum-zeitachse`.

## Ziel in Nutzerworten

"Ich will einen Stammbaum, an dem ich sehe, welche Features über die Zeitachse dazugekommen sind. Da geht ein neuer Ast auf, wann ich was an dem Feature gemacht habe, welche Änderungen kamen. Eine visuelle Roadmap aus der Vergangenheit."

Seite: `https://admin.deutsche-deadlock-community.de/twitch/admin/roadmap-history/` (statische Einzeldatei, Deploy-Kopie `/srv/deadlock-roadmap/index.html`, erzeugt von `tools/generate_roadmap.py`, Timer `deadlock-roadmap-history.timer`).

## Befund am Ist-Stand

Quelle: `tools/roadmap-history/model.js` (`layoutFamily`, Zeile 176 bis 203), `app.js`, `features.json`, `history_data.py`.

1. Kein Baum über der Zeit. Feature-Knoten stehen links nach Generation (`x = 24 + depth * 248`), ausdrücklich ohne Datumsbezug. Die Zeit beginnt erst rechts daneben als Tabelle. Ein Ast zweigt also nie an dem Datum ab, an dem das Feature entstand.
2. Es ist eine Swimlane-Tabelle: jede Funktion eine starre Zeile mit 170 px, 38 Zeilen untereinander, je Monat eine 264 px breite Karte. Das ergibt eine riesige, fast leere Fläche, in der man scrollt statt zu lesen.
3. Die Stationen sind Monatsbündel mit einem zufälligen Beispiel-Commit als Titel. Man sieht nicht, was sich geändert hat, sondern "42 Commits im März".
4. Die Zuordnung ist grob: 38 Features für 3677 Commits, die Top-Eimer schlucken fast alles (dashboard 649, social 329, raids 327, other 324, runtime 304). 2518 Zuordnungen hängen nur am Commit-Betreff. Unterfeatures sind kaum gefüllt (caster-overlay 1, title-oauth 1, brain 1).
5. Die Historie beginnt am 2026-02-24 mit dem Import-Commit. Fast alle Hauptfeatures "entstehen" deshalb in derselben Woche. Das ist eine Datengrenze und muss ehrlich dargestellt werden (gemeinsamer Stamm "Bestand beim Import"), nicht als 15 gleichzeitige Geburten.

## Soll

Ein Bild im Stil eines Git-Graphen oder U-Bahn-Plans, von links (Februar 2026) nach rechts (heute):

1. Eine durchgehende, maßstäbliche Zeitachse oben (Monate beschriftet, Wochen als feine Striche). Die x-Position jedes Elements ist sein echtes Datum, überall im Bild.
2. Ein Stamm "Twitch Bot". Jedes Feature ist ein Ast, der am Datum seines ersten Commits aus seinem Eltern-Ast herausschwingt (weiche Kurve) und bis zu seiner letzten Änderung weiterläuft. Unterfeatures zweigen genauso aus ihrem Feature-Ast ab. Features aus dem Import starten gemeinsam am Stammanfang und sind als "Bestand beim Start" markiert.
3. Auf dem Ast sitzen Meilensteine als Punkte am echten Datum, nicht Monatskarten. Ein Meilenstein ist eine zusammenhängende Änderung: Commits desselben Features, die zeitlich dicht beieinanderliegen (Lücke kleiner als etwa 3 Tage), werden zu einem Punkt gebündelt. Punktgröße nach Umfang, Farbe nach Änderungsart (neu, Ausbau, Fix). Titel des Meilensteins ist der aussagekräftigste `feat`-Betreff des Bündels, sonst der größte Commit, nie ein zufälliger.
4. Astdicke oder Helligkeit zeigt die Aktivität, damit man auf einen Blick sieht, woran wann gearbeitet wurde. Ruhende Äste laufen dünn und blass aus.
5. Astbeschriftung direkt am Abzweig (Featurename plus Startdatum). Labels dürfen sich nicht überdecken.
6. Kompaktes Layout: Äste werden nicht als feste Zeilen gestapelt, sondern platzsparend in freie Bahnen gelegt (Bahn wird wiederverwendet, sobald ein Ast endet oder lange ruht). Die Gesamtansicht "Alle Zweige" muss auf einem 1920er-Bildschirm ohne vertikales Dauer-Scrollen lesbar sein. Fokus auf ein Feature blendet den Rest ab, statt ihn zu entfernen.
7. Klick auf einen Meilenstein öffnet die bestehende Detailleiste mit allen Commits des Bündels. Hover zeigt Datum, Titel, Anzahl Commits.
8. Die bestehenden Bausteine bleiben: Filter, Suche, Direktlink-Zustand, Änderungsliste, Detailleiste, Einzeldatei ohne Framework und ohne CDN, Look aus `bot/dashboard_v2/src/index.css` (Schwarz-Gold, kein Braun).

## Zuordnung schärfen

- `features.json` feiner schneiden, damit die Sammeleimer kleiner werden: mindestens `dashboard`, `social`, `raids`, `runtime`, `moderation`, `partners` in echte Unterfeatures teilen, abgeleitet aus den Crates und Seiten im Repo (`rust/crates/*`, `bot/dashboard_v2/src/pages/*`). Pfadregeln vor Betreffregeln.
- `other` bleibt als ehrlicher Rest, wird aber im Baum nicht als Ast gezeichnet, nur in der Liste.
- Reine Pflege (Doku, Tests, Abhängigkeiten, Merges) erzeugt keine Meilensteine im Baum.

## Nicht anfassen

- Caddy-Route, systemd-Timer, GitHub-Workflow, Auth der Admin-Seite.
- Keine Portierung des Generators in eine andere Sprache, kein LLM im Generator, keine neuen Abhängigkeiten.
- Kein anderer Bereich des Repos.

## Fertig-Kriterium

- `python3 tools/generate_roadmap.py` erzeugt die Datei ohne Fehler, bestehende Tests (`test_history.py`, `test_family.py`, `model.test.mjs`, `family.test.mjs`, `browser.test.mjs`) sind nachgezogen und grün.
- Screenshots (Gesamtansicht, Fokus Uplink, Fokus Raids, mobil) liegen im Task-Ordner. Rendert Headless-Chrome in der Sandbox nicht, das offen melden statt zu raten.
- README in `tools/roadmap-history/` beschreibt den neuen Aufbau.
- Fertigmeldung mit Branch, Commits, geänderten Dateien. Kein Merge, kein Deploy, das macht der Orchestrator nach dem Review.
