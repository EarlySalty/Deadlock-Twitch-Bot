# Gemeinsame Abnahme und Merge-Gate

status: aktiv, 2026-10-08

## Integrierter Stand vor Intent-Abnahme

P und Q sind abgeschlossen, Schreibbereiche eingefroren. Gemeinsamer Aufruf von apply_tiktok_choice unter bestehender Clip-Sperre bestätigt. Zehn Produktivdateien im erlaubten TikTok-/Vorschau-/Queue-Bereich, keine Migration und keine Archivänderung. Gezielter rustfmt mit skip_children=true und git diff --check jeweils Exit 0.

B führte nach P-Abschluss einen tatsächlichen Moli-Lauf gegen die reale TikTokPostDialog-Komponente mit synthetischen API-Antworten aus. Fünf beobachtete Fälle bestanden, Exit 0. SHA256 der Komponente vor und nach dem Lauf: `4c79cd2adc7269577d7b71df02818faac1eae74eebda119f078267e85a28ec46`. Fehlend startet einen POST, Rendering pollt ohne zusätzliche POSTs, Ready lädt Creator-Auswahl einmal und lässt Zustimmung unmarkiert, Fehler kann erneut angefordert werden, Schließen stoppt Polling. Nachweise: browser/results-compact.json, browser/results.json und fünf Layout-PNGs. Eigene Server beendet. Teil-Orchestrator hat Ready- und Fehleraufnahme selbst angesehen.

Die Browser-Fixture belegt keinen echten Video-Renderlauf, keine Medienwiedergabe und keinen TikTok-Aufruf. Ps isolierte Tests prüfen echte Videodateien und die Vorschau-Zustandsmaschine getrennt. Der produktive angemeldete Funktionsbeweis steht nach Deploy noch aus. Der bestehende sichere Dienstzugang ist inzwischen nachgewiesen, siehe ENTSCHEIDUNG-LIVE-ZUGANG.md.

## Intent-Abnahme

Unabhängiger nativer Abnehmer ade1b69626e5137db prüfte committed SHA `a99b2bcf50d5a518dfb8ca7bb24a2fc84d08a205` gegen den Nutzervertrag. Urteil: lokal bereit für den zentralen Gate, keine Intent-Abweichung, kein Fix nötig. Grenzen der Belege wurden ausdrücklich getrennt: synthetische API-Antworten im Moli-Dialog, echte isolierte DB-/Dateiproben separat, produktive angemeldete Wirkung zum Abnahmezeitpunkt noch offen. Bug-/Security-Review wurde nicht vorweggenommen.

## Zentraler Gate

Runde 1 für Base `0ecae1370f1a80d1a101249b5c932663d69be8af`, Head `a99b2bcf50d5a518dfb8ca7bb24a2fc84d08a205`: Exit 0, `[gpt-6.1-sol] ALLOW: No blocking defect is established by the supplied code.` Vier NIT-Hinweise betrafen erneute Aktivierung mehrerer Queuezeilen, veränderte Render-Eingaben, eine vom Review-Zustand verdeckte Kurzaktion und Bildherkunft.

Gezielte Nachprüfung: Der Bildhash ist identisch mit der committed Komponente. Der Uploadpfad besitzt bereits eine Prüfung vorhandener Plattformveröffentlichungen. Das Layout-Schreiben invalidiert dagegen nachweislich keine Vorschau: set_clip_layout_override aktualisiert bisher lediglich layout_override_json. Durch die neue idempotente Wiederverwendung kann nach einer Layoutänderung eine alte Vorschau erhalten bleiben. Dies ist innerhalb von Sollzustand 3 zu korrigieren, auch wenn Runde 1 ALLOW meldet.

Frischer nativer Fixer a5c904a3a7cd6e502, geerbtes freigegebenes gpt-6.1-sol/high, korrigierte die Render-Eingabeinvalidierung und Folgewirkungen für Freigabe und Queue mit Commit `7073de0d09c3356876b311740b8bd97f7732200c`. Vier Rust-Dateien geändert: layout.rs, preview.rs, clip_queue.rs, upload_worker.rs. Keine ursprünglichen Worker wieder zum Bauen aktiviert, kein neuer T3-Thread, kein paralleler Review. Keine Release-Builds oder Deploys begonnen.

## Zweite gemeinsame Abnahme

I nahm `7073de0d09c3356876b311740b8bd97f7732200c` erneut unabhängig am Nutzervertrag ab: lokal bereit, keine notwendige Intent-Korrektur. Browser-Komponentenhash unverändert, vorhandene Moli-Belege weiterhin zuordenbar. Neue Rust-Tests wurden vom Abnehmer angesehen, nicht ausgeführt. Produktive Live-Wirkung bleibt offen.

