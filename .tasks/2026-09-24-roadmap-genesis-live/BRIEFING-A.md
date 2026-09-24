# Auftrag A: Feature-Stammbaum vollständig in die bestehende Admin-Seite integrieren

Intent: aktuelle ChatGPT-Session über codex-mcp. Der Nutzer hat ausdrücklich nachgefasst: „Das ist deine Aufgabe und deploy das, mach alles fertig, dass das alles sauber geht.“ Er will keine weitere ZIP-Übergabe oder Anleitung zum Selbsteinbau.

Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen. Der Opus-4.8-Thread ist vor jeder Bearbeitung an abgelaufener Claude-Anmeldung gescheitert. Du übernimmst als verfügbarer Codex-Worker den bestehenden Implementierungsauftrag. Keine CLI-Anmeldung ändern und keine Secrets lesen. Der tote Thread darf nicht wieder aufgenommen werden. Arbeite ausschließlich im bereits angelegten eigenen Worktree `/home/nathanael/.worktrees/tb-roadmap-genesis-live-20260924`, Branch `feat/roadmap-genesis-live-20260924`, Basis `901744c609ba677558804db504a7ec79de386588`. Keine fremde Arbeit anfassen. Der kanonische Checkout hat fremde untracked Dateien und bleibt unberührt.

## Ergebnis

Ersetze die aktive Roadmap unter `/twitch/admin/roadmap-history/` durch einen schlanken, interaktiven, horizontalen Feature-Stammbaum mit explizitem Ursprung. Nicht erneut nur ein Offline-Paket erstellen. Bestehender Generator `tools/generate_roadmap.py` und stündlicher Dienst `deadlock-roadmap-history.service` müssen die neue Ansicht erzeugen. Bestehende Admin-Authentifizierung und Caddy-Route unverändert erhalten. Bot-Binaries und Datenbank benötigen für diese statische Seite keinen Neustart bzw. keine Migration.

## Bereits geprüfte Quellen

Aktuelles Repo: `/home/nathanael/repos/Deadlock-Twitch-Bot`.
Vorgeschichte: `/home/nathanael/repos/Deadlock-Bots`.
Echter Twitch-Root: Commit `3654f6c73be53fc569da673fb307e7e0c79f2b87`, 21.09.2025, Titel `new Twitch Bot`, legt `cogs/twitch_deadlock/` mit cog.py, dashboard.py, storage.py, twitch_api.py an. Der allgemeine Repo-Anfang 29.08.2025 ist nicht der Twitch-Ursprung. Keinen Start 2024 erfinden.
Import ins eigenständige Twitch-Repo: `fcac673c63a1169940f3d11eb565b89d6bc16b06`, 24.02.2026, `Add twitch_cog package with full source and PG migration`. Es ist eine Folgestufe, nicht der Root.
Vorgeschichte auf Twitch-Pfade `cogs/twitch_deadlock`, `cogs/twitch`, `cogs/twitch_cog` begrenzen und mit Beginn der eigenständigen Quellhistorie am 24.02.2026 abschneiden. Verwende Commit-Metadaten, keine Secrets/Dateiinhalte oder Commit-Bodies auslesen. Vollständige gepinnte Historie verwenden, keine 17-Beispiele-Vorschau.

## Daten

FeatureNode: id, title, description, date (echtes ISO-Datum), category (core|twitch|dashboard|api|community), type (root|major_feature|update|refactor), parentId:string|null; optional commitHash, prUrl, repository, commitIds, spanEnd, provenance.
Genau ein Root `genesis` namens `Twitch Bot Genesis / Core Init`, parentId=null. Jeder weitere Knoten muss über existierende Eltern den Root erreichen. Doppelte IDs, Zyklen, weitere Roots, unbekannte Eltern und Kinder vor ihren Eltern abweisen.
Bestehende Taxonomie `tools/roadmap-history/features.json` und Zuordnung in `history_data.py` weiterverwenden. Redaktionelle Feature-Eltern als solche kennzeichnen, keine erfundenen technischen Abhängigkeiten. Eltern aus frühestem Beleg des gesamten Teilbaums datieren, nicht aus gefilterter Ansicht.
Kleine Commits pro Feature und Typ in festen 14-Tage-Kalenderfenstern bündeln, Pflege getrennt und standardmäßig ausblenden. Kein endloses gap-based Zusammenkleben. Alle Originalmeldungen und Commit-Hashes in Changelog/Details erhalten, Gesamtzählung deduplizieren.
Wichtig: Suche muss auch NICHT-führende Meldungen eines Bündels finden. Datumsfilter müssen überlappende Bündelintervalle berücksichtigen, nicht nur deren ersten Tag; Eltern behalten ihre ursprünglichen Daten und bleiben als Kontext sichtbar.
Für CI und Timer einen gepinnten, checksummengeprüften Legacy-Metadaten-Snapshot ins Repo übernehmen (gerne gzip deterministisch, keine Secrets). Die Vorgeschichte ist abgeschlossen; der Timer muss nicht ein zweites Repo fetchbar haben. Cache darf keinen fehlenden Ursprung erfinden. Fehler lassen die alte publizierte HTML-Datei stehen. Generator pinnt aktuellen Ref einmal, Output atomar. Renderer-Revision und Datenrevision getrennt in Artefakt ausweisen.

