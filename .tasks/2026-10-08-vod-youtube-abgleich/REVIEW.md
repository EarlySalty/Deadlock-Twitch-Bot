# Merge-Gate: Runde 1

HEAD: `84e3f58d69adc418a4d2245546b6fa506101b644`. Basis: `origin/main`.

Befehl: `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008 --base origin/main --head HEAD`.

Exit 1, Kritiker `gpt-6.1-sol`, BLOCK.

## Offene Funde

1. BLOCKING, `rust/crates/tb-vod-archive/src/youtube_check.rs:540`: Vorhandene Video-IDs mit anderen lokalen Teilzuständen als `done` werden abgefragt, ihre Ergebnisse anschließend aber verworfen. Wenn jeder Teil eine ID hat, unterbleibt zusätzlich die Playlist-Suche. Die Abgleichszuordnung muss vom historischen lokalen Abschlusszustand unabhängig sein.
2. BLOCKING, `rust/crates/tb-vod-archive/src/youtube_check.rs:483`: Bekannte ID-Batches können das gültige Mindestbudget von drei Leseoperationen aufbrauchen, sodass gemischte historische Fälle nie eine Playlist-Seite erreichen. Ein Fortsetzungsweg muss bei diesem Budget tatsächlich vorankommen.
3. NIT, `rust/crates/tb-dashboard-api/src/handlers/social_media_vod_archive.rs:89`: Ein YouTube-Nachweis ersetzt derzeit unabhängig vom anderen Ziel den Hauptstatus und unterdrückt ursprüngliche Fehlererklärungen. Drive-Erfolg und erforderliche Upload-Erklärungen sollen sichtbar bleiben.
4. NIT, `bot/dashboard_v2/src/components/socialmedia/VodArchiveTab.tsx:77`: Sichtnachweise für bestätigt, teilweise und Verbindungsfehler fehlen noch. Die gebündelte Desktop-/Mobilprüfung steht aus.

## Fixrunde und Runde 2

Frischer nativer Fixer `ab6d2a6f3ec6b3702`, Commit `76332e3d26e80e48e013e92ddddb429acf068652`. Vorhandene IDs liefern Nachweise unabhängig vom lokalen Teilzustand. Historische Playlist-Suche macht mit Mindestbudget drei Fortschritt, auch bei 52 bekannten IDs. Zwei neue echte Datenbankproben sichern diese Fälle. Sie ändern keine Uploadteile oder historischen Abschlüsse. Der API-Pfad erhält unabhängigen Drive-Erfolg und erforderliche Upload-Erklärungen.

Befehl: `python3 /home/nathanael/Documents/.claude/gpt-workers/gate_hook.py --review --repo /home/nathanael/.worktrees/tb-vod-youtube-abgleich-20261008 --base origin/main --head HEAD --model gpt-6.1-sol`.

Exit 0. Urteil: `ALLOW: Both previous blockers are fixed; no blocking regression shown.` Beide BLOCK-Funde wurden als FIXED ausgewiesen. Derselbe Kritiker wie Runde 1, kein Modellwechsel und keine Übersteuerung.

## Runde 3 auf gesichertem Stand fb89fe69

Befehl wie Runde 2 mit demselben Kritiker. Exit 1, BLOCK: `Historical rechecks reuse stale video evidence and report a fresh check time.`

1. BLOCKING, youtube_check.rs:303: Abgeschlossene Zuordnungsinventare werden bis zur nächsten Playlist-Suche wiederverwendet, aber save erneuert die Erfolgszeit. Historische Fälle ohne IDs und fehlende IDs gemischter Fälle brauchen frische Videoabfragen vor einer neuen Erfolgsmeldung, auch bei manuellem Prüfen und laufender Verarbeitung.
2. NIT, pruefung/live-evidence.sql:10: Produktiver Nachweis muss wie die API Authrevision und Upload-Snapshot prüfen, sonst zählen veraltete Nachweise als aktuell.
3. NIT, VodArchiveTab.tsx:74: Der Wiederverbindungslink fehlt bei Drive-Anforderung trotz sichtbarem YouTube-Verbindungsfehler.
4. NIT, VodArchiveTab.tsx:51: Historische Teilzählung braucht eine eindeutige Beschriftung, damit sie nicht einer aktuellen vollständigen YouTube-Bestätigung widerspricht.

Erneut frischer nativer Fixkontext für den BLOCK-Fund. Elternsession bearbeitet ausschließlich Nachweis-SQL und Register.

## Runde 4

Frischer Fixer a5ec69ec1742e7a0b, Commit 89bfd5fa7da8b06b5fe8967051c577b940236260. Vorhandene und über Inventar zugeordnete IDs werden vor neuer Bestätigung frisch gesammelt abgefragt. Bei übersprungenen Kandidaten bleibt die bisherige Prüfzeit erhalten. Zwei neue PostgreSQL-Proben decken manuelles Prüfen, verschwundene Videos, Verarbeitungswechsel und das Abfragebudget ab. Die beiden UI-Hinweise wurden eng begrenzt korrigiert und mit zwei zusätzlichen bestehenden Frontendproben gesichert.

Gleicher Gate-Befehl und Kritiker gpt-6.1-sol, Exit 0:
`ALLOW: Inventory matches are freshly queried before success is recorded; no blocking regression shown.`
`FIXED: rust/crates/tb-vod-archive/src/youtube_check.rs:303`.

59 Archivtests und acht Frontendtests bestanden. Die erlaubte gebündelte Bestätigungsrunde nach der Korrektur ist abgeschlossen: sechs Desktop-/Mobilkombinationen, keine Überbreite, geladene Schriften, keine pageerrors. Sechs Detailbilder tatsächlich betrachtet. Belege in pruefung/bestaetigung. Keine weitere UI-Polierschleife.

