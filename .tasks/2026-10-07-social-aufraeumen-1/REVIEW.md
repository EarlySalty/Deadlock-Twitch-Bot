# Merge-Gate

1. Der erste Lauf verwendete das veraltete lokale `main` (`d8284816`). Dadurch gingen rund 6,4 Millionen Zeichen inklusive bereits integrierter Änderungen an den Gate. Die Kette fiel ohne Urteil aus. Das war kein BLOCK.
2. Der korrigierte Lauf verglich den Auftrag mit dem tatsächlichen Ausgangsstand `origin/main` (`9315b3cf`). Urteil des konfigurierten Modells `gpt-6.1-sol`: `ALLOW: No grounded merge-blocking defect found in the supplied diff.`
3. Ein NIT verlangte Bilder des gesperrten Schalters während des Ladens und bei fehlender Twitch-ID. Beide Bilder sowie der freigegebene Zustand liegen unter `/home/nathanael/.claude/sichtpruefung/social-aufraeumen-1/`. Die Browserprobe prüft zusätzlich die fehlende horizontale Überbreite und den tatsächlichen ID-basierten PUT aus der Detailseite. Die Antworten sind ausdrücklich lokale Testfixtures, kein Livebeweis.
4. Der anschließende echte Downloadtest erkannte, dass yt-dlp den Suffix `.part` aus dem vorgegebenen Dateinamen entfernt. Der atomare Kern verwendet deshalb eine eindeutige temporäre MP4 mit `.tmp.mp4`. Der Test wurde nicht abgeschwächt.

Der Gate muss nach diesem Downloadfix und der Integration des inzwischen fortgeschrittenen `origin/main` erneut laufen. Es gab bislang keinen inhaltlichen BLOCK und damit keinen Gate-Fixer.