## UI

Neutrales tiefschwarzes Dark-Theme, klare Hierarchie, goldener Core-Akzent und unterscheidbare Modulfarben. Keine alten parallelen Lebensdauer-Linien: direkte geschwungene Bézier-Kanten zwischen konkreten Eltern- und Kind-Ankern. Zeit maßstäblich auf X; gleiches Datum gleiche X-Position. Label-Kollisionen nur vertikal lösen.
Karten/Nodes mit lesbarem Titel und Datum. Root und Hauptfunktionen größer, Updates kleiner; leuchtende Anker sparsam. Übersicht zeigt pro Kategorie den jüngsten echten Entwicklungspfad samt allen Eltern, damit die gesamte Historie nicht direkt überlädt. Umschalter Übersicht / Hauptfunktionen / Alle Änderungen, Zweigfokus, Auf- und Zuklappen.
Suche, Kategorie, Feature-Typ, Datum von/bis, Pflege-Schalter, Reset. Smooth Zoom & Pan, Fit-Button, Ursprung-Button, Mini-Map optional. Native SVG + HTML/JS ist ausdrücklich erlaubt und zum bestehenden Self-contained-HTML-Deploy passend. Keine zusätzliche CDN-/Font-/React-Build-Abhängigkeit nötig.
Klick öffnet zugänglichen Detail-Slide-Over/Dialog mit Beschreibung, Datum, Elternlink, vollständigem paginiertem Changelog und Commit-Links ins jeweils richtige Repo. Vorhandene PR-Links übernehmen, beliebige #Nummern nicht als PR erfinden. TextContent, sichere URL-Validierung, JSON gegen </script> escapen, kein innerHTML für Git-Inhalte. Tastatur, Escape, Fokus-Rückgabe, mobile Breite und Zoom testen. Deep-Links mit bisherigen focus/feature möglichst migrieren.

## Integration und Nachweise

Arbeite direkt an den produktiven Roadmap-Dateien. Tote alte Renderer nicht daneben liegen lassen. Bestehende Python-Suites für Zuordnung/Taxonomie erhalten, alte rendererabhängige Node-/Browser-Tests auf den neuen Contract aktualisieren. `.github/workflows/roadmap-history.yml` muss dieselben aktuellen Tests ausführen und mit dem eingecheckten Legacy-Snapshot ohne private Nachbar-Repos funktionieren. Bestehende Entry-Points nach Möglichkeit behalten. Kein CHANGELOG.md.
Installer `ops/systemd/install-roadmap-history.sh` auf erforderliche neue Assets anpassen und Timer-Überschreiben verhindern. Sicherer release-/atomarer Austausch mit Rollback, keine Auslieferung aus verschmutztem/fremdem HEAD. Der Dienst ist oneshot, statische Caddy-Datei `/srv/deadlock-roadmap/index.html`; laufende Bots nicht anrühren.
Führe echte Python-, Node- und Playwright-Prüfungen gegen die VOLLSTÄNDIGE generierte Historie aus, keine Beispielmenge. Screenshots Desktop/Mobil und Bericht in `dist/roadmap-history/`, mit Dateipfad melden. Prüfe Root-SHA/Datum, alle Elternketten, echte Kantenendpunkte, Filter-Suchfälle, große Datenmenge, XSS und keine Seitenüberbreite. Atomare Veröffentlichung auch mit beschädigtem Cache negativ testen.
Dokumentiere knappe repo-nahe Anleitung und Status in dieser Task-Akte. Belege Tests tatsächlich, keine alten Zahlen übernehmen. Verifizierten Commit sofort pushen (-u origin Branch beim ersten Push), PR erstellen/aktualisieren. Kein automatisches Merge/Deploy/Cleanup durch dich: Der heute geladene PR-first-Testhalt steht im Konflikt zur aktuellen Nutzerforderung, der Orchestrator prüft den Freigabeweg separat. Schutz-Hooks nicht abschalten oder umgehen. Keine Secrets ausgeben. Die unvollständige Paketübertragung in Documents/tmp ist kein Input und darf nicht gelesen oder decodiert werden.

Abschlussbericht: vollständiger HEAD, PR, Testzahlen, Browser-/Screenshotpfade, tatsächliche Gesamt-Commit-/Knotenzahl, Root-Beleg, Änderungen am Generator/Timer, offene Hindernisse. Unabhängiges Review folgt nach deiner Fertigmeldung; Autor urteilt nicht selbst als Kritiker.
