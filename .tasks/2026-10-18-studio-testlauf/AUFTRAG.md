# Studio: clip toggle fix + pipeline test run

Worktree ~/.worktrees/tb-studio-testrun, branch feat/studio-clip-toggle-testrun. Left sidebar navigation is untouchable.

## A. Toggle "Unterschiedliche Clips je Plattform posten" (Planen, Schritt 1)
- Dimmed subtext under the toggle, from draft state:
  - OFF: „Aktiv: Derselbe Clip wird auf allen aktiven Plattformen veröffentlicht (empfohlen).“
  - ON: „Aktiv: Jede Plattform zieht sich einen eigenen, anderen Clip aus dem Vorrat (verbraucht mehr Clips).“
- "Nächster Termin" and the outside-lead hint react immediately to the draft toggle (before saving).
  OFF = next slot where all active platforms are free; ON = each platform its own next free slot.
- Backend hold (OutsideLead) and planner must use per-platform earliest slot in independent mode.

## B. "⚡ Pipeline jetzt testen (1 Clip vorbereiten)"
- Heute header next to "Läuft gerade", plus banner/button in Planen.
- Next pool clip in configured order, full preparation now, ignoring lead time and slots, using current Schritt 2 settings.
- Never auto-publishes; lands in Prüfung & Freigabe with "Testlauf" badge; live stages in Läuft gerade.
- Review: play, "Änderung wünschen", AI costs, then "Für nächsten Termin einplanen" or "Test verwerfen (Clip zurück in den Vorrat)".

Acceptance: build/fmt/clippy/tests green, screenshots 390/1440 via Moli, gate ALLOW, merged, deployed, live check.
