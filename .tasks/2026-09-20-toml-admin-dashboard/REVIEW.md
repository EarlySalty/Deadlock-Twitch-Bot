# Review R1: Paket A Twitch TOML Admin

status: aktiv (2026-09-20)

Urteil: FREIGABE. Keine fixbedürftigen Mängel im geprüften Diff.

WIRKUNGSPRUEFUNG[WP-1]: 0 Befunde | Zwillingssuche: grep-belegt | Fremddienst-Pfade: 0/0 geprüft

## Diff und Scope

Worktree `/home/nathanael/.worktrees/twitch-toml-admin-live-20260920`, Branch
`feat/twitch-toml-admin-live-20260920`, HEAD `97e5353a` gegen `origin/main`
`93def59e`. Genau zwei Dateien, zwei Einfügungen, kein weiterer Commit.

- `ops/systemd/deadlock-twitch-bot-rust.service:45` `ReadOnlyPaths=/var/lib/deadlock-twitch/config`
- `ops/systemd/deadlock-twitch-dashboard-rust.service:43` `ReadWritePaths=/var/lib/deadlock-twitch/config`

Live steht bereits `origin/main` `93def59e` plus Drop-in am Dashboard. Speichern
über die Admin-API ist unabhängig belegt (Quelle `104d32c0`, Live-Datei
`1e6fa3b3`). Dieser Review prüft den Unit-Diff plus den Live-Stand lesend.
Nicht gemergt, nicht deployed, keine Unter-Agenten.

## Mängel (blockierend)

Keine.

Die Dashboard-Zeile ist der in systemd 255 dokumentierte Weg, unter
`ProtectSystem=strict` einen Schreibpfad freizugeben. Sie trifft das Verzeichnis,
nicht die Datei: `tb-config/src/editor.rs:92` nimmt das Elternverzeichnis,
`:103` legt `.bot.toml.lock` an, `:152` die Tempdatei, `:184` das Rename. Ein
ReadWritePaths auf die Datei `bot.toml` würde das Rename verbieten und die alte
Inode festnageln. Das Live-Drop-in
`/etc/systemd/system/deadlock-twitch-dashboard-rust.service.d/operating-config.conf:3`
trägt denselben Wert; `systemctl show` meldet `ReadWritePaths=/var/lib/deadlock-twitch/config`,
Mountinfo des Dashboard-PID 234938 zeigt denselben Pfad `rw`. Hostrechte:
Verzeichnis `drwxrws---` `root:twitchmedia`, Datei `0660` `twitchdash:twitchmedia`,
`twitchdash` in `twitchmedia`, `test -w` auf Datei und Verzeichnis erfolgreich.

Die Bot-Zeile ist für das Lesen nicht notwendig (siehe H2), aber nicht falsch.
Live liest der Bot die Datei ohne ReadOnlyPaths: PID 234825, Kommandozeile
`--config /var/lib/deadlock-twitch/config/bot.toml`, Wurzelmount `ro`, kein
eigener Bind auf das Config-Verzeichnis, `NRestarts=0`, `twitchbot` kann die
Datei lesen. `ProtectSystem=strict` lässt Lesen zu; ReadOnlyPaths bindet denselben
Pfad zusätzlich nur lesbar. Verzeichnis statt Datei vermeidet eine festgenagelte
Inode nach Rename.

## Hinweise (nicht blockierend)

- H1: `ops/systemd/install-twitch-release.sh:162` archiviert `ops/systemd` in den
  Release-Baum. Das Skript schreibt nirgends nach `/etc/systemd/system` (keine
  Fundstelle). `deploy-twitch-release` startet Dienste neu, kopiert keine Units.
  Nach Merge dieses Commits bleiben die installierten Units die Fassungen vom
  31.08.2026, bis jemand die Dateien nach `/etc/systemd/system` kopiert und
  `daemon-reload` ausführt. Die Live-Wirkung fürs Dashboard bleibt das Drop-in.
  Das ist der bestehende Installationsweg, kein neuer Fehler dieses Diffs, und
  kein Grund, die Unit-Quellen nicht zu aktualisieren.

- H2: `ops/systemd/deadlock-twitch-bot-rust.service:45` `ReadOnlyPaths` ist unter
  `ProtectSystem=strict` fürs Lesen überflüssig. Live belegt: Bot ohne diese
  Zeile, Wurzel `ro`, Datei lesbar, Prozess aktiv. Die Zeile dokumentiert den
  Pfad und ändert das Schreibverbot nicht. Nicht entfernen müssen, nicht
  nachziehen müssen.

- H3: `ops/stt-server/deadlock-stt-server.service:8` startet
  `tb-stt-server --config /var/lib/deadlock-twitch/config/bot.toml` mit
  `ProtectSystem=strict`. User-Unit ist `active`, System-Unit `inactive`. Kein
  ReadOnlyPaths, nicht Teil von Paket A. Der Bot-Unit-Diff ändert diesen Dienst
  nicht.

