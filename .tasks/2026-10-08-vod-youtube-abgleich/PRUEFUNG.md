# Prüfnachweise

Stand 2026-10-09. Eigentum, IDs, Quellen-/Integrations-SHAs und Gatechronik stehen einmalig in [REGISTER.md](REGISTER.md), Abschlussgrenzen in [HANDOFF.md](HANDOFF.md). Originale Logs, Gate-/Testdateien und REVIEW.md unverändert. Ausführliche Vorfassung: Git-Checkpoint31e8c4557aec156de06c27945186eaae3e887442. Historische Ergebnisse gelten für ihren tatsächlichen Quellstand, nicht automatisch für aktuelle Quellen.

## Aktueller gemeinsamer Belegvertrag

Quellenb38e2051, ausschließlich Fixture1bfab489, integrierter Gate-/Clippy-HEAD863ff0ff. Vollständige SHAs im Register. Wörtliche aktuelle Befehle, synthetischer DSN und Einzelergebnisse in [r13-pruefungen.txt](pruefung/r13-pruefungen.txt). Jeder Testaufruf regulär über cargo-slot mit --jobs3, --include-ignored und --nocapture.

| Tatsächlicher Lauf | passed/failed/ignored; filtered | Exit, Aussage und Originalbeleg |
| --- | --- | --- |
| tb-vod-archive, erster vollständiger Versuch | 66/5/0; 0 | 101, 71 gestartet, Kompilation6m06s; fünf Workerfälle: synthetic PostgreSQL required, zusätzlicher TB_TEST_DATABASE_URL fehlte laut Fixer. Neue echte Rust-Clientfolge60+60→HTTP503→dauerlose Ablehnung bestand; r13-archive-first.txt |
| tb-vod-archive worker::tests, korrigierter synthetischer DSN | 15/0/0; 56 | 0; tatsächliche DB-/Vorbereitungs-/Worker-/Upload-/Wiederholungsfolgen; r13-worker.txt |
| tb-dashboard-api social_media_vod_archive::tests | 0/0/0; insgesamt1385 | 0, falscher Filter, ausdrücklich kein Testnachweis; r13-api-zero.txt |
| tb-dashboard-api vod_archive_management::tests | 15/1/0; 1344 | 101; verlangte echte API→Worker-Gegenfolgen bestanden. Rote Anzeige-Fixture behauptete complete ohne Beobachtungen; r13-api-contract.txt |
| youtube_evidence_keeps_drive_success_and_upload_recovery_visible nach legitimer Fixturekorrektur | 1/0/0; 1359 Bibliothek +25 weitere Ziele | 0; negativer Kontrollpunkt ohne Beleg, positiver mit echten quellgebundenen processed-Beobachtungen zu bekannten IDs. Produktivcode unverändert; r13-api-fixture.txt |
| kombinierter Clippy API+Archiv, --all-targets --no-deps | kein Testlauf | 0, Slot4/3m31s; 21 API-lib-, 25 lib-test-Warnungen einschließlich21 Duplikaten und eine Beispielwarnung. Keine Diagnose in sechs eigenen Quellen; r13-clippy.txt |

Fünf initial rote Archivfälle: idless_mismatched_parts_reach_preparation_and_actual_upload_after_read_failure; processed_recovery_guard_rejects_changed_targets_parts_and_mismatched_observations; whole_recovery_proof_survives_connection_error_and_prioritizes_actual_rejection; verworfene_uploads_fallen_nicht_unter_den_tisch; processed_recovery_parts_keep_history_through_preparation_and_upload_entry. Nicht als Flake oder Altbaseline ausgegeben.

Gezählt werden31 verschiedene fokussierte Fälle: Worker15, API15, korrigierter Einzeltest1. Keine vollständige erneut grüne Archiv-, API- oder Workspace-Suite; initiale66 und korrigierte15 nicht doppelt addieren. Keine neue Baseline gemessen, kein neuer separater strikter Archiv-Clippy behauptet. API-Kompilation meldete MemFdCreateFlag-Abkündigung in uplink_config.rs:847, keine Unterdrückung/fremde Bereinigung.

