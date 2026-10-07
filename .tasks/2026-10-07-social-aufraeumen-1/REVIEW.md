# Merge-Gate

1. Der erste Lauf verwendete das veraltete lokale `main` (`d8284816`). Dadurch gingen rund 6,4 Millionen Zeichen inklusive bereits integrierter Änderungen an den Gate. Die Kette fiel ohne Urteil aus. Das war kein BLOCK.
2. Der korrigierte Lauf verglich den Auftrag mit dem tatsächlichen Ausgangsstand `origin/main` (`9315b3cf`). Urteil des konfigurierten Modells `gpt-6.1-sol`: `ALLOW: No grounded merge-blocking defect found in the supplied diff.`
3. Ein NIT verlangte Bilder des gesperrten Schalters während des Ladens und bei fehlender Twitch-ID. Die Belege sind [Ladezustand](loading.png), [fehlende Twitch-ID](missing-id.png) und [freigegebener Zustand](ready.png). Sie liegen zusätzlich unter `/home/nathanael/.claude/sichtpruefung/social-aufraeumen-1/`. Die Browserprobe prüft die fehlende horizontale Überbreite und den tatsächlichen ID-basierten PUT aus der Detailseite. Die Antworten sind ausdrücklich lokale Testfixtures, kein Livebeweis.
4. Der anschließende echte Downloadtest erkannte, dass yt-dlp den Suffix `.part` aus dem vorgegebenen Dateinamen entfernt. Der atomare Kern verwendet deshalb eine eindeutige temporäre MP4 mit `.tmp.mp4`. Der Test wurde nicht abgeschwächt.

Der dritte Gate-Lauf prüfte den korrigierten Download und die Integration von `origin/main` (`0452e03c`), HEAD `2347772c`. Urteil: `[gpt-6.1-sol] ALLOW: No grounded merge-blocking defect found in the supplied diff.` Die beigefügten Bilder schließen den wiederholten Bildhinweis ab. Es gab keinen inhaltlichen BLOCK und damit keinen Gate-Fixer.
