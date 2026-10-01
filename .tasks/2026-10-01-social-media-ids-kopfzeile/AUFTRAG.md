# Auftrag: Social Media wieder freischalten, durchgehend über Twitch-IDs, Kopfzeile wie die Shell

Intent-Thread: 37ae96e4-49e9-46b7-a77f-e148784352c1
Stufe: mittel. Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.

## Befund (geprüft am 2026-10-01)

Seit Commit `f00ed825` (30.09., "bind clip access to IDs") prüft
`my_access_handler` und `require_sm_access` in
`rust/crates/tb-dashboard-api/src/handlers/social_media.rs` die Freigabe über
`tb_social_media::partner_access::is_partner_id_granted` mit
`WHERE twitch_user_id = $1`. Die Bestandszeilen in
`social_media_partner_access` haben aber keine ID:

```
twitch_user_id | streamer_login | granted
NULL           | dach_lock      | t
NULL           | earlysalty     | t
```

Folge: kein Partner hat seit dem 30.09. Zugriff auf `/social-media-admin`
(Hinweis "Noch nicht freigeschaltet"), nur der Admin-Modus kommt durch.

## Ziel 1: alles sauber über IDs

Der Umstieg auf IDs ist richtig, er wurde nur nicht zu Ende gebracht. Nutzerregel:
Identitäten immer über die Twitch-User-ID auflösen, nie über Login oder
Anzeigenamen. Login nur noch zur Anzeige.

- Einmaliger Backfill als neue Migration: `twitch_user_id` der Bestandszeilen
  aus der Streamer-Tabelle (z. B. `twitch_streamers`) per Login setzen. Zeilen,
  die sich nicht auflösen lassen, im Report nennen statt still zu verwerfen.
- Danach `twitch_user_id` als Pflichtfeld absichern (NOT NULL, eindeutig), damit
  nie wieder eine Freigabe ohne ID entsteht. Prüfen, dass `set_partner_access`
  und der Admin-Freischalt-Pfad im Frontend (`SocialMediaAdmin.tsx`,
  `accessMutation`) immer die ID schreiben.
- Den restlichen Social-Media-Pfad (Handler, `tb-social-media`, Frontend-API in
  `bot/dashboard_v2/src/api/socialMedia.ts`) nach verbliebenen Login-basierten
  Zugriffs- oder Besitzprüfungen absuchen (Graphify zuerst) und auf IDs
  umstellen. Kein Login-Fallback als Notnagel.
- Prod-Migration: der Deploy-Wrapper migriert nicht. Migration als `postgres`
  einspielen, Rechte an `twitchbot`/`twitchdash`, Version plus sha384-Checksumme
  in `_sqlx_migrations` (siehe Repo-Doku und Memory
  `twitch-connect-rank-live-und-migrator-luecke`).

## Ziel 2: Kopfzeile wie die gemeinsame Shell

Der Block in `SocialMediaAdmin.tsx` (um Zeile 155 bis 200: `studio-brand` mit
`deadlock-d-logo.png`, "Deutsche Deadlock Community", "Arbeitsbereich / Dein
Kanal" und der Partner-Chip oben rechts) kam mit `83756fb5` und gibt es nur auf
dieser Seite. Regel: alle Dashboards teilen eine Shell, Kopfzeilen gleich hoch,
kein Partner-Chip oben rechts. Block entfernen und dieselbe Kopfzeile wie auf
`/analyse` nutzen (Admin-Kanalwahl und Freigabe-Schalter für Admins bleiben
funktional erhalten, nur sauber in die Shell eingepasst). Zugehöriges CSS in
`components/socialmedia/studio.css` mit aufräumen, wenn es verwaist.

## Regeln

- Keine Code-Kommentare schreiben, bestehende in berührten Stellen nicht erweitern.
- Eigener Worktree unter `~/.worktrees/tb-sm-ids-kopfzeile` von `origin/main`,
  Branch `fix/social-media-ids-kopfzeile`. Nie im geteilten Checkout arbeiten.
- `cargo fmt`, `cargo clippy`, `cargo test` der berührten Crates (mit
  `set -o pipefail`), Frontend `npm run build` und bestehende Tests.
- `.sqlx`-Dateien und Schema-Snapshot mitziehen, falls Queries sich ändern.
- Vor der Abgabe Selbstprüfung mit `gate_hook.py --review`.
- Nach ALLOW: Merge nach main (`git push origin HEAD:main`, ein Git-Schritt pro
  Aufruf), Prod-Migration, Deploy über `deploy-twitch-release <sha>`, Neustart
  Dashboard, Live-Beweis, Branch und Worktree löschen, Register nachziehen,
  `t3-thread.py settle --selbst`.

## Live-Beweis

- `social_media_partner_access`: beide Zeilen tragen die echte Twitch-ID.
- `GET /social-media/api/access/me` liefert für eine Partner-Session ohne
  Admin-Modus `allowed: true` (z. B. earlysalty mit Admin-Modus aus).
- Screenshot oder Beschreibung: `/social-media-admin` zeigt dieselbe Kopfzeile
  wie `/analyse`, kein Logo-Block, kein Partner-Chip.

## Bericht

Branch, Commits, Merge-SHA, Migration, Live-Beweis, offene Punkte in den
Intent-Thread.
