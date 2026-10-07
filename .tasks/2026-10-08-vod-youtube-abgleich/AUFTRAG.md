[Orchestrator]
# VOD-Archiv: YouTube-Abgleich

status: aktiv, 2026-10-08

## Nutzerfreigabe und Ziel

Der Nutzer hat den Plan `/home/nathanael/.claude/plans/noble-yawning-charm.md` ausdrücklich mit „Ja umsetzen“ freigegeben. Lies den gesamten Plan und sichere seine Inhalte unverändert als PLAN.md in dieser Task-Akte. Umsetzung bis Gate, Merge, Push, Migration, Deploy, Neustart, Live-Abgleich und eigenem Cleanup ist beauftragt. Kein erneutes Plan-Go.

Der vorherige Statusfix ist abgeschlossen und live auf b0bd68248c3accc1771e938e6166c3a122ac154e; dessen Abschlussdoku liegt in main 0ecae137. Er zeigt ungeklärte historische Uploads nur ehrlich an. Jetzt soll YouTube selbst Auskunft liefern. Letzte belegte Aggregate: 82 VODs, 76 mit gespeicherten vollständigen Uploadnachweisen, fünf unklare Altfälle und ein nicht verfügbares Twitch-VOD. Nicht mit aktuellen YouTube-Bestätigungen verwechseln. Die fünf Fälle sollen tatsächlich geprüft werden, nicht nur ein neuer Status gebaut werden.

## Rolle und Eigentum

Du bist ein einzelner Implementierer und Integrationsverantwortlicher für dieses zusammenhängende Paket Y. Keine neue T3-Hierarchie oder zusätzliche Orchestratoren. Bestehende Schutz-/Gate-Fixregeln gelten. Direkter Auftraggeber und Hauptorchestrator: T3-Thread `d264f838-4a47-4b9e-9bf2-12efa37223f7`, Claude-Session `9fffdbdc-3f14-4f4a-b93d-4437d1133cc6`.

Worktree: `/home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008`
Branch: `feat/vod-youtube-abgleich-20261008`
Basis: frisch geholtes origin/main `0ecae137` (vollen SHA selbst verifizieren).
Ausgangszustand: Code sauber, ausschließlich diese initialen Task-Dateien uncommittet. Eigene Dateien einzeln adden. Commit/Push und regulärer Abschluss auf main sind freigegeben. Niemals den dreckigen Kanon oder fremde Worktrees verändern, fremde Threads bleiben unangetastet.

Zusammengehöriger Schreibbereich: bestehender YouTube-Client in tb-social-media, Archiv-Worker/Store/Metadaten/Config in tb-vod-archive, Archiv-API samt Typen/Anzeige/Übersetzungen, neue notwendige Migration, passende bestehende Tests und sqlx-Daten sowie diese Task-Akte. Kein TikTok, keine Drive-Erweiterung, keine allgemeine Dashboard- oder Auth-Neugestaltung. Der fremde TikTok-Thread ist keine Abhängigkeit und wird nicht koordiniert. OAuth nur soweit ein tatsächlich fehlendes Recht im EXISTIERENDEN Weg nötig wäre.

## Bestand und verbindliche Leitplanken

Graphify wurde im Vorcheck befragt; vor eigenen Code-Suchen ebenfalls Skill code-suche und Graphify nutzen. Bereits verifiziert auf Basis origin/main:
- `tb-social-media/src/uploaders/youtube.rs`: YouTubeUploader::video_status, get_videos und gemeinsamer call/Refresh-Fehlerpfad.
- `tb-social-media/src/upload_worker.rs`: youtube_uploader.
- `tb-social-media/src/oauth.rs`: YouTube-Login fordert bereits youtube.upload und youtube.readonly an; tatsächliche Altberechtigungen können abweichen.
- `tb-vod-archive/src/worker.rs`: pruefe_fruehe_uploads; markiere_verworfen kann einen Neu-Upload auslösen und ist deshalb KEIN unverändert brauchbarer historischer Prüfpfad.
- `tb-vod-archive/src/store.rs`: frisch_hochgeladene_teile, Teile-Persistenz, Abschlussguard.
- `tb-vod-archive/src/metadata.rs`: Beschreibung enthält Original-Twitch-VOD-Link. Bei Altvideos nicht blind voraussetzen. Gleichnamige Streams am selben Tag sind in den Nutzerscreenshots ausdrücklich vorhanden.