Die drei verlangten Folgen liefen tatsächlich: unzureichender1s+60s-Treffer mit ausdrücklichem Retry/Upload und ausreichender60+60-Gegenprobe; ursprünglicher vollständiger Ganzbeleg mit transientem Folgefehler; sicher zugeordnete aktuelle Teilablehnung mit tatsächlichem Einzelteilupload und Schutz des unverändert bestätigten Nachbarn, anschließende Wiederholung ohne Zusatzupload. Kein künstlicher historischer Uploadabschluss.

Hashbelege: r13-source-sha256.txt gehört ursprünglichem b38 vor Fixturekorrektur; abschließend [r13-final-source-sha256.txt](pruefung/r13-final-source-sha256.txt), identisch fixer-r13-source-sha256.log, sechs von Eltern erneut OK. Technische Cargo-Metadaten fixer-r13-metadata.log(177.546Bytes) bestätigten keinen Archiv→API-Zyklus, nicht als vollständiger Testlauf gezählt.

Gate13 einmal regulär über gate_hook.py --review mit gpt-6.1-sol auf eingefrorenem Stand, Exit2 vor Modellstart: input_too_large, 1.058.627/max1.048.576 Zeichen, kein Urteil. [gate-round13-tool.txt](pruefung/gate-round13-tool.txt), [gate-round13-base.txt](pruefung/gate-round13-base.txt). Keine unveränderte Wiederholung, Gate-/Modell-/Kriterienänderung. Frühere Empfehlung einer technischen Budgetänderung ist durch aktuelle Freigabe zur reinen Verdichtung der drei eigenen Notizen ersetzt.

TESTNACHWEIS[TW-1]: 31 passed, 0 ignored | Baseline: nicht neu gemessen, keine Altfehlerbehauptung

## Lesender Beleg des verbleibenden Größenblockers

Messung nach Verdichtung und vor diesem Befundnachtrag: unveränderte Funktionen run_git, build_review_source_context und build_review_prompt aus gate_hook.py ausschließlich lokal zur Größenberechnung ausgewertet. Kein Modell-/Provideraufruf, keine Zustands-, Konfigurations- oder Werkzeugänderung. Die Gate13-Promptgröße wurde exakt reproduziert. Lokales origin/main inzwischen635e750f6a24fa02d07d9e2ef9044322a0ee2fcf; tatsächlicher Merge-Base weiterhin4ddf37032050c2b8cbe74bb921d7ed1c055514b4, daher derselbe Vergleichsbereich.

| Eingabestand | Pfade | Vollständiger Diff, Zeichen | Kontext, Zeichen | Vollständiger Prompt, Zeichen |
| --- | --- | --- | --- | --- |
| Gate13 auf863ff0ff | 151 | 838020 | 205291 | 1058627 |
| Elterncheckpoint31e8c455 vor Verdichtung | 165 | 908114 | 207119 | 1131523 |
| verdichtete drei Notizen, noch uncommittet | 165 | 877004 | 207119 | 1100413 |

Aktueller Task-Diff wurde dadurch um31467Bytes/31110Zeichen kleiner; die ursprünglichen Notizdateien hatten zusammen68582Bytes. IDs, SHA- und Originalreferenzinventar geprüft, keine fehlende Kennung. Nur die drei freigegebenen Notizen geändert.

Restliche vollständige Eingabe liegt um51837Zeichen über1048576. Rein rechnerisch beträgt der unveränderte Eingabeanteil außerhalb dieser drei Notizen1062642Zeichen und übersteigt das Limit bereits um14066. Das ist eine Größenzerlegung, keine ausgeführte Filterung. Weitere Verdichtung ausschließlich dieser Notizen kann den vollständigen Gate auf diesem Stand deshalb nicht unter das Limit bringen.

Unveränderte Diffanteile:503887Zeichen übrige Taskeingaben,335346Zeichen übrige Repoänderungen; Quellenkontext207119Zeichen. Größte erhaltene Taskanteile: beide Original-Moli-Messdateien50954/50867Zeichen, REVIEW.md32556, r7-api-clippy.txt24836, r9-/r8-api-clippy.txt je17726, r13-clippy.txt17614, r13-archive-first.txt12387. Größte Quellanteile: youtube_check_tests.rs80959, API-Tests61404, worker.rs54251, youtube_check.rs54247. Keine dieser Quellen/Originalbelege geändert oder aus dem Gate entfernt.