- H4: `bot/admin_dashboard/src/pages/config/OperatingConfig.tsx:81` und `:25`:
  Speichern startet keinen Dienst. Fingerabdruck bleibt bis zum Neustart auf
  Neustart ausstehend. Das ist Paket C laut `PAKETE.md`, nicht dieser Diff.

## Zwillinge (geprüft, Urteil)

| Stelle | Urteil |
|---|---|
| `ops/systemd/deadlock-twitch-dashboard-rust.service:43` ReadWritePaths Verzeichnis | nötig und richtig; Live-Drop-in trägt denselben Pfad |
| `ops/systemd/deadlock-twitch-bot-rust.service:45` ReadOnlyPaths Verzeichnis | überflüssig fürs Lesen, unschädlich, Datei wäre falsch |
| Live-Drop-in `deadlock-twitch-dashboard-rust.service.d/operating-config.conf:3` | laufende Wahrheit fürs Dashboard; nach Unit-Kopie redundant, nicht widersprüchlich |
| `rust/crates/tb-config/src/editor.rs:92,:103,:152,:184` Lock, Temp, Rename im selben Verzeichnis | verlangt Schreibrecht auf das Verzeichnis; Zwilling zur Dashboard-Zeile |
| `rust/scripts/run_tb_bot_service.sh:7,:10,:159` und `run_tb_dashboard_service.sh:30,:31,:126` | beide Dienste `--config /var/lib/deadlock-twitch/config/bot.toml`; ohne Datei kein Start |
| `ops/stt-server/deadlock-stt-server.service:8` | gleicher Pfad, anderer Dienst, nicht in diesem Paket |
| `rust/scripts/run_stream_audit_service.sh` | kein `bot.toml`, nur Infisical/rclone |
| `ops/systemd/tb-category-collector.service:15` | anderer Config-Pfad `/etc/deadlock-twitch/category-collector.json` |
| `ops/systemd/deadlock-twitch-stream-coaching-watch.service` | kein `bot.toml` |
| ReadWritePaths auf die Datei statt das Verzeichnis | nicht gebaut; wäre für Rename und Inode falsch |

Kein zweiter Schreibpfad auf dieselbe Betriebsdatei außer dem bestehenden
Admin-Editor. Kein neuer HTTP-Weg, kein neuer systemctl-Endpunkt, kein
Fremddienst in diesem Diff.

## Livebelege, ausschließlich lesend

Release `/opt/deadlock/twitch/releases/93def59e7deb633ff57470fd5599cd73f90ca81e`.
exe Bot und Dashboard ohne `(deleted)`. Journal `-p err` der beiden Units der
letzten zwei Stunden leer.

| Prüfung | Ergebnis |
|---|---|
| Bot PID | 234825, `User=twitchbot`, `SupplementaryGroups=twitchmedia twitchstt` |
| Dashboard PID | 234938, `User=twitchdash`, `SupplementaryGroups=twitchmedia` |
| Bot `ReadOnlyPaths` live | leer; Wurzelmount `ro` |
| Dashboard `ReadWritePaths` live | `/var/lib/deadlock-twitch/config` (Drop-in) |
| `bot.toml` | reguläre Datei, kein Symlink, kein `.git` darüber |
| SHA Live-Datei | `1e6fa3b345bdd8ea89cf46dc2eeec42bef2caee37ae42ce32c05950061adedd1` |
| SHA `~/.config/deadlock-twitch/bot.toml` | `104d32c0d0656e8e9ec1cf0825ebcd4a399b37eba7433293af7680fa26917a5f` |
| Lock | `/var/lib/deadlock-twitch/config/.bot.toml.lock` existiert, `0600` `twitchdash:twitchmedia` |

Die abweichenden SHAs belegen: die Live-Datei wurde aus der belegten Quelle
installiert und danach über die Admin-API geändert. Die Home-Kopie ist nicht
der Laufzeitpfad (`ProtectHome=yes`).

Branch `origin/feat/twitch-toml-admin-live-20260920` zeigt denselben SHA
`97e5353a`.

## Auftrag Paket A gegen diesen Diff

1. Betriebsdatei am vom Startskript erwarteten Ort: erfüllt (Live-Datei, kein
   Symlink, kein Checkout).
2. origin/main mit Operating-Seite live: erfüllt (Release `93def59e`).
3. Dashboard darf unter `ProtectSystem=strict` schreiben: live über Drop-in,
   dauerhaft in der Unit-Quelle `deadlock-twitch-dashboard-rust.service:43`.
4. Bot startet mit `--config` dieser Datei: erfüllt.
5. Übernahme des Fingerabdrucks ohne SSH nach Speichern: nicht dieser Diff,
   Paket C.

## Konsequenz

Runde 2 gegen diese Liste: nichts zu fixen. Merge dieses Commits ändert die
laufenden Units nicht von allein (H1). Das Drop-in nicht löschen, bevor die
Unit-Dateien unter `/etc/systemd/system` dieser Fassung entsprechen.