Prüfnachweis, historischer Uploadabschluss und öffentliche Sichtbarkeit sind verschiedene Zustände. Bekannte IDs gesammelt prüfen; fehlende Zuordnung über Upload-Playlist des exakt verbundenen Kanals, keine öffentliche Suche oder Namensheuristik. Teilabdeckung muss vollständig belegt sein. Keine erfundenen Uploadzeiten, Teile, Pfade oder Erfolgsmeldungen. Nicht gelistet/privat ist kein Fehler. Bei fehlenden IDs/Rechten nicht Löschung behaupten. Unvollständige paginierte Suche nie als endgültig erfolglos speichern. Konto-/Kanalwechsel und parallele Uploads bei Persistenz schützen. Keine Uploads durch den Prüfknopf oder historischen Abgleich. Lokale letzte Kopien bei ungesichertem Ziel erhalten.

Neue Beobachtungen im regulären Rust-Pfad in Postgres speichern, keine manuellen Statuskorrekturen in Prod. Wiederverwendung vorhandener Credentials, Scopes, Transport, Worker und Dashboard-Aktionen. Intervalle/Quotenbudget in normaler Config, keine neue ENV, keine neue Unit, kein externer KI-Aufruf. Spezifische Quellen-/Teilkennzeichnung in zukünftigen Uploadmetadaten darf nicht durch Titel-Längenkürzung verschwinden; echte Bestandsvideos bleiben unverändert.

## Nachweise und Abschluss

Beweisziel: API und UI zeigen echte gespeicherte YouTube-Prüfergebnisse, Links und Prüfzeit, plus geschützten entprellten Prüfknopf. Worker prüft regelmäßig und bestätigt die historischen Fälle nur bei hinreichender Evidenz. Reale Read-only-Abfrage über den bestehenden Zugang inklusive Ergebnis pro historischem Fall erforderlich; keine bloße Mock-Fertigmeldung. Bei fehlendem Scope den tatsächlichen Blocker und den vorhandenen Selbstbedienungsweg nennen.

Passende fmt/clippy/Build/bestehende Tests ausführen. Bei roten Baselines beide Stände tatsächlich messen, nicht behaupten. DB-Tests mit isolierter Postgres-DB und realen Writes, kein Null-Lauf. Statusübergänge, Quota, 401/403, Timeout, privat, processing, fehlend, Mehrteiler, gleiche Titel, Dubletten und Kanalwechsel berücksichtigen. Keine pauschale neue Testsuite bauen.

Moli ist der einzige freigegebene Browser: vor Browserarbeit `/home/nathanael/Documents/claude-config/wissen/agent-browser.md` lesen. MUST NOT Brave starten, übernehmen oder als Fallback verwenden. Persönlicher Browser bleibt unangetastet. Desktop/Mobil gebündelt prüfen, höchstens eine Korrektur-/Bestätigungsrunde. Keine erfundene Nutzersitzung oder Auth-Umgehung. Anonyme Produktionsprüfungen nicht als eingeloggten End-to-End-Beweis ausgeben.

NEVER Secrets lesen, ausgeben oder ablegen. Keine ENV-Dateien, Secrets nur im bestehenden Infisical-/Credentialpfad. Nutzer- und Community-Daten nicht an externe Codiermodelle geben; technische Aggregate und bereinigte Belege genügen. YouTube-Aufrufe ausschließlich am verbundenen eigenen Kanal im beauftragten API-Umfang. Keine echten VODs hochladen, löschen, ausblenden, umbeschriften oder deren Sichtbarkeit ändern. Keine Community-Ankündigung.

