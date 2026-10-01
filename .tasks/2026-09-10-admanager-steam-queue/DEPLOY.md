# Deploy: Werbemanager Queue-Phase (c6453dca)

status: bereit
datum: 2026-09-11

Der Release ist gebaut, eingefroren und vollständig vorbereitet. Die root-Schritte
(Installer, Service-Restart) müssen wegen der Produktiv-Schutzschranke vom Nutzer
ausgeführt werden. Befehle kopieren, in dieser Reihenfolge:

## 1. Installer ausführen (aus dem Repo-Root, einzeln, wörtlich)

```
SHA="$(git rev-parse c6453dca)"
sudo /usr/local/sbin/install-twitch-release "/opt/deadlock/twitch/builds/$SHA" "$SHA"
```

Der Installer prüft SHA, Eigentümer, Artefakte, schreibt SHA256SUMS, kopiert nach
`/opt/deadlock/twitch/releases/<sha>` und wechselt `current` atomar.
Erwartete Ausgabe: `Twitch-Release aktiviert: c6453dca...`

## 2. Dienste neu starten (einzeln)

```
sudo systemctl restart deadlock-twitch-dashboard-rust.service
sudo systemctl restart deadlock-twitch-bot-rust.service
sudo systemctl restart deadlock-twitch-stream-coaching-watch.service
```

## 3. Rückmeldung

Nach Installer und den drei Restarts kurz sagen, ob der Installer die erwartete Zeile
gebracht hat; dann übernimmt die Session die Live-Prüfung (PID-Vergleich,
`/proc/<pid>/exe`, Journal `-p err`, Anker im laufenden Binary, Browser-Check
der Werbemanager-Karte Desktop + mobil) und das Aufräumen.

## Freeze-Details (bereits erledigt)

- Eingefrorener Checkout: `/opt/deadlock/twitch/builds/$SHA`, wobei `$SHA` die vollständige
  SHA von `c6453dca` ist (eigenständiger Clone, HEAD c6453dca, root:root, go-w,
  keine Sonderdateien)
- Binaries: rust/target/release/{tb-bot,tb-dashboard,tb-stream-audit}, gebaut als
  twitchbuild, Cargo 1.97.1, CARGO_TARGET_DIR=/var/lib/twitchbuild/tb-admgr-target-20260911
- Anker belegt: „Match-Status“ im dashboard_v2-Bundle (index-CIzJFLYc.js),
  „Steam-Match-Status nicht lesbar“ im tb-dashboard-Binary
- Frontend-Dists: dashboard_v2, admin_dashboard, website (alle Exit 0)
- Vorher-Werte für den Live-Beweis: tb-dashboard PID 3958717 (Port 8769),
  tb-bot PID 57819 (Ports 8776/8786/8892); Bundle ohne Steam-Strings;
  Ad-Manager-Endpoint 401 (unauth) mit Fehlertext ohne steam-Block
