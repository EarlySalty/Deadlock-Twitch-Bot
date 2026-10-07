# VOD-Archiv: Uploadstatus erfassen und verständlich anzeigen

status: aktiv, 2026-10-07

## Auftrag und Rolle

[Orchestrator] Nutzerauftrag: „lass das fixen ja und schau wie wie wir den Status erfassen können“. Bezug: unverständliche VOD-Archivliste mit YouTube-Links neben „Letzter Versuch: Noch kein Versuch“. Der Status sitzt am fernen rechten Rand, „Aus der Liste ausblenden“ wirkt wie die Hauptaktion.

Ein zusammenhängendes Paket, Stufe mittel. Du bist Worker und Integrator für diesen Fix. Keine zusätzlichen T3-Threads. Native frische Fixer für die reguläre Gate-Schleife sind erlaubt. Hauptorchestrator, Intent-Thread und Ersteller: d264f838-4a47-4b9e-9bf2-12efa37223f7. Fachbericht und Status in der Taskakte, kein Sessionchat. Hauptorchestrator prüft etwa alle 20 Minuten. Nur Abschluss oder echter Blocker melden.

## Arbeitsstand und Eigentum

- Repo: /home/nathanael/repos/Deadlock-Twitch-Bot.
- Eigener Worktree: /home/nathanael/.worktrees/tb-vod-archiv-status-20261007.
- Branch: fix/vod-archiv-status-20261007.
- Start-HEAD: 2ead4d556327596fcc7d9feeaceb848e910cf8f8, frisch von origin/main geholt. Vor Anlegen dieser Akte sauber.
- Gemeinsamer Checkout ist alt und enthält über 300 fremde Änderungen. Nicht umstellen, bereinigen oder daraus bauen. Fremde Threads, Brain-Arbeit und Worktrees bleiben unangetastet.
- Eigentum: bestehender Archivtab, dessen API-Vertrag und Übersetzungen, Rust-Archiv-Handler, Statusschreibwege im Archivworker/-store und unmittelbar zugehörige Prüfungen. Schemaänderung nur wenn nach Bestandsprüfung nötig, dann neue Migration nach frischem Fetch. Keine angewandte Migration ändern.
- Enger Scope: Uploadstatus und seine Darstellung. Keine neue Archivplattform, kein Clip-Redesign, kein eigener OAuth-/Uploadweg, kein neuer Hintergrunddienst und keine Löschung des Nebenrepos.
- AUFTRAG.md und REGISTER.md gehören dem Hauptorchestrator. Eigene Diagnose, Prüfungen und Abschluss in BEFUND.md, REVIEW.md, ABSCHLUSS.md und status/vod-status/1/<sequenz>.json festhalten. Kein TODO.md schreiben. Nachweise ohne private Nutzerdaten auf den eigenen Branch sichern.

## Bekannter Bestand, vor Umsetzung verifizieren

Graphify wurde befragt. Der aktuelle Quellstand wurde gelesen, kein Browser-Login oder neuer Upload durchgeführt.

- bot/dashboard_v2/src/components/socialmedia/VodArchiveTab.tsx:47: fehlender Zeitstempel wird immer als „Noch kein Versuch“ dargestellt. Das ist keine verlässliche Aussage über einen historischen Upload.
- Derselbe Tab: Statuslabel rechts per justify-between; Links, Teilezahl und letzter Versuch darunter; Ausblenden als auffälliger Standardknopf. Screenshot zeigt keine erkennbare Abschlussmarkierung. Die konkrete CSS-/Viewportursache selbst prüfen, nicht blind als bewiesen übernehmen.
- bot/dashboard_v2/src/api/socialMedia.ts: ArchivedVod enthält status, status_label, last_attempt_at und parts.
- rust/crates/tb-dashboard-api/src/handlers/social_media_vod_archive.rs: list_handler, status_label (um Zeile 98); aktueller Lesevertrag.
- rust/crates/tb-vod-archive/src/store.rs:525 schreibt uploaded_at zusammen mit uploaded. Store behandelt Teile und Fehler.
- rust/crates/tb-vod-archive/src/worker.rs:688 schreibt last_attempt_at; um 1059 drive_uploaded mit uploaded_at.
- Bestehende Route: /twitch/social-media?view=archiv. Alte API /social-media/api/vod-archive und neuer Alias /twitch/social-media/api/vod-archive existieren und liefern ohne Sitzung JSON-401.
- Letzter eigener Prozessnachweis: vier Twitch-Dienste auf 2ead4d55, Archivtext im tatsächlich ausgelieferten Bundle unter bot/analytics/dashboard_v2/dist. Beim ursprünglichen Archivabschluss waren 82 Einträge nachgewiesen. Das ist keine frische Bestandszahl.
- Nutzer-Screenshot lokal: /home/nathanael/.t3/userdata/attachments/d264f838-4a47-4b9e-9bf2-12efa37223f7-ce385bb2-8a45-4987-a5b6-813aac0218b7.png.

