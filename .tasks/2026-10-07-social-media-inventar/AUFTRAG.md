# Social-Media-Bestandsaufnahme

Auftraggeber: Haupt-Orchestrator, Thread `d3a1741e-82bc-4a48-865b-2845c663dca7`.
Datum: 2026-10-07. Rolle: Blatt-Worker, nur lesende Recherche. Native Subagenten für abgegrenzte Recherche ausdrücklich erlaubt. Keine weiteren T3-Threads.

## Vertrag

- Vollständige Bestandsaufnahme von Rust-Social-Media, Dashboard/API/Frontend, Migrationen und produktiven Tabellen, Python-Altbestand, Nebenrepos, Aufgabenakten, Branches und Worktrees.
- Jede Softwareeinheit mit Name, Datei:Zeile, Zweck, Klasse, Beleg und Empfehlung. Null Nutzung allein ist kein Schrottbeleg.
- Bausteinkarte für einen späteren Clip-Agenten: vorhandene Signale, Highlight-Messung, Schnitt/Hochkant, Facecam, Untertitel, Vorschau, Titel, LLM, lokales STT, Daten und Lücken. Keine neue Architektur.
- Produktive Anwendung, Konfiguration, Dienste, Datenbanken und fremde Worktrees nicht ändern. Keine Bereinigung ausführen. Nur eigene Berichtsdokumente schreiben.
- Keine Secrets im Klartext lesen oder ausgeben. Nur zusammengefasste Daten, keine Clipinhalte, Transkripte, Community-Nachrichten oder personenbezogenen Zeilen veröffentlichen.

## Geschützte laufende Arbeiten

`fix/social-token-ablauf`, `fix/social-golive-upload-texte` und `feat/social-tiktok-direct-post` sind laufende Aufträge, keine Schrottkandidaten. Anreicherung und Transkription sind seit `e0b0dbaf` bewusst abgeschaltet.

## Ergänzung des Auftraggebers vom 2026-10-07

Python soll als Hauptteil der Bots entfernt werden. Die erste Ergänzung verlangte einen eigenen Abschnitt „Python raus“. Die spätere Präzisierung erweitert und begrenzt den Maßstab: „Nicht-Rust im Dauerbetrieb raus“. Bots, Worker und dauerhaft laufende produktive Dienste müssen Rust sein, auch bisherige Node-Services oder Shell-Daemons. Bereits durch Rust ersetzte Alt-Runtime erhält „entfernen“; nicht ersetzte dauerhafte Botfunktion „nach Rust portieren, dann Nicht-Rust-Runtime entfernen“. Kleine kurz laufende Timer-Skripte mit wenig Arbeit sowie einzelne Prüf-, Einmal-, Verwaltungs- und Testskripte dürfen bleiben und erhalten keinen pauschalen Portauftrag. Der Bericht nennt Prozess oder Unit, sofern vorhanden, sowie Reihenfolge und Größe.

## Ergebnis und Freigabegrenze

Bericht: `.tasks/2026-10-07-social-media-inventar/INVENTAR.md`.
Basis: `origin/main` bei Auftragseingang, `e0b0dbaf662d7680c4ceaa210bf15f1443693cd8`.
Eigener zunächst detached Worktree: `/home/nathanael/.worktrees/tb-social-inventar`.
Bericht-Branch: `docs/social-media-inventar`.

Nur Dokumentation committen und auf den Bericht-Branch pushen. Kein Main-Merge, kein Deploy. Eigenen Worktree nach gesichertem Bericht entfernen. Dem Auftraggeber Berichtpfad und Commit melden. Als letzten Schritt den eigenen T3-Thread mit `t3-thread.py settle --selbst` abschließen.
