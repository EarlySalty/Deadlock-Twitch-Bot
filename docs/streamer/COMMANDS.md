# Commands

Stand Twitch-Chat: `2026-09-09`

## Grundsaetze

- Fast alle Twitch-Chat-Commands setzen voraus, dass der Kanal als Partner-Streamer registriert ist.
- Discord-Hybrid-Commands sind fuer Streamer-Self-Service gedacht und antworten in der Regel `ephemeral`.
- Der Name `traid` ist doppelt belegt:
  - Twitch-Chat `!traid` ist Alias fuer den manuellen Raid.
  - Discord `/traid` ist der OAuth-Link-Command.

## Twitch Chat Commands

| Command | Alias | Wer darf | Zweck | Wichtige Bedingungen |
| --- | --- | --- | --- | --- |
| `!raid_status` | `!raidbot_status` | jeder im Kanal | Status, Autorisierung und Raid-Stats anzeigen | Kanal muss als Partner registriert sein |
| `!raid_history` | `!raidbot_history` | jeder im Kanal | letzte 3 Raids anzeigen | rein lesend |
| `!raid` | `!traid` | Broadcaster oder Mod | sofortigen manuellen Raid starten | Stream muss live und raid-faehig sein |
| `!uban` | `!unban` | Broadcaster oder Mod | letzten Auto-Ban rueckgaengig machen | nutzt letzten gespeicherten Auto-Ban |
| `!clip` | `!createclip` | jeder im Kanal | Clip aus dem aktuellen Streambuffer erstellen | braucht gueltige Auth bzw. Fallback-Token |
| `!ping` | `!health`, `!status`, `!bot` | jeder im Kanal | Liveness-/Statusantwort | allgemeiner Bot-Check |
| `!silentban` | keine | Broadcaster oder Mod | Chat-Hinweis fuer Auto-Bans toggeln | Bans laufen weiter, nur die Chat-Nachricht wird abgeschaltet |
| `!silentraid` | keine | Broadcaster oder Mod | Chat-Hinweis fuer Raids toggeln | Raids laufen weiter, nur die Chat-Nachricht wird abgeschaltet |
| `!lurkersteuer_off` | `!lurkersteuer_aus`, `!lurker_tax_off` | nur Broadcaster | Lurker Steuer dauerhaft deaktivieren | nur in Paid-Plaenen verfuegbar |

## Wichtige Details zu einzelnen Chat-Commands

### `!raid`

Der manuelle Raid liefert je nach Zustand unterschiedliche Rueckmeldungen:

- `started`: Raid gestartet
- `source_not_live`: der Stream ist nicht live
- `source_not_eligible`: aktuell nur fuer Deadlock oder kurz nach Wechsel von Deadlock auf Just Chatting
- `no_target`: kein passender Raid-Kandidat gefunden
- `unavailable`: Bot oder Session nicht verfuegbar

### `!clip`

- Erstellt einen Clip aus dem laufenden Stream.
- Nutzt bevorzugt das Broadcaster-Token.
- Faellt notfalls auf ein Bot-Token zurueck.
- Wenn die Twitch-Verbindung fehlt, bittet der Bot darum, sie erneut herzustellen.

### `!lurkersteuer_off`

- Deaktiviert die Lurker Steuer nur fuer Paid-Plaene.
- Reaktivierung erfolgt aktuell nicht per Chat, sondern ueber `/twitch/abbo`.

## Discord Hybrid Commands

| Command | Alias | Zweck | Wichtige Bedingungen |
| --- | --- | --- | --- |
| `/check-scopes` | `check_scopes`, `checkscopes` | Alias fuer `/check-auth` | Streamer muss ueber Discord-ID zugeordnet sein |
| `/check-auth` | `check_auth`, `checkauth` | prueft, ob alle `RAID_SCOPES` vorhanden sind | zeigt fehlende Scopes und Re-Auth-Bedarf |
| `/traid` | `twitch_raid_auth` | erzeugt frischen Twitch-OAuth-Link | Streamer-Self-Service fuer Re-Auth |
| `/raid_enable` | `raidbot` | Auto-Raid aktivieren | sendet bei fehlender Auth direkt Requirements + OAuth-Button |
| `/raid_disable` | `raidbot_off` | Auto-Raid deaktivieren | rein fuer den eigenen Kanal |
| `/raid_status` | `raidbot_status` | Status, Tokenablauf und Raid-Stats anzeigen | Antwort als Embed |
| `/raid_history` | `raidbot_history` | letzte Raids anzeigen | `limit` wird auf `1..20` begrenzt |

## Nicht als Streamer-Self-Service gedacht

- `/sendchatpromo` ist admin-only.
- `/reauth_all` ist admin-only.
- `tte` ist owner-only.
- `twl` ist ein Discord-Community-Command fuer Stats-Channels, kein normaler Streamer-Self-Service.
