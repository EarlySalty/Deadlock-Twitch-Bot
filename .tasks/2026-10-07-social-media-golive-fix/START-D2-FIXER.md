[Orchestrator] Frischer Fixer für Auftrag D, Gate-Runde 1 BLOCK.

Briefing mit Befunden und Freigabepunkten: `/home/nathanael/.worktrees/tb-social-golive-upload/.tasks/2026-10-07-social-golive-upload/BRIEFING-FIXER.md` (daneben REVIEW.md, EVIDENCE.md, REGISTER.md). Worktree `/home/nathanael/.worktrees/tb-social-golive-upload`, Branch `fix/social-golive-upload-texte`, Stand ac08dba4. Rolle: Blatt-Worker, keine weiteren Threads. Haupt-Orchestrator: d3a1741e-82bc-4a48-865b-2845c663dca7.

Zusatz vom Orchestrator:
1. Der Branch ist laut Übergabe noch nicht auf origin. Als ersten Schritt den unveränderten Stand auf origin sichern (normaler Push des Feature-Branches, kein Force).
2. Die beiden Nits (englische Wartegründe, Sichtnachweise) mit erledigen: Wartegründe im Dashboard auf Deutsch mit echten Umlauten.
3. Auftrag E (TikTok Direct Post) und A (Token-Ablauf) laufen parallel; vor dem Gate auf aktuelles origin/main rebasen, deren Bereiche nicht anfassen.
4. Folgerunde mit gpt-6.1-sol, Selbstprüfung per gate_hook.py --review vorher. Nach ALLOW Abschluss wie im ursprünglichen Auftrag D.