## Runde 5 auf 6dd214a2

Derselbe Kritiker, Exit 1, BLOCK: `Reconciliation can stall indefinitely, report stale absence as fresh, and strand rejected uploads.`

1. BLOCKING, youtube_check.rs:620: Eine Zuordnung mit 101 IDs macht bei gültigem Mindestbudget drei keinen Fortschritt, weil dieselben ersten 100 IDs erneut gelesen werden. Gilt für lokale IDs und wiedergefundene Inventarkandidaten. Dauerhafte Fortsetzung erforderlich, auch für schließlich uneindeutige Zuordnungen.
2. BLOCKING, youtube_check.rs:553: Ein altes abgeschlossenes Inventar verhindert bei manuellen Prüfungen neue Playlist-Leseabfragen. Leere oder unvollständige historische Zuordnungen dürfen keine neue Erfolgszeit erhalten, wenn ihre Suche nicht erneuert wurde. Suche erneuern oder tatsächliche Suchzeit beibehalten.
3. BLOCKING, social_media_vod_archive.rs:95 und :299: Aktuelle abgelehnte oder nicht abrufbare Ziele bleiben wegen terminaler historischer Zustände ohne ausdrücklich ausgelöste Wiederholung oder bestehenden Drive-Weg. Manuelle bestehende Wiederherstellung erhalten, ohne dass Abgleich oder Prüfen einen Upload auslösen. Keine automatische Wiederholung, keine Drive-Erweiterung.
4. NIT, youtube_check.rs:334: Bei Fehlern und verändertem Teil-Snapshot werden alte Beobachtungen mit neuem Snapshot verbunden. Nachweis und ursprünglichen Snapshot zusammenhalten oder den nicht mehr passenden Nachweis nicht als aktuell ausgeben.

Neuer frischer nativer Fixkontext. Kein Main-Push und kein Deploy vor erneuter Freigabe. Die UI-Sichtgrenze ist ausgeschöpft; bestehende UI nicht umgestalten, weitere Korrekturen auf Rust, Persistenz und vorhandene Aktionen begrenzen.

## Wiederanlauf und Abschluss der erhaltenen Runde-5-Fixes

Versuch 2 übernimmt die erhaltene dauerhafte Fortsetzung mit Auth-, Kanal-, VOD-, Teil- und Kandidatenbindung. Einzelbeobachtungszeiten bleiben erhalten; unvollständige oder leere Suchnachweise werden nicht künstlich frisch datiert. Fehler halten ursprünglichen Nachweis und Snapshot zusammen. Bestehende geschützte Aktionen erlauben ausdrücklich angeforderte Wiederherstellung mit verfügbarer lokaler Quelle; nicht abrufbare akzeptierte Videos werden nicht automatisch erneut hochgeladen. Keine UI-Änderung.

Frischer nativer Kontext ad1360b9c5a31929f erledigte die tatsächlich offenen Testfälle, keine neue Hierarchie. Produktivfix unverändert erhalten, Auth-ID-Sperrkollision zwischen synthetischen Großfallproben und veraltete Suchzeit-Erwartungen korrigiert. 63 Archivtests und neun API-Tests bestanden, striktes Archiv-Clippy Exit 0. Reguläre nächste Gateentscheidung steht noch aus.

Der Sichtnachweis für NIT 4 aus Runde 1 wurde anschließend in einer gebündelten Runde erbracht: bestätigt, teilweise und Verbindungsfehler jeweils auf Desktop und Mobil. Sechs betrachtete Detailbilder und `pruefung/moli-layout.json` sind gesichert. Keine weitere UI-Korrekturrunde.

## Runde 6 auf f6870299

Nach regulärer Integration von origin/main, gleicher Kritiker gpt-6.1-sol, Exit 1. Urteil: `BLOCK: Legacy recovery fails, error backoff is bypassed, and recovery locks can deadlock.` Vollständiger bereinigter Beleg: restart-gate-round6.log.

1. BLOCKING, social_media_vod_archive.rs:392: Die Aktionsprüfung verweigert bei leerer gespeicherter Kanal-ID einen gültigen Nachweis. Liste und bestehende ausdrückliche Wiederherstellung müssen denselben sicheren Kontobindungsvertrag verwenden.
2. BLOCKING, youtube_check.rs:212: Ein geänderter Teil-Snapshot macht Fehler trotz zukünftiger next_check_at bei jedem Poll erneut fällig. Ursprüngliche Evidenz erhalten und Fehler-/Quotenwartezeit gesondert an den tatsächlichen Prüfversuch binden.
3. BLOCKING, social_media_vod_archive.rs:386: Aktionspfad sperrt Teile vor Konto, beide Abgleichsschreiber Konto vor Teilen. Gemeinsame Sperrreihenfolge erforderlich.
4. NIT, social_media_vod_archive.rs:97: Gemeinsam aktivierte Wiederholungs- und Drive-Aktion können eine nicht verfügbare zweite Aktion anbieten.
5. NIT, social_media_vod_archive.rs:307: Drei Wiederherstellungserklärungen fehlen im englischen Wörterbuch.

Versuch 3 erhält sämtliche abgeschlossenen Fixes und Nachweise. Frischer nativer Fixer a416a113c573896ad bearbeitet gemeinsam die drei BLOCK-Funde und nötiges enges Funktionswiring. Kein weiterer Browserlauf, keine neue Vorprobe, kein Merge oder Deploy vor ALLOW. Die bisherigen inhaltlichen BLOCK-Runden sind 1, 3, 5 und 6; ALLOW-Runden 2 und 4 zählen nicht als gescheiterte Fixrunden.
