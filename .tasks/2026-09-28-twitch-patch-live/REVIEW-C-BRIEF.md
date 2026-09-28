status: aktiv (2026-09-28)

# Unabhängiges Vorabreview C: Patchfeed

Lies `AUFTRAG.md`, `PAKETE.md` und `WORKER-C.md` im selben Ordner. Prüfe den gepushten Commit `7593923f` im Worktree `/home/nathanael/.worktrees/twitch-patch-feed-20260928` auf Branch `feat/twitch-patch-feed-20260928` gegen Basis `992e2659`. Autor war GPT 6 Sol. Dies ist ein lesender Vorabreview, nicht die spätere gemeinsame Review-Runde für A, B, C und D. Keine Dateien ändern, keine Unter-Threads starten, nicht committen oder mergen.

Ziel: Ein neuer, fertig veröffentlichter Einzelartikel wird beim laufenden Bot genau einmal als Ereignis an den späteren Empfänger gegeben. Der erste erfolgreiche Abruf legt dagegen ausschließlich eine dauerhafte Baseline aller vorhandenen IDs an. Der Callback erhält `PatchArticle { id, url, source_url, observed_at }`. Paket B baut später `twitch_patch_feed_progress(singleton, last_processed_patch_id, pending_patch_id, pending_observed_at)`.

Prüfe besonders die Rennen bei parallelen Bot-Instanzen und Neustarts, den unveränderlichen Beobachtungszeitpunkt, die Reihenfolge mehrerer neuer IDs, Fehler vor und nach einem Callback, Abbruch bei ungültigem oder zeitweise unvollständigem Artikel, strikte Website-URL ohne fremde Weiterleitung, die echte Originalquelle, die Baseline ohne fest verdrahtete Patch-ID und den Fall eines über 120 Sekunden alten Pending-Ereignisses. Gib für jeden Befund Datei:Zeile, konkreten Eingabefall, Folge und kleinsten Fix an. Prüfe außerdem, ob die 10 gemeldeten Tests und der Build den produktiven DB-Pfad oder nur Fakes abdecken; fehlende Prüfung ausdrücklich nennen, aber nicht pauschal als Defekt werten. Am Ende: `fertig J/N`, Abweichungen, `Fix nötig J/N`.

Melde dein Urteil an Intent-Thread `4ddc68d5-0c42-41ce-b02c-c1be909c20fd`. Bei Bump-up: `[Bump-up] Review C: Grund: ... Erledigt: ... Worktree: /home/nathanael/.worktrees/twitch-patch-feed-20260928 Offen: ...`. Keine Testnachricht an echte Twitch-Kanäle.
