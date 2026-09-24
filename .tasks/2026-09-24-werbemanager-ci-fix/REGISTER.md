status: aktiv
Datum: 2026-09-24

# Register: Werbemanager-CI-Fix

Intent-Thread: dieser T3-Thread (Grok/GLM-5.3-flash). Abweichung von der
Pyramide: Laut Vorregister (2026-09-24-admanager-steam-api/REGISTER.md) sind
die T3-Rollen heute kontingentgesperrt (GLM/Grok bis 12:48, Astra bis 09:53,
Claude-OAuth abgelaufen); der Fix läuft daher in dieser Session, Stufe mittel.

| Rolle | Thread/Modell | Worktree/Branch | Status |
|---|---|---|---|
| Intent+Orchestrator (diese Session) | t3-code Thread | `.worktrees/tb-admanager-steam-api-20260924` (fix/admanager-steam-api-20260924) | aktiv |
| Implementierung | dieselbe Session (Abweichung s. o.) | dito | Umsetzung läuft |
| Unabhängiges Review | offen — erst nach CI-Grün ansetzen | — | offen |

## Bezüge

- Vorauftrag/Cluster: `.tasks/2026-09-24-admanager-steam-api/` (branch)
- PRs: EarlySalty/Deadlock-Twitch-Bot#958, EarlySalty/Deadlock-Steam-Bot#69
  (beide im T3-Thread verlinkt)
- Blocker außerhalb des Codes: GitHub-Actions-Billing (Steam #69, Bots #451),
  unabhängiges Review (PR-first-Testbetrieb)