Produktive Verarbeitung Rust, vorhandene React-Oberfläche im bestehenden Frontend erweitern. Keine neuen Code-Kommentare. Bestehende Kommentare nicht erweitern; unnötige Löschdiffs vermeiden. Echte Umlaute und natürliche deutsche UI, keine Em-Dashes. Kein Sonnet, kein ungefragter Modell-/Anbieterwechsel. Gates niemals umgehen.

Alle Prüf- und Releasebuilds nach HOSTPROBE/RUST-BUILDS über cargo-slot mit --jobs 3, keine eigenen Locks oder Build-Warteschleifen. Logs in eigener Task-Akte, MCP-Root-Ablehnungen nicht umgehen. Release aus eigenem Worktree mit sauberer Provenienz, alle Binaries auf exakten SHA ohne dirty prüfen. Vor Deploy aktuelle Herkunft per deploy-twitch-release --pruefen belegen. Regulärer Deploy-Wrapper serialisiert; eigener Release aus aktuellem origin/main. Migrieren im bestehenden autorisierten Weg, kein SQL-Handbackfill. Main-Git-Schritte einzeln, literale Pfade, HEAD:main; kein add -A, kein force, kein fremder HEAD-Eingriff. Branchlöschung nur nach geprüfter Ancestor-Abfrage samt Exit-Code.

Nur Merge-Gate ist Bug-/Security-Reviewer, dessen gültige Regeln beachten. Fachliche Abnahme gegen das Nutzerziel im Abschluss gesondert belegen. Bei BLOCK frischer Fixkontext gemäß Gate-Regeln, bis ALLOW; nach fünf erfolglosen Runden echten Blocker mit Vorschlag melden. Kein neuer Review-T3-Thread. Fertig erst mit wirklichem Merge/Push, Migration/Deploy, Dienstneustart, Prozess-/SHA-/Journal-/Funktionsbeweis und Cleanup. Vor Cleanup wertvolle Artefakte sichern und Berichte auf Git pushen. Als allerletzten Schritt eigenen Thread mit `t3-thread.py settle --selbst` abschließen.

## Register, Status und Rückmeldung

Hauptorchestrator hat REGISTER.md initial angelegt und übergibt dir mit dem Threadstart EINMALIG die alleinige weitere Schreibhoheit für diese Datei, damit keine parallelen Registry-Edits den Releasebaum dirty machen. Trage eigene vollständige T3-/Session-ID und Startnachweis ein. Niemand sonst editiert deinen Worktree während des Builds. Andere zentrale Register/TODO-Dateien nicht anfassen.

Statusereignisse gemäß ABLAUF.md unveränderlich unter `.tasks/2026-10-08-vod-youtube-abgleich/status/youtube/1/<sequenz>.json`. Produzent nur du, Paket youtube, Versuch 1. Erstes Ereignis zeitnah nach echtem Start. Gebaut/reviewt/gemergt/live getrennt. Meldung nur bei fachlicher Entscheidung, echtem Blocker oder Abschluss. Rückfrage als FRAGE AN ORCHESTRATOR mit Lage und Empfehlung, nicht an Nutzer eskalieren, was du selbst prüfen kannst.

Final `ABSCHLUSS.md`: tatsächliche Upload-/Prüfzustände, je Altfallsentscheidung bereinigter Beleg ohne private Inhalte, Testzahlen/Baseline, Review, Main-/Release-SHAs, Services/Liveproben, Einschränkungen und Cleanup. Pflichtzeilen TESTNACHWEIS[TW-1], MERGEPROTOKOLL[MS-1] und LIVEBEWEIS[DV-1] mit echten Zahlen. Nicht nur „Plan umgesetzt“ melden.