Der Elterncheckpoint fügte nach dem gescheiterten Gate13 insgesamt17 Akten/Originalbelege hinzu (734Zeilen, eine entfernt); vollständiger Diff wuchs um70094Zeichen und Prompt um72896. Die reine Notizverdichtung ist belegt, der verbleibende Blocker kommt aus ausdrücklich zu erhaltenden anderen Eingabeanteilen. Daher kein blindes erneutes Senden, kein ALLOW/BLOCK und keine automatische neue Produkt-/Prüfschleife. Für den Abschluss fehlt ein freigegebener Weg, diese vollständige unveränderte Eingabe regulär prüfen zu lassen.

## Historische Tests und Befehle

Befehlsbasis W=`/home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008`. Rust: `/home/nathanael/.local/bin/cargo-slot test --jobs 3 --manifest-path W/rust/Cargo.toml -p <Paket> <Filter> -- --include-ignored --nocapture`. Synthetischer zusätzlicher Parameter, wo verlangt: `TB_TEST_DATABASE_URL=postgresql://nathanael@127.0.0.1:55683/token_db_youtube`; bei ursprünglicher API zusätzlich `VOD_ARCHIVE_PROOF_PATH=W/.tasks/2026-10-08-vod-youtube-abgleich/pruefung/api-fixture.json`. W bezeichnet hier nur den dokumentierten absoluten Pfad, keine Änderung des ausgeführten Befehls. Paket Archiv=tb-vod-archive ohne Filter; API=tb-dashboard-api vod_archive_management; Client=tb-social-media archive_read. Originalbefehle bleiben in den jeweiligen Belegen/Fixerübergaben.

Frontend: `npm run test:vod-archive --prefix /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008/bot/dashboard_v2`. Build: derselbe absolute --prefix mit `npm run build`. Tatsächliche Dashboardausgabe bot/analytics/dashboard_v2/dist, nicht Quellordner. Frühere Builds/Tests ersetzen keinen aktuellen Main-Release.