## Verbindliches Ziel

1. Status pro VOD sofort links beim Titel erkennbar, unabhängig von Bildschirmbreite. Text plus eindeutiges Icon, nicht allein Farbe. Bestehenden Schwarz-Gold-Stil erhalten. Abgeschlossene Sicherung, laufender Upload, wartend, Teil-Erfolg, Fehler und unklarer Altbestand dürfen nicht gleich aussehen.
2. YouTube und Drive unterscheiden. Alle Teile abgeschlossen ist etwas anderes als ein vorhandener Link für einen Teil. „Erfolgreich“, „vollständig“ oder „gesichert“ nur aus belastbarem Zustand ableiten. Ein Link oder eine Video-ID alleine reicht nicht. Sichtbarkeit privat/öffentlich und Uploadabschluss nicht verwechseln.
3. Statusherkunft prüfen und reparieren: echte vorhandene Daten lesen, Upload- und Fehlerübergänge samt Teile-Status verfolgen. Vorhandene Uploadbestätigungen, uploaded_at, attempts, last_attempt_at und Teil-Daten wiederverwenden. Erfolg und Fehler müssen beim richtigen Schreibübergang gespeichert werden, nicht durch dauernde UI-Heilung oder einen manuellen DB-Patch.
4. Prüfen, welche YouTube-Rückmeldung tatsächlich vorliegt und ob der bestehende zentrale YouTube-Client bereits eine lesende Prüfung von Upload-/Verarbeitungsstatus bietet. Wenn nötig diesen bestehenden Weg nutzen, keine neue Verbindung und kein neues Secret. Persistierte Uploadbestätigung von heutiger Plattform-Verfügbarkeit klar unterscheiden. Frühere unbekannte Zustände ehrlich als unbekannt lassen; keine erfundenen Uploadzeitpunkte. Kein pauschales Polling pro Seitenaufruf, keine unkontrollierte Quotaerhöhung. Größere neue Überwachungsarchitektur zuerst als konkreten Blocker mit Empfehlung melden.
5. Fehlendes last_attempt_at bei fertigen historischen Uploads darf nie „Noch kein Versuch“ erzeugen. Wenn uploaded_at existiert, passenden Abschlusszeitpunkt anzeigen. Wenn nicht, Zeitzeile weglassen oder neutral benennen, nicht aus updated_at einen Uploadzeitpunkt erfinden.
6. Reihenfolge pro Eintrag: Titel, sichtbarer Gesamtstatus und sinnvoller Fortschritt, Datum/Kanal/Dauer und vorhandene Abschlusszeit, danach Ziellinks. Nur relevante Fehlerdetails und nächste Aktion zeigen. „Ausblenden“ bleibt erhalten, aber optisch nachgeordnet. Keine neue Prozentanzeige ohne gemessene Daten. Bei einem Teil kein unnötiges „1 von 1 Teilen“ als Ersatz für den Status.
7. Unbekannte, teilweise, fehlgeschlagene und ältere abgeschlossene Datensätze durch API bis zum gebauten UI prüfen. Keine Erfolgsmeldung bei widersprüchlichen Daten. Falls Inkonsistenz die Backendursache zeigt, diese im bestehenden Rust-Pfad korrigieren.

## Grenzen und Arbeitsregeln