F1 meldete auf genau diesem SHA den zentralen Gate mit gpt-6.1-sol/high, Exit 0. `/tmp/tb-preview-fixer-gate.log`: `ALLOW: No grounded merge-blocking defect in the supplied changes.` Cache-Nachweis in `/tmp/tb-preview-fixer-gate-cache.log` nennt denselben Kritiker. Zwei NITs: Fehler beim Schreiben der Eingabedatei oder Umbenennen lassen Rendering stehen; vollständige Abdeckung der Render-Eingaben ist im begrenzten Review-Auszug nicht nachweisbar.

F1s Dashboard-Lauf war trotz ALLOW rot: 48 bestanden, ein fehlgeschlagen, null ignoriert. `tiktok_new_choice_after_confirmed_failed_preserves_history_and_snapshot` bekam beim unwrap in social_media.rs:5504 HTTP 500. Dashboard-Baseline noch nicht gemessen. Der separate Social-Media-Gesamtlauf hatte 342 bestanden und einen Fehler; F1 maß 341 bestanden mit demselben Fehler `queue_dedup_und_invalid_platform` am Stand a99b2bcf vor der Render-Korrektur. origin/main 0ecae137 wurde dabei nicht getestet. Damit ist dieser Fehler nicht als vorbestehend zum gesamten TikTok-Auftrag belegt. F2 prüft ihn zusätzlich am ursprünglichen main-Stand und erhält die bestehende Terminsicherheitsprüfung. Diese Zahlen sind keine grüne Gesamtabnahme.

Frischer nativer Fixer F2 `ad69d6a9d298d58b7` misst den betroffenen Dashboard-Test gegen a99b2bcf in eigenem Baseline-Worktree und korrigiert Testaufbau oder bestätigte Regression ohne Abschwächung der Historien- und Freigabeprüfung. Zusätzlich bestätigte der Teil-Orchestrator NIT 1 an publish_render und render_one: Datei-I/O-Fehler werden weitergegeben und geloggt, aber der aktuelle Auftrag erhält keinen Fehlerzustand. F2 korrigiert diesen Pfad unter dem bestehenden claimed_at-Schutz. Abschlussprüfung, unabhängige Intent-Abnahme und zentraler Gate erneut für den finalen SHA. Merge und Deploy bleiben bis zur Klärung dieser roten Validierung angehalten.

## Dritte Gate-Runde und bestehende Tests

F2 commit `7ea50d9c856a7472583da65d886ab4bfdf73358b`: Dashboard-Fixture enthielt eine unvollständige TikTok-Auswahl. Derselbe Fehler wurde an a99b2bcf gemessen: gezielt null bestanden, ein fehlgeschlagen; Suite 48 bestanden, ein fehlgeschlagen. Die Fixture enthält jetzt vollständige Optionen, bestehende Historien- und Snapshot-Assertions erhalten. Der Termin-Test bestand am ursprünglichen main 0ecae137 mit eins bestanden, null fehlgeschlagen. Seine aktualisierte Erwartung erhält einen zukünftigen TikTok-Termin und prüft weiter die ausdrückliche YouTube-Terminänderung.

F2 stellte bei Datei-I/O-Fehlern einen generationsgebundenen Vorschaufehler her. Echte PostgreSQL-, FFmpeg- und Dateiproben. Zwei zunächst rote Social-Läufe zeigten Sperrkollisionen identischer Clip-IDs über Test-Schemas; getrennte Fixture-IDs korrigieren die Kollision ohne Serialisierung oder Abschwächung. Finale Suite: tb-social-media 345 bestanden, null fehlgeschlagen, null ignoriert; Dashboard-Suite 49 bestanden, null fehlgeschlagen, null ignoriert. Nicht vorhandene Paketeigenschaft testing war kein anwendbares Flag. Format- und Diffcheck Exit 0. Rohlogs `/tmp/tb-tiktok-f2-*.log`, eigene Baseline-Worktrees entfernt. Keine Veröffentlichung oder Live-Datenänderung.

Zentraler Gate auf genau 7ea50d9c, gleiche Base 0ecae137, gpt-6.1-sol/high: Exit 1, `/tmp/tb-tiktok-f2-gate.log`: `BLOCK: A stale-preview transition can strand the TikTok dialog.` Nach einmaligem POST verhindert requested.current den nächsten Auftrag, wenn die Vorschau vor dem Poll invalidiert wird und status=null zurückkommt. Polling endet ohne Wiederholungsaktion, während die Anforderungsanzeige stehen bleibt.