| Stand/Prüfung | Ergebnis und Scope | Originalbelege |
| --- | --- | --- |
| erste Läufe | Archiv55/API6/Client2/Frontend6, jeweils0 failed/ignored bzw.skipped; Archiv0/API1336/Client339 filtered; Exit0. Früherer falscher API-Filter social_media_vod_archive ohne Tests zählt nicht | archive-tests3.log, api-tests2.log, youtube-client-tests.log, frontend-tests.log, frontend-build.log |
| 76332e3d | Archiv57/API7 +unveränderte Client2/Frontend6 =72 verschiedene passed,0 ignored; keine Gesamtsuite. Archiv0/API1336 filtered; Exit0. Dashboard/Admin/Websitebuild Exit0 | archive-gatefix2.log, api-gatefix.log, frontend-build2.log, admin-build.log, website-build.log |
| 89bfd5fa/Gate4 | Archiv59/Frontend8,0 failed/ignored bzw.skipped; unveränderte API7/Client2, historische Summe76. Zusätzlich gestoppte API-/Clientnachläufe ohne Testabschluss nicht gezählt; Archiv-Clippy strikt Exit0 | damalige Fixerübergabe, pruefung/bestaetigung |
| Versuch2 | API9/0/0,1336 Bibliothek filtered, Exit0; vier zusätzliche Null-Ziele nicht gezählt. Archiv zuerst60/3/0,Exit101: Mindestbudget101IDs, Frische unvollständiger Inventare, fehlende Erfolgszeit | restart-api-tests.log, restart-archive-tests.log |
| Abschluss ad1360b9c5a31929f | Archiv63/0/0,Exit0, Archiv-Clippy strikt0; mit API9/Frontend8/Client2 historisch82. Auth-ID-Sperrkollision synthetischer101-ID-Proben und zwei falsche Frische-Erwartungen eng korrigiert; keine rote Altcodebaseline. Gezielt fmt/diff0 | fixer-restart-archive.log, fixer-restart-archive-clippy.log, restart-frontend-tests.log, restart-frontend-build.log |
| Versuch3 a416a113c573896ad | Frontend9/0/0skipped, Build0. Rust wegen belegter Slots nicht gestartet; historische82 nicht dem neuen Rust-Stand zugerechnet | fixer-r6-ui.log, fixer-r6-ui-build.log |
| Eltern bgiqvfbes | Slot3, tatsächliche Kompilation, Archiv63/1/0,0 filtered,Exit101. Neue Backoff-/beide101-ID-Proben bestanden; roter completed_inventory_rechecks_refresh_deletions_processing_and_manual_requests, youtube_check_tests.rs:391 | parent-r6-archive.log |
| 672e90af vor Integration240e6214 | Archiv64/API10,0 failed/ignored; Archiv0/API1336 Unit+25 Integration filtered,Exit0; Archiv-Clippy strikt0, sequenziell. Kontosperrenfreigabe ausdrücklich abgewartet. +Frontend9/Client2 =85 verschiedene historische Fälle | pruefung/r6b-archive.txt, r6b-api.txt, r6b-clippy.txt |
| ab1304491e0d292d5/Eltern bivjl16t0 | API13/0/0,1336 Unit+25 Integration filtered, Archiv66/0/0,0 filtered;Exit0. Beide echten Worker-SQL-Fälle, Vorbereitung/Upload/Wiederholung. +Frontend9/Client2 =90 historische Fälle; beide Clippy0, gezielt fmt/diff0, fünf Quellhashes | parent-r7-archive.log und damalige API-/Clippybelege, pruefung/gate-round8.txt |
| 61c57d33/91fccc69 | Archiv67/API14,0 failed/ignored,81 aktuelle Rustfälle. API-Null-Lauf nicht gezählt, echter Zwischenfehler durch ausdrücklich abgewarteten Rollback behoben; Archiv-Clippy strikt/API-Clippy0, sieben Hashes | pruefung/r8-*.txt, gate-round9.txt; ursprünglicher Gateoutput fixer-r8-gate.log |
| d928e8cc/1345a55f | Archiv68/API15,0 failed/ignored; beide Clippy/gezielte fmt/diff/hash0, sieben Hashes. Kein kompletter Workspacebeweis | pruefung/r9-*.txt, Gate10-/Basisbelege |
| c8f44356/7fb7c6f2 | ordinary_provider_rejection; partial_processed; verworfene_uploads_fallen_nicht_unter_den_tisch: je1/0/0,Exit0, echte gespeicherte Ablehnung/API→Worker; kein voller Nachlauf. Erster wartender Schutztest am echten Werkzeuglimit ohne Compiler/Teststart, leeres Log, zählt nicht; eigene alte Prozessfamilie beendet, genau ein erfolgreicher Ersatz | pruefung/r10-api-worker.txt, r10-api-resets.txt, r10-archive-rejection.txt, r10-pruefungen.txt, r10-source-sha256.txt |
| c3dbabc/Gate12 | keine Cargo-Tests wegen alter Queue, keine positive/negative Dauerprobe, kein neuer Clippyerfolg; gezielt rustfmt/diff laut Fixer0, fünf HashesOK | pruefung/r11-freeze.txt, r11-hashcheck.txt, r11-source-sha256.txt, gate-round12.txt |
| Datenvertragsdiagnose | keine Quellen/Tests/Gate; alte kombinierte Clippy-Queueb8zb1ayz9 beendet Exit2 vor Cargo: Wrapper break-Zeile66/fi-Zeile67. Kein geprüfter Quellstand, kein Test-/Clippyfehler; aktuelles bash -n unverändert0 | pruefung/r12-datenvertrag.txt, r10-clippy.txt |

Eigene slotlose Warteaufrufe b2x8gxmfc/blarcscnc wurden nach18:47/10:29 beendet, zusammen29:16 ohne Compiler, kein Testnachweis. Späterer einzelner Elternlauf ohne eigene30-Minuten-Abbruchgrenze erfolgreich. Historischer API-Clippy zunächst ebenfalls Wrapper-Syntaxfehler vor Start, danach unveränderter regulärer Nachlauf0:21 lib-/25 test-Warnungen mit21 Duplikaten, eine Beispielwarnung, keine in fünf eigenen Quellen. Ergebnisse nie fremdem/neuem Quellstand zugerechnet.

## Tatsächliche Baseline, Clippy und Formatabweichungen

