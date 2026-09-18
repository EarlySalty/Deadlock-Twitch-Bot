# API-Vertrag Werbemanager (Paket A liefert, Paket B konsumiert)

Basis bleibt `/twitch/api/v2/streamer/ad-manager`. Feldnamen camelCase wie bisher. Identität kommt nur aus der Session.

## Geändert: Settings (GET-Antwort und POST-Rumpf)

- `strategy`: nur noch `'snooze' | 'smart'`. `'monitor'` wird mit 400 abgelehnt.
- neu `budgetMinutesPerHour`: Ganzzahl 1 bis 8, Default 3.
- alle übrigen Felder unverändert.

## Geändert: Status

- neu `plan`:
  - `nextBlockAt: string | null` (frühester Termin des nächsten eigenen Blocks)
  - `blockSeconds: number` (30 oder 60)
  - `blocksPerHour: number`
  - `budgetUsedSecondsThisHour: number`
  - `source: 'twitch' | 'own'` (Nachtrag 1: Twitch-Plan aktiv oder eigenes Budget)
  - `fit: 'good' | 'tight' | 'unprotectable'` (Nachtrag 2/3: internes Schützbarkeits-Signal, nur für den Status-Satz, nie als Empfehlung; `plan.suggestion` gibt es nicht)
- neu `currentReason: string | null` (Grundcode der letzten Entscheidung, siehe unten)

## Neu: `GET /twitch/api/v2/streamer/ad-manager/history`

Liefert die laufende Session, sonst die letzte.

```json
{
  "sessionStartedAt": "2026-09-18T18:00:00Z",
  "summary": {
    "blocksRun": 5,
    "blocksInWindow": 5,
    "budgetSecondsUsed": 150,
    "budgetSecondsPlanned": 180,
    "postponed": 3
  },
  "entries": [
    { "at": "2026-09-18T19:14:00Z", "decision": "commercial", "reason": "in_queue", "blockSeconds": 30, "detail": null },
    { "at": "2026-09-18T19:40:00Z", "decision": "postpone", "reason": "recent_raid", "blockSeconds": null, "detail": "1337cammy" }
  ]
}
```

`decision`: `commercial | snooze | postpone | none`.

`reason` (geschlossene Liste, B übersetzt in Nutzersprache, unbekannte Codes zeigt B neutral als "verschoben"):
`in_queue`, `match_start_window`, `post_match_quiet`, `quiet_chat`, `fallback_least_bad`, `in_match`, `post_match_wait`, `post_match_chat_active`, `recent_raid`, `recent_first_chatter`, `startup_protection`, `cooldown`, `budget_reached`, `no_snoozes`, `twitch_ad_moved`, `disabled`, `offline`.

Ergänzt durch A (Nachträge 1 und 2):
`pulled_forward` (geplante Twitch-Werbung aus einem offenen Fenster vorgezogen),
`twitch_plan_active` (Twitch-Plan aktiv, Bot wartet oder spart Pausen),
`plan_fit_changed` (interner Zustandswechsel der Schützbarkeit, `detail` trägt `good`/`tight`/`unprotectable`; B zeigt diesen Eintrag neutral).

`decision` bei diesen Einträgen wie gehabt: `pulled_forward` ist `commercial`, `twitch_plan_active` ist `none` oder `snooze`, `plan_fit_changed` ist `none`.
