[Orchestrator] Frischer Fixer für Auftrag E (TikTok Direct Post), Gate-Runde 1 BLOCK.

Briefing mit Befunden und Freigabepunkten: `/home/nathanael/.worktrees/tb-social-tiktok-direct/.tasks/2026-10-07-social-tiktok-direct/BRIEFING-FIXER.md` (daneben REVIEW.md, EVIDENCE.md, REGISTER.md). Worktree `/home/nathanael/.worktrees/tb-social-tiktok-direct`, Branch `feat/social-tiktok-direct-post`, Stand f637d138. Rolle: Blatt-Worker, keine weiteren Threads. Haupt-Orchestrator: d3a1741e-82bc-4a48-865b-2845c663dca7.

Zusatz vom Orchestrator:
1. Ist der Branch noch nicht auf origin, zuerst sichern (normaler Push, kein Force).
2. Testvideo: Clip-Dateien liegen in der Dienstansicht unter dem Bind-Mount `/var/lib/deadlock-twitch-media/clips` (eingebunden als `current/data/clips`); der Go-Live-Bericht hat 109 von 109 Quelldateien über `/proc/<PID>/root` der Dienste gefunden, Vorschaupfade werden über `resolve_preview_path` auf `data/clips/<id>_preview.mp4` umgelegt. Vor "Datei fehlt" dort prüfen. Freigabe bleibt: genau ein SELF_ONLY-Post auf earlysalty nach ALLOW, Merge und Deploy, eigener earlysalty-Clip ohne fremde Musik, Clip-ID im Bericht.
3. Parallel laufen A3 (Token-Ablauf) und D2 (Upload-Wartezustand, berührt den Upload-Worker). Vor dem Gate auf aktuelles origin/main rebasen, Konflikte im Upload-Worker sorgfältig auflösen.
4. Folgerunde mit gpt-6.1-sol, Selbstprüfung per gate_hook.py --review vorher. Nach ALLOW Abschluss wie im ursprünglichen Auftrag E.
