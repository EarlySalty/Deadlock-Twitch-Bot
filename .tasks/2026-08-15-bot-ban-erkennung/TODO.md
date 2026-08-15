# Offen: Bot-Ban-Erkennung und Deadlock-Pause

Stand 2026-08-15 06:15. Branch `feature/engagement-reaktions-lernmodus`,
Cherry-Picks liegen lokal im Worktree `/home/nathanael/.worktrees/tb-vod-auto-save`
(Live-Branch `feature/vod-auto-save`, **nicht gepusht**).

## 1. BLOCKER: Ban-Klassifikation ist widerlegt

`c246502a` und `96d48afd` **nicht deployen**, bis das geklärt ist.

Die aktive Prüfung stuft einen Kanal als gebannt ein, wenn Twitch auf
`POST /moderation/moderators` mit `400 {"message":"user is banned"}` antwortet.
Das ist kein Beweis. Gegenbeleg aus dem Live-Log vom 2026-08-15 06:01:11:

```
tb_chat::global_ban_sweep: GlobalBanSweep: Bot kein Moderator (403) — Ban übersprungen
broadcaster="miracleghost9" chatter=benjamen63 urteil="übersprungen" grund="moderator_forbidden"
```

Der Bot liest dort den Chat mit und sieht die Chatter namentlich. Er scheitert
ausschließlich an fehlenden Mod-Rechten. Ein gebannter Bot wäre nicht im Chat.

**Zu tun:** Die Erkennung an ein Signal hängen, das der Bot selbst verifizieren
kann. Kandidaten, in dieser Reihenfolge zu prüfen:

- Ein echter `sender_banned`-Drop beim Chat-Senden. Das ist der einzige heute
  vorhandene harte Beweis und läuft bereits über `tb-chat::timeout_tracking`.
- Chat-Lesbarkeit als Gegenprobe: sieht der Bot im Kanal Chatter, ist er nicht
  gebannt. `global_ban_sweep` hat diese Information bereits.
- `GET /moderation/banned` scheidet aus: dafür fehlt der Scope `moderation:read`
  (siehe `tb-analytics/src/system_oauth_scopes.rs`).

Bis dahin bleibt der deployte Stand `99bc323e` richtig: die aktive Prüfung
meldet nur ins Admin-Log und pausiert niemanden.

**Aufräumen danach:** `whysolowkey` und `pixelpiratemarvin` stehen auf
`technical_pause_reason='bot_banned'`. Die Marker stammen laut Blacklist-Grund
aus dem `eventsub`-Pfad, nicht aus der Probe, gehören aber überprüft.
`miracleghost9` ist bereits entpausiert.

Nicht per Hand in der DB geradebiegen. Der Dienst muss den Zustand selbst
zurücknehmen können, sonst ist die Ursache nicht behoben.

## 2. Deadlock-Pause: erste Welle ist raus

Alle 15 Kandidaten wurden am 2026-08-15 zwischen 05:28 und 05:37 pausiert, jeder
mit einer DM. `46haris` und `talakos86` sind korrekt ausgenommen.

Bei rund 9 der 15 war der Bot schon vorher kein Moderator (erkennbar an
dauerhaftem EventSub-403 `moderator_forbidden`). Deren DM behauptet, der Bot habe
gerade seine Rechte abgegeben. `96d48afd` behebt das für künftige Fälle
(`UnmodOutcome::WasNotModerator` löst keine DM mehr aus). Entscheidung vom
2026-08-15: die bereits gesendeten neun DMs werden **nicht** nachkorrigiert, der
Zustand und die Handlungsanweisung darin stimmen.

**Zu tun:** nichts, außer `96d48afd` mitzudeployen, sobald Punkt 1 geklärt ist.

## 3. MCP-Connector für tb-bot

Branch `feature/tb-mcp-connector`, gebaut und getestet, weder registriert noch
deployt. Port 8891, loopback-only. Sechs Werkzeuge, davon zwei schreibende mit
`confirm`-Pflicht (`disconnect_bot`, `run_deadlock_pause_sweep`).

Registrierung in `.mcp.json`:

```json
{ "mcpServers": { "tb-mcp": { "type": "http", "url": "http://127.0.0.1:8891/mcp" } } }
```

Greift erst in einer neuen Session.

**Zu tun danach:** `46haris` und `talakos86` über `disconnect_bot` trennen. Beide
haben in der erfassten Historie nie einen Deadlock-Stream; der Nutzer will sie
aus dem Partner-Bestand haben. Sie können jederzeit neu Partner werden.

## 4. Branches

- `feature/engagement-reaktions-lernmodus`: alles gepusht, 60 Commits vor `main`.
  Merge nach `main` tippt der Nutzer selbst.
- `feature/vod-auto-save`: enthält lokal 12 Cherry-Picks, noch nicht gepusht.
  Die Parallel-Session pusht nach erfolgreichem Deploy.

## Regeln für die Weiterarbeit

- Kein Deploy und kein Dienst-Neustart ohne Abstimmung mit der Parallel-Session.
  Genau ein Deployer pro Ressource, eine Ansage ist keine Abstimmung.
- Keine Streamer-DMs und keine schreibenden Prod-DB-Zugriffe ohne wachen Menschen.
- Artefakte nie nach Datei-Alter beurteilen, immer gegen den Commit-Stand.
- Vor jeder Codebase-Frage Graphify statt grep.