Archivbaseline auf `0ecae1370f1a80d1a101249b5c932663d69be8af`:50 passed,0 failed,0 ignored,Exit0 in eigener token_db_youtube_baseline_c5d0. Erster Einrichtungsversuch49 passed/ein Konfigurationsfehler wegen zusätzlich nötiger token-db-tests.conf; korrigierter identischer Lauf ohne Skipmarker0. Keine rote Codebaseline. Gestoppte API-Baseline ohne Testabschluss nicht als0 Fehler gezählt. Historisches „Baseline:0 rot“ gilt nur gemessenem Archivumfang; kein neuer Baselinelauf für r13.

Strikter historischer Clippy: cargo-slot clippy --jobs3, eigenes rust/Cargo.toml, Pakete tb-config/tb-social-media/tb-vod-archive, --all-targets -- -D warnings. Aktuell und eigener detached Baselinestand jeweilsExit101, dieselbe tb-raid/src/signup_denylist.rs:71 result_unit_err. Mit --no-deps jeweilsExit101, dieselben vier eindeutigen tb-social-media-Stellen: analytics.rs:321 too_many_arguments; credentials.rs:214 manual_map; upload_worker.rs:822 type_complexity; vocab.rs:164 needless_borrows_for_generic_args. Duplikate nicht als weitere Fehler. Ausgewählter Umfang --all-targets --no-deps ohne-D warningsExit0, dieselben vier Warnungen, keine neue in geänderten Zielpfaden. Keine Unterdrückung/fremde Bereinigung; eigener Baselineworktree danach entfernt.

Historischer gezielter FormatcheckExit1, vier bestehende store.rs-Assertion-Hunks. pruefung/fmt-vergleich.py verglich denselben Formatter mit vollständigem Ausgangs-SHA oben: beide vier Hunks/36 geänderte Zeilen, identische Änderungen; pruefung/fmt-vergleich.json, VergleichExit0. Das ist belegte unveränderte Abweichung, keine grüne Formatprüfung. Erster rustfmt-Standardeingabeversuch nicht aussagekräftig, zählt nicht.

## Aussage synthetischer Datenproben und Erwartungskorrekturen

Eigene PostgreSQL16 auf Loopback55683, token_db_youtube mit synthetischen Daten. Fehlende Konfiguration führt ausdrücklich zum Fehler, nicht zu stillen Skips. Fortsetzungsprobe nutzt synthetische verschlüsselte Credentials über CredentialManager und echte Rust-Leseendpunkte an lokalem HTTP-Server: sechs GETs in zwei Läufen mit je drei logischen Leseoperationen, Cursor setzt zweite Playlistseite fort. Keine endgültige Fehlzuordnung/Bestätigung vor Suchabschluss; gleiche Titel/verschiedene Twitch-Quellen getrennt; dritter nicht gefundener Fall erst nach vollständiger Suche ungeklärt. Original-VODs unverändert, keine künstlichen Uploadteile.

Echte SQL-Proben sichern alten positiven Nachweis bei Fehlversuch, verwerfen überholten Schreiber bei Kontowechsel/parallelem Upload, binden Cleanup an vollständige aktuelle verarbeitete Ziel-/Uploadnachweise. Die alte automatische Produktionsprüfung fehlender/abgelehnter Videos wurde durch unabhängigen Lesepfad ersetzt; Worker-Erwartung prüft keinen erneuten Upload bei verschwundener Antwort und erhält lokale Kopie. Bestehender Bereinigungstest speichert echten SQL-Nachweis. Metadatenprobe erhält Originalquelle/Teilnummer trotz langer Titel/Beschreibungen, ändert keine Bestandsvideos.

## Abgeschlossene Moli-Sichtprüfung

Moli1.1.14, eigener Loopback9338, synthetischer lesender Fixture-Server4198. Keine Produktionsanmeldung/Provider-Schreibaktionen; echte synthetische PostgreSQL-Handlerantworten und hashgebundene gebaute Assets. Erster falscher Ausgabeordner scheiterte vor Sichtprüfung, kein visueller Nachweis.

Erste erfolgreiche gebündelte Runde auf76332e3d: current/partial/connection jeweils1440×1100 und390×844, neun Karten, Dokumentbreite=Viewport, Statusicons/Zeiten sichtbar, Prüfbutton entprellt, Verbindungsweg erhalten. Manrope/Sora geladen, keine Bilder/pageerrors. Sechs Detailbilder betrachtet,18 Viewportbilder und sieben Assethashes in pruefung/moli-layout.json. dirty=true betraf uncommittete Taskartefakte/Testkonfiguration, nicht späteren UI-Code.

