# Ops-Runbooks

## Einordnung

Die bisherigen Ops-Dokumente mischen zwei Welten:

- ein historisches Split-Runtime-Runbook mit Windows-/Proxy-Bezug
- ein neueres Runbook fuer die Rotation des Twitch-Analytics-DSN

Fuer dieses Repo sollte die operative Sicht kuenftig hier gebuendelt werden. Die Altdateien unter `ops/` bleiben vorerst als Quelle erhalten und koennen spaeter aufgeraeumt werden.

## Split Runtime: Soll-Zustand

Der Betriebsplan trennt den Bot-Laufzeitteil vom Dashboard-Laufzeitteil. Fachlich heisst das:

- Bot-Service: Bot-Runtime, interne API, EventSub-Callback-Verarbeitung
- Dashboard-Service: OAuth-, Billing-, Admin- und Streamer-Web-Surfaces

Wichtig ist die klare Netztrennung:

- interne API nur fuer lokalen Service-zu-Service-Verkehr
- EventSub-Callback nicht ueber dieselbe Surface wie normale Dashboard-Seiten routen
- oeffentliche `/twitch/*`-Seiten am Dashboard, Webhook-Callback am Bot-Service

Im aktuellen Linux-Betrieb werden Services als systemd-User-Units neu gestartet, nicht ueber die alten Windows-Helfer.

## Standardbetrieb

Vor Aenderungen an Dashboard, OAuth, Billing oder EventSub immer mitpruefen:

- antwortet der Dashboard-Service auf Health-/Ready-Pfade
- antwortet die interne API lokal mit Token
- ist der EventSub-Callback-Endpunkt getrennt und erreichbar
- sind Legal-/Billing-/OAuth-Pfade im Webrouting nicht versehentlich blockiert

Bei Fehlerbildern zuerst die Trennung pruefen: viele "Dashboard kaputt"-Symptome sind in Wahrheit Bot-Runtime-, Internal-API- oder Callback-Probleme.

## Betriebsschalter in der Config-Datei

Betriebswerte des Bots stehen in einer normalen Config-Datei, nicht in Umgebungsvariablen. Nur Secrets kommen aus Infisical.

**Pfad:** `~/.config/deadlock-twitch-bot/bot.json` (Home des Nutzers, unter dem der Bot-Service laeuft)

Die Datei ist JSON, in Abschnitte gegliedert. Unbekannte Abschnitte werden ignoriert, ein Abschnitt darf also fehlen. Fehlt die Datei, ist sie unlesbar oder kaputt, gilt ueberall der Vorgabewert.

| Abschnitt | Feld | Typ | Vorgabe | Wirkung |
|---|---|---|---|---|
| `obs_docks` | `enabled` | bool | `false` | Schaltet den Schreibpfad des OBS-Dock-Event-Busses ein. Bei `true` schreibt der Bot jedes dock-taugliche Twitch-Ereignis (Chat, Raid, Abo/Geschenk, Go-Live/Offline) in die Tabelle `obs_dock_events`, meldet die Zeile per `pg_notify('obs_dock', ...)` und laesst einen Aufraeum-Loop mitlaufen. Bei `false` passiert nichts davon; der Bot verhaelt sich wie ohne das Modul. Gelesen wird der Wert einmal beim Start, eine Aenderung wirkt erst nach `systemctl --user restart`. |

Beispiel:

```json
{
  "obs_docks": { "enabled": true }
}
```

Wer den Bus einschaltet, braucht die Migration `20260824090000_obs_dock_events.sql` angewendet (Migrator-Lauf, nicht beim Service-Start) und ein Gateway, das auf `LISTEN obs_dock` haengt. Code: `rust/bin/tb-bot/src/obs_dock.rs`.

## DSN-Rotation

Die DSN-Rotation betrifft nur das Twitch-Analytics-Postgres-Passwort. Nicht Teil dieser Rotation sind andere Secret-Arten wie allgemeine App-Schluessel oder Datenverschluesselung.

Die sichere Reihenfolge ist:

1. neues DB-Passwort erzeugen
2. Passwort in Postgres aendern
3. neuen DSN direkt verifizieren
4. Secret Store aktualisieren
5. betroffene Services neu starten
6. Health-Checks laufen lassen
7. bei Fehlern Rollback fuer DB und Secret Store ausfuehren

Wichtige Betriebseigenschaften:

- die Rotation schreibt ein secret-freies Audit-Log
- die Bot- und Dashboard-Runtime werden nach erfolgreicher Rotation neu gestartet
- Health-Checks fuer DB, interne API und Dashboard sind Teil des sicheren Abschlusses

## Health-Check-Minimum nach Releases

Nach Aenderungen an Routing, Auth, Billing oder Split-Runtime sollte mindestens geprueft werden:

- Dashboard-Entrypoint
- Legal-Gate
- interne API mit Auth-Header
- EventSub-Callback-Pfad
- Billing-Webhooks nur intern, nie versehentlich oeffentlich ohne Schutz

## Rollback-Grundsaetze

- Routing zuerst auf den letzten funktionierenden Zustand zuruecksetzen
- danach Bot- und Dashboard-Service neu starten
- bei DSN-Rotation zusaetzlich den vorherigen DB-/Secret-Stand wiederherstellen
- keine Teil-Rollbacks akzeptieren, wenn Dashboard und Bot gegeneinander auf unterschiedlichen Vertragsstaenden laufen

## Auffaellige Altlasten aus den Quell-Runbooks

- Das Split-Runtime-Runbook referenziert noch Windows/NSSM/Caddy-Details und ist nicht 1:1 der aktuelle Linux-Betrieb.
- Das DSN-Runbook ist inhaltlich nuetzlich, enthaelt aber standortbezogene Pfade, die in die neue Internal-Doku nicht uebernommen werden sollten.
- Eine spaetere Bereinigung von `ops/PG_DSN_ROTATION.md` und `ops/runbook-split-runtime.md` ist sinnvoll, wurde hier aber bewusst nicht geloescht.
