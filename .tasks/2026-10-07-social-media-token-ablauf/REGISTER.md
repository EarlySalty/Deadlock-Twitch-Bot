# Register

Intent-Thread (Haupt-Orchestrator): d3a1741e-82bc-4a48-865b-2845c663dca7

## Session-Register

| Paket | Thread-ID | Ersteller | Startnachweis | Harness | Modell | Status | Worktree | Branch | HEAD | Letzte Meldung |
|---|---|---|---|---|---|---|---|---|---|---|
| A Token-Ablauf + Reauth-DM | e046e6b3-cddb-488b-9dd3-7760ce6c1665 | d3a1741e | "Turn angenommen" 2026-10-07 | t3-harness (Claude Code) | sol | Gate BLOCK Runde 1, übergeben; settle offen (HTTP 500, Session läuft noch), nicht wieder aufnehmen | ~/.worktrees/tb-social-token-ablauf | fix/social-token-ablauf | a782702c | Fixer-Briefing BRIEFING-FIXER.md |
| B Go-Live-Prüfung | fe22ccaf-1d70-488d-a881-3fd4848cc25a | d3a1741e | "Turn angenommen" 2026-10-07 | t3-harness (Claude Code) | sol | fertig, gesettelt | entfernt (sauber) | docs/social-golive-check (bleibt auf origin als Übergabe, nicht nach main) | 923b0024 | Bericht gelesen, nicht wieder aufnehmen |
| C Anreicherung | ad7b7b9f-d688-44dd-9fe4-ec229aa16fb0 | d3a1741e | "Turn angenommen" 2026-10-07 | t3-harness (Claude Code) | sol | gestoppt, gesettelt: Nutzer nimmt Anreicherung aus dem Go-Live (eigenes Thema) | nie angelegt | kein Branch | - | nicht wieder aufnehmen, Auftrag ../2026-10-07-social-media-golive-fix/AUFTRAG-C-ANREICHERUNG.md |
| D Upload-Wartezustand + Texte | 688111f5-fbd8-47bf-ab2a-64acf19f1ab0 | d3a1741e | "Turn angenommen" 2026-10-07 | t3-harness (Claude Code) | sol | läuft | ~/.worktrees/tb-social-golive-upload | fix/social-golive-upload-texte | - | gestartet, Auftrag ../2026-10-07-social-media-golive-fix/AUFTRAG-D-UPLOAD-TEXTE.md |
| E TikTok Direct Post | beb1f091-3232-4e8f-9703-e8c7f7b65b9f | d3a1741e | "Turn angenommen" 2026-10-07 | t3-harness (Claude Code) | sol | läuft | ~/.worktrees/tb-social-tiktok-direct | feat/social-tiktok-direct-post | - | gestartet, Auftrag ../2026-10-07-social-media-golive-fix/AUFTRAG-E-TIKTOK-DIRECT-POST.md |
| F Anreicherung abschalten | 3eae2692-6e81-4c9d-bc0e-dfbde4f46d1b | d3a1741e | "Turn angenommen" 2026-10-07 | t3-harness (Claude Code) | sol | fertig, live (main e0b0dbaf, Impl 8c58ea2c), selbst gesettelt | entfernt | gelöscht | e0b0dbaf | gestartet, Auftrag ../2026-10-07-social-media-golive-fix/AUFTRAG-F-ANREICHERUNG-AUS.md |