Einzige erlaubte Bestätigung nach Build89bfd5fa: dieselben sechs Kombinationen, sieben neue Assethashes, neun Karten, keine Überbreite/pageerrors, sechs Detailbilder betrachtet; pruefung/bestaetigung/moli-layout.json und Bilder. Ausgangsbilder erhalten. Eigene Moli-/Fixture-Prozesse beendet. Keine weitere UI-/Browserrunde oder Politur; fremder TikTok-Gesamtbundlediff kein eigener Archivoberflächendiff.

## Provider- und Produktionsgrenzen

Einzige echte normale YouTube-Vorprobe2026-10-08T02:34:28.993209799Z, eigener verbundener Kanal, vorhandener CredentialManager/Infisical-Pfad: channels.list, erste Upload-Playlistseite, videos.list erfolgreich;50IDs/50Videos, weitere Seiten vorhanden,Exit0. Alle fünf Fälle gehören zu diesem geprüften Zugang. Nur Aggregate: pruefung/provider-vor-release.log und provider-vor-release.txt. Leserecht vorhanden; keine vollständige Altfallsentscheidung, neue Vorprobe oder VOD-Schreiboperation.

Lesende Rolle twitchlegacy hat tatsächlich kein SELECT auf _sqlx_migrations (has_table_privilege geprüft). Keine Rechteausweitung/privilegierte Umgehung. Datenbeleg READ ONLY/repeatable read; Migration über vorhandene Unit, Result/Journal und identischen Release-Migrationshash. Erfolgs-Joins in live-evidence.sql konto-/revisions-/kanal-/quell-/snapshotgebunden, aktueller Versuch getrennt authgebunden, Fehler nur bool; keine private ID/Rohproviderfehlerausgabe. Quelle und Dauer in beiden Joins geprüft. Drei SELECTs bislang nur synthetisch ausgeführt, keine neue Produktion/Altfallsprobe.

Produktiver Ausgangsbeleg2026-10-08T00:02:26.79818+00:00:82VODs,76 historische Uploadbestätigungen, fünf unklare Fälle, ein Twitch-VOD nicht mehr verfügbar. Fälle10(24750s)/11(19510s) ohne Teile;2941(13830s)/2995(14663s)/2996(2830s) je pending-Teil ohne Video-ID. Alle fünf archived, keine Uploadzeit, accepted_parts=0. Noch keiner mit neuem produktiven Worker abgeschlossen.

## Eigene Ressourcenereignisse

Vor Versuch2 ein Archivlauf vor Tests Exit101 wegen Speicherplatz, kein Ergebnis. Danach233GiB frei, keine zusätzliche Cachebereinigung. Eigene PostgreSQL zunächst ohne ursprüngliche Portoption an belegtem Standardport gescheitert, mit55683 wieder gestartet. Einmal sichtbar gewordener eigener Harness-Zugang nicht in Task/Git übernommen; Widerruf/Neuzugang im Register, keine dauerhaften Bot-Secrets gelesen.

Früheres eigenes Debug-Cleanup erst nach Ende eigener Prüfprozesse und lesender Prozess-/Releaseprüfung: cargo-slot clean explizites Profil dev/eigenes Baseline-Manifest/eigener Hauptworktree als target-dir. Trockenlauf und tatsächlicher identischer Lauf je8778Dateien/11,2GiB,Exit0. Keine Releasebinaries/fremden Caches/Modelle/Container entfernt; eigener sauberer Baselineworktree danach gelöscht. Keine Wiederholung beauftragt.

## Offen

Reine Notizverdichtung und gleicher vollständiger Gate ohne Quellen-/Kriterienausblendung, dann tatsächliches ALLOW; autorisierter Main-/Release-/Migration-/Deploy-/Neustart-/Liveabschluss und eigenes Cleanup. Originalbelege bleiben vollständig. Kein produktiver Nachweis mit neuem Worker geschrieben, keine Produktionsmigration angewendet, fünf historische Fälle offen. Eigene Testdatenbank bleibt bis Belegsicherung.