- Produktive Backend-/Workerlogik in Rust, vorhandenen Frontendvertrag anpassen. Keine neue Python-Produktivlogik, kein weiterer Service. Keine Code-Kommentare schreiben. Bestehende Kommentare nicht erweitern; keine sachfremden Löschdiffs.
- Vor Codebestandssuche code-suche laden und Graphify zuerst. Vor Oberflächenarbeit impeccable, bestehende Theme-/Komponentenreferenz lesen, craft-floor unmittelbar vor UI-Edits. Rolle Operate, gezielte Korrektur statt neues Design.
- Vor JEDER Browserarbeit /home/nathanael/Documents/claude-config/wissen/agent-browser.md lesen. Nur /home/nathanael/.local/bin/moli. MUST NOT Brave starten, übernehmen oder als Rückfall nutzen. Persönlichen Browser und laufende Dienste unangetastet lassen. Fehlende Funktionen melden. Desktop und Mobil gebündelt prüfen, maximal eine gemeinsame Korrektur- und Bestätigungsrunde.
- Keine echten VODs erneut hochladen, ausblenden, löschen oder auf Drive kopieren, keine Sichtbarkeit ändern und keine öffentlichen Testposts. Echtdaten lesend; schreibende Proben nur isolierte eigene Wegwerfdaten ohne externe Veröffentlichung.
- NEVER Secrets lesen, ausgeben oder ablegen. Bestehende Infisical-/Clientwege nutzen. Keine erfundene Nutzersitzung, kein Auth-Bypass. Private Daten nicht in Taskberichte oder externe Codierprompts übernehmen.
- Echte Umlaute, natürliche deutsche UI-Texte, keine Gedankenstriche. Keine Community-Ankündigung versenden.
- Cargo per cargo-slot und --jobs 3, Release zusätzlich vom Wrapper serialisiert. Keine selbst gebaute flock-Schleife und keine fremden Compiler stoppen.
- Schutzablehnungen nicht umgehen. Bei ctx_execute_file-Projektrootproblemen reguläre Rootbindung prüfen und konkret berichten, nicht auf einen Nebenkanal ausweichen.

## Beweis und Abschluss

Du darfst den beauftragten Fix regulär committen, den eigenen Branch pushen und nach gültigem Gate nach main integrieren. Keine PRs, kein GitHub-Actions-Gate. Jeder Git-Schritt einzeln mit literalem absolutem Pfad; kein force, kein add -A. Regulären Merge-Gate nutzen, bei BLOCK frischer nativer Fixer und dasselbe Urteilmodell gemäß bestehendem Verfahren, spätestens nach fünf erfolglosen Runden echte Ursache und Empfehlung melden. Zusammengehörigen Rust-Schreibpfad, API und UI gemeinsam prüfen. Keine Umgehung, keine eigene Übersteuerung.

Passende Format-, Clippy-, Compiler- und bestehenden Tests ausführen. Bereits rote Baseline getrennt belegen. Erfolg, Teilerfolg, Fehler, unbekannter Altstand, fehlender Versuchszeitpunkt und vorhandener Abschlusszeitpunkt müssen nachweisbar korrekt angezeigt werden. Kein zusätzlicher Reviewer-Thread, Gate bleibt der Code-Reviewer.

Nach Merge frisches origin/main prüfen, im eigenen Worktree bauen. Deploy über /usr/local/bin/deploy-twitch-release, betroffene Dienste im freigegebenen Weg neu starten. /usr/local/bin/deploy-twitch-release --pruefen ist der lesende Prozessnachweis. Herkunfts-SHA, echte Assets, Journal seit Start, API-Vertrag und sichtbaren Status nachweisen. Keine Fertigmeldung nur mit 200 oder is-active. Den Nutzerfall als Screenshot auf Desktop und Mobil belegen; Auth-/Plattform-Prüfgrenzen offen nennen.

Abschlussbericht nennt Ursache, genaue Statusquellen, was bestätigt wird und was nicht, gemergte SHAs, Prüfungen, Release, UI-Adresse und verbleibende echte Grenzen. Erst nach Live-Beweis eigene Branches/Worktrees mit geprüfter Vorfahrenbeziehung entfernen; wertvolle Nachweise vorher sichern. Gemeinsame Builds und fremde Bäume nicht löschen. Vor Entfernung dieses vom Hauptorchestrator angelegten Worktrees gehört der aktuelle Auftrag samt Register in die gesicherte Historie. Eigenen Thread als allerletzten Schritt per settle --selbst abschließen. Eigentum und volle Worker-ID werden im REGISTER ergänzt.

INTENT[IA-1]: Stufe mittel | Modell sol | Thread d264f838-4a47-4b9e-9bf2-12efa37223f7 | Register: .tasks/2026-10-07-vod-archiv-status/REGISTER.md
BESTAND[BS-1]: teilweise | Fundort: bot/dashboard_v2/src/components/socialmedia/VodArchiveTab.tsx:47 | Anknüpfung: bestehende Archivstatus-, Teile- und Zeitstempelpfade
BRIEFING[WB-1]: Pflichtteile 5/5 | Timer 20 min | Worktree: /home/nathanael/.worktrees/tb-vod-archiv-status-20261007