Nachtrag 2026-10-07: D per send umgestellt (Trennen = Token löschen, TikTok-Texte und Postfach-Erklärung an E).
Nachtrag 2026-10-07: F per send umgestellt: Editor bleibt als manueller Editor mit gespeicherten Hashtags, nur Automatik raus; Stufe mittel.
Nachtrag 2026-10-07: A meldete Gate-Abbruch (bwrap ENOMEM). Ursache Container-Mount-Limit fs.ve-mount-nr 4096 (3216 belegt); 11 alte Test-DB-Container gestoppt, bwrap wieder ok, A per send zum Abschluss fortgesetzt. Selbstheilung als eigener Auftrag in claude-config: Thread 48f2c6a1-6bb3-47ae-bd9a-cea67be13a61 (.tasks/2026-10-07-test-container-waechter).
Nachtrag 2026-10-07: E-Frage beantwortet: ein SELF_ONLY-Testpost auf earlysalty nach Deploy freigegeben, eigener earlysalty-Clip ohne fremde Musik.
| A2 Fixer Runde 1 | e5d43083-6a35-4292-865f-33619e2a5de8 | d3a1741e | "Turn angenommen" 2026-10-07 | t3-harness (Claude Code) | sol | läuft | ~/.worktrees/tb-social-token-ablauf | fix/social-token-ablauf | a782702c | Fix reauth.rs:67 (kein Vorfall ohne Refresh-Möglichkeit), Gate-Folgerunde gpt-6.1-sol |
| A3 Fixer Runde 2 | b78ae080-1331-4b94-813e-483ccab1893c | d3a1741e | "Turn angenommen" 2026-10-07 | t3-harness (Claude Code) | sol | läuft | ~/.worktrees/tb-social-token-ablauf | fix/social-token-ablauf (Sicherung -r2) | 21631a2d | Fix credentials.rs: SQL-Fehler als unbekannter Zustand |
Nachtrag 2026-10-07: A gesettelt; A2 (e5d43083) Gate BLOCK Runde 2, settle HTTP 500, nicht wieder aufnehmen.
| D2 Fixer Runde 1 | cfa688d1-ed78-4dce-a4bf-f2d4fbed7d34 | d3a1741e | "Turn angenommen" 2026-10-07 | t3-harness (Claude Code) | sol | läuft | ~/.worktrees/tb-social-golive-upload | fix/social-golive-upload-texte | ac08dba4 | waiting_connection verdrängt Uploads + 2 Nits; Folgerunde gpt-6.1-sol |
Nachtrag 2026-10-07: D (688111f5) Gate BLOCK Runde 1, übergeben an D2, nicht wieder aufnehmen.
| E2 Fixer Runde 1 | 65d85993-94d4-46c9-95c7-d8f07f853200 | d3a1741e | "Turn angenommen" 2026-10-07 | t3-harness (Claude Code) | sol | läuft | ~/.worktrees/tb-social-tiktok-direct | feat/social-tiktok-direct-post | f637d138 | 3 Gate-Befunde (Zeitplan-Ablehnung, JSON null, alte Vorgangskennung); Folgerunde gpt-6.1-sol |
Nachtrag 2026-10-07: E (beb1f091) Gate BLOCK Runde 1, übergeben an E2, nicht wieder aufnehmen. Creator-Info earlysalty: PUBLIC_TO_EVERYONE erlaubt.
| I Bestandsaufnahme | 18612f8f-1399-409f-85de-460c1747d727 | d3a1741e | "Turn angenommen" 2026-10-07 | t3-harness (Claude Code) | sol | läuft | ~/.worktrees/tb-social-inventar | docs/social-media-inventar | - | Auftrag ../2026-10-07-social-media-inventar/AUFTRAG.md |
Nachtrag 2026-10-07: F und E gesettelt. Neue Regel (claude-config 60137bf): Worker fahren Gate-Fixschleife selbst, an A3/D2/E2 geschickt.
| Concierge Coaching/Fallback (Deadlock-Bots) | 858c44f3-2f2f-475e-96cd-4cce2fa173d2 | d3a1741e | 2026-10-07 | t3-harness | sol | läuft | eigene | fix/concierge-coaching-fallback | - | Deadlock-Bots/.tasks/2026-10-07-concierge-coaching-fallback |
| settle --selbst Nachläufer (claude-config) | d9b6f3e7-9149-45b5-a6a8-ba79ff37f3e4 | d3a1741e | 2026-10-07 | t3-harness | sol | läuft | eigene | fix/settle-selbst-nachlauf | - | claude-config/.tasks/2026-10-07-settle-selbst-nachlauf |
| Offene Branches nach main (claude-config) | 5c407477-e57e-4f42-b037-101eb242b046 | d3a1741e | 2026-10-07 | t3-harness | sol | läuft | - | - | - | claude-config/.tasks/2026-10-07-offene-branches-main |
Nachtrag 2026-10-07: Wächter 48f2c6a1 fertig und gesettelt; e5d43083 hing auf running, per interrupt/stop beendet und gesettelt. claude-config kanonisch auf main f59a54c (Backup backup/canon-dirty-20261007, ~/.cache/claude-config-canon-untracked-20261007.tgz).
Entscheidung Nutzer 2026-10-07: YouTube-Audit bei Google steht noch aus (Uploads bis dahin privat, Dashboard zeigt echte Sichtbarkeit aus D). Instagram nicht zum Go-Live.
| I Bestandsaufnahme | 18612f8f | fertig, Bericht docs/social-media-inventar cfedfe84; settle nach Turn-Ende nachholen |
| R1 Aufräumen Runde 1 | d0852699-58e5-40cf-988d-3ee5c5f473d4 | d3a1741e | 2026-10-07 | t3-harness | sol | läuft | eigene | fix/social-aufraeumen-1 | - | Inventur-Punkte 1 bis 3 plus Fetch-Fehler |
| V VOD-Archiv ausbauen | b8b0bffd-22d6-4b43-a418-fdd19a79edb2 | d3a1741e | 2026-10-07 | t3-harness | sol | läuft | eigene | feat/vod-archiv-ausbau | - | ~/vod-archive in tb-vod-archive, Verwaltungsseite |
| S Social-Media-Manager Route | 2fae9d96-5ecf-4ccf-b7fa-c0711b6f56e2 | d3a1741e | 2026-10-07 | t3-harness | sol | läuft | eigene | feat/social-media-route | - | /social-media-admin -> /social-media, Name Social-Media-Manager |
| P Twitch-Seiten unter /twitch/ | dcf21784-9bf2-4b90-b4e3-fb9775ade7c3 | d3a1741e | 2026-10-07 | t3-harness | sol | läuft | eigene | feat/twitch-pfade | - | /social-media und /analyse nach /twitch/, Rest inventarisieren |
| C Deploy-Prüfmodus (claude-config) | fbbb7435-4dda-4f48-bb49-c2dbde58283d | d3a1741e | 2026-10-07 | t3-harness | sol | läuft | eigene | - | - | deploy-twitch-release --pruefen |
| Brain-Delegator Astra | 481426fe-b477-42b3-91c6-901811fcba1d | d3a1741e | 2026-10-07 | codex | astra | läuft | - | - | - | Deadlock-Brain/.tasks/2026-10-07-brain-fertigstellung-astra |
| K Twitch-Kanon nachziehen | 995d0026-b3aa-40db-a5df-2886661fe227 | d3a1741e | 2026-10-07 | t3-harness | sol | wartet auf Ende von P und V, dann 'fortsetzen' senden | - | - | - | claude-config/.tasks/2026-10-07-twitch-kanon-nachziehen |