Frischer Fixer F3 a801449820d3e978e erhält den UI-Befund und wiederholt die tatsächlichen Moli-Belege gegen die neue Komponente. Frühere Bilder und Hash belegen den Basislauf, nicht die kommende UI-Version. Zwei NITs wurden gezielt bestätigt und eng zusätzlich zugeordnet: Die Review-Aktionsgruppe verdeckt den vorhandenen Abbruch bei wartendem TikTok; ihr Ablehnen-Pfad setzt lediglich skipped, beendet aber keine bestehenden Queueaufträge. Außerdem übernimmt die API einen ausdrücklichen RFC3339-Termin, während queue_upload bei TikTok einen vorhandenen Zukunftstermin ohne Unterscheidung erhält und den ausgewählten neuen Termin verwirft. F3 macht den vorhandenen Abbruch wieder erreichbar und trennt ausdrückliche Terminänderung von bloßer Freigabeerneuerung. Keine allgemeine UI- oder Scheduler-Überarbeitung.

TESTNACHWEIS[TW-1]: F2 Social 345 passed, Dashboard 49 passed, jeweils 0 ignored | Baseline: Dashboard a99b2bcf 1 rot, ursprünglicher main Termin-Test 0 rot. Überlappende frühere Läufe werden nicht addiert.

## Zugang für den Live-Beweis

Die Hauptsession präzisierte den erlaubten Dienstzugang in ENTSCHEIDUNG-LIVE-ZUGANG.md. Der vorhandene run_with_infisical.sh-Launcher mit dem vorhandenen dl-infisical-env übergab den bestehenden internen Zugang ausschließlich im lokalen Prozessspeicher an einen loopback-HTTP-Aufruf. Keine Werte im Modellkontext, in Argumenten oder Dateien, keine ENV-Dateien, keine fremde Prozessumgebung und keine Session-Cookies aus der DB gelesen.

Tatsächlicher lesender Statusaufruf für Clip 124768: HTTP 200, application/json, preview_status=null, ready=false, has_error=false, Launcher Exit 0. Der angemeldete Vorbereitungspfad ist damit sicher erreichbar. Das beseitigt die Zugangsvoraussetzung; nach Deploy steht der tatsächliche Vorschau-POST und sein Endzustand noch aus. Kein Live-POST vor dem Fix erfolgt.

## Gesicherter Abschluss nach dem Neustart

F3: `169b796df1e4172cd2079dec217a2feadc38a896` und `f1275b27732308650042b1d752de34d963052391`. Zentraler Gate ALLOW, `/tmp/tb-tiktok-f3-gate.log`, Exit 0. Fehlende Vorschau nach bereits angefordertem Renderauftrag zeigt jetzt Wiederholung. Späte Antworten aus geschlossenen Dialogen überschreiben eine neue Öffnung nicht. Ausdrückliche Terminwahl ersetzt den TikTok-Termin; implizite Erneuerung erhält einen zukünftigen Termin. Der vorhandene Abbruch bleibt bei approved-Clips erreichbar.

F4: `b9e284c5d03583b093ddb9324168949bf1775b12`. Zentraler Gate ALLOW, `/tmp/tb-tiktok-f4-gate.log`. Belegänderungen, keine zusätzliche Produktivänderung. Ursprüngliche Baseline 0ecae137 und finale npm-Läufe zeigen dieselben fünf Fehler; konkrete Farbfundstellen identisch. Ein zusätzlicher Purity-Lintbefund durch Date.now im TikTok-Button ist neu und ausdrücklich als NIT dokumentiert, nicht als Altfehler. Die Fortsetzung ordnet keine optionale NIT-Runde an.

Der vor dem Neustart gesicherte Intent-Bericht zu f127 bestätigt keine notwendige Intent-Korrektur. Renderer-Abgleich: Quellpfade, Dateigröße und nanosekundengenaue Änderungszeit, Kanalname, Clip-/Customtitel und wirksames Layout sind abgedeckt. Assets und Renderkonstanten sind kompiliert. Kein fehlender veränderlicher DB-Eingang festgestellt. Grenze: gleich große Quelldateien mit identischer Änderungszeit werden nicht durch einen Inhaltsdigest unterschieden.

F4-Moli-Belege enthalten gesetzte Zustimmung vor dem Wechsel von Video und Konto sowie anschließend gelöschte Zustimmung. Synthetische API, keine reale Zustimmung oder Veröffentlichung. Abschließende unabhängige Intent-Abnahme nach dem belegten Abbruch des alten Abnehmers durch I2. Neue main-Commits bis 6937e4a6 betreffen ausschließlich die separate Vollreview-Akte. Gemeinsame Schlussabnahme und Gate werden für den endgültigen Integrations-SHA gesichert.
