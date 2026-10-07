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

Der Sichtnachweis für NIT 4 wurde anschließend in einer gebündelten Runde erbracht: bestätigt, teilweise und Verbindungsfehler jeweils auf Desktop und Mobil. Sechs betrachtete Detailbilder und `pruefung/moli-layout.json` sind gesichert. Keine weitere UI-Korrekturrunde.
