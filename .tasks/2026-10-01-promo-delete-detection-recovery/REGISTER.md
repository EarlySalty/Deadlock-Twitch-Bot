status: aktiv | Datum: 2026-10-01

# Register: Promo-Delete-Detection Recovery

- Repo: Deadlock-Twitch-Bot
- Source branch/SHA: `promo-delete-detection-20260915` / `364ad1c8d92c6701163b69d7608675ad08e50270`
- Arbeitsbranch: `codex/luna-dispatch/deadlock-twitch-bot/promo-delete-detection-20260915-364ad1c8`
- Worktree: `/home/nathanael/.worktrees/luna-dispatch-deadlock-twitch-bot-promo-delete-detection-20260915-364ad1c8`
- Stufe: mittel
- Host-Resource-Hold: durch jüngste Orchestrator-Anweisung aufgehoben; keine schwere Suite parallel
- TokenDB-Produktionshold: pauschaler Hold aufgehoben; Gruppen- und Ownerverantwortung für Integration und Live-Schritte bleibt bestehen
- Gate R1: BLOCK, `gpt-6.1-sol`; fünf Fixes umgesetzt
- Gate R2: BLOCK gegen unveränderten Commit `364ad1c8`; kein Urteil über den WIP
- Gate R3: BLOCK gegen Commit `a634e834`; Alert-Retry und Message-ID-Locks nachgebessert
- Gate R4: ALLOW gegen Commit `c2b2116150442c3340066ea6283cf5c427a20868`
- Gruppenintegrator: Thread `e60a2e14-1b57-4700-bb31-bc0492f4487e`; gemeinsame Integration noch offen
- Unabhängige Intent-Abnahme: angefragt in Thread `49ae48bc-5668-41e8-89f5-50967381c813`

## Thread-Register (T3)

| Paket | Thread-ID | Modell | Status | Worktree | Letzte Meldung |
|---|---|---|---|---|---|
| Intent / ursprünglicher Auftrag | `a7e16de3-e682-42da-a3d7-6c8c7d434f5e` | gpt-6-luna | laufend | obiger Worktree | WIP auf Remote gesichert; Gate R4 ALLOW; unabhängige Intent-Abnahme läuft |
| Fixer R2 | kein Thread erstellt, nicht erforderlich | keine Rolle | durch direkte Orchestrator-Freigabe ersetzt | obiger Worktree | Promo-Autor-Luna behob die fünf R1-Befunde selbst; kein Kontingent geändert |
| Gruppen-Intent-Koordination | `68348009-64c8-477b-9ef7-01140f5c7cb7` | laut T3-Thread | gestoppt, nicht wieder aufnehmen | kein Worktree erforderlich | alter Koordinationsthread gestoppt |
| Unabhängige Intent-Abnahme | `49ae48bc-5668-41e8-89f5-50967381c813` | claude-opus-5-5 | wartet auf Ergebnis | kein Worktree erforderlich | prüft Commit `c2b2116150442c3340066ea6283cf5c427a20868` |
| Gruppenintegrator PR #1035 | `e60a2e14-1b57-4700-bb31-bc0492f4487e` | laut T3-Thread | bereit | anderer Worktree, keine Änderungen durch diesen Auftrag | gemeinsame Integration noch offen |
