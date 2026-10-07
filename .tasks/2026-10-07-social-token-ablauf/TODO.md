# Offener Abschluss

## Blocker

Der unveränderte lokale Merge-Gate endet in beiden vorgesehenen Aufrufen mit Exit 2. Sämtliche Modelle scheitern beim Erzeugen ihrer isolierten Prüfumgebung: `bwrap` und `unshare` melden `Cannot allocate memory`. Es gibt kein Codeurteil und kein ALLOW. Der genaue Befund steht in `REVIEW.md`.

Implementierung: Branch `fix/social-token-ablauf`, Commit `0ee53ca2`, Worktree `/home/nathanael/.worktrees/tb-social-token-ablauf`. Quellen, Migration und Nachweise sind erhalten. Der schmutzige Haupt-Checkout wurde nicht angefasst. Weitere Worker wurden nicht gestartet.

## Wiederaufnahme

1. Nach Reparatur der Host-Prüfumgebung `origin/main` frisch holen und den bestehenden Branch mit dem regulären `gate_hook.py --review` prüfen. Kein Wechsel in einen ungeschützten Prüflauf.
2. Nach ALLOW den Branch nach `main` bringen und mit `git push origin HEAD:main` pushen. Git-Schritte einzeln mit literalem Worktree-Pfad.
3. Die beauftragte Produktionsmigration als `postgres` mit `PROD-MIGRATION.sql` anwenden. Die neue Migration und ihre SHA384-Checksumme bleiben unverändert. Danach acht erforderliche Rust-Binaries aus dem eigenen sauberen Worktree bauen, `.twitch_build` gegen den aktuellen `origin/main`-SHA prüfen und mit den drei gebauten Frontends in einen eigenständigen Release-Clone übernehmen. `deploy-twitch-release <sha> <clone>` bleibt der Deploy-Weg.
4. Bot, Dashboard und weitere durch den Wrapper betroffene Dienste am laufenden Prozess nachweisen. TikTok soll beim echten Backfill-Refresh `refresh_expires_at` schreiben. Das ausgelieferte Dashboard-Artefakt muss die neue Anzeige enthalten. Der Discord-Pfad ist gegen die Wegwerf-DB und den lokalen Test-Broker belegt; kein echter Zugang wird als Probe entwertet.
5. Nach Live-Beweis Nachweise sichern, die eigene Test-DB und den fehlgeschlagenen eigenen Testcontainer prüfen und aufräumen. Branch erst nach erfolgreichem Ancestor-Check löschen, Worktree entfernen, den Hauptthread informieren und zuletzt den eigenen Thread settlen.

## Erhaltene Betriebsmittel

- Eigene Test-DB `tb_social_token_ablauf` im Wegwerf-Cluster auf Port 33045 bleibt zur Wiederaufnahme erhalten.
- Der zusätzlich für historische Migrationen angelegte Datenbankname `twitch_analytics` im Wegwerf-Cluster ist keine Produktionsdatenbank und wird nicht ungeprüft gelöscht.
- Der fehlgeschlagene Containername `tb-social-token-ablauf-db` muss beim endgültigen Cleanup auf Existenz und Zustand geprüft werden.
- Test- und Buildlogs sind im Task-Ordner ignoriert. Wertvolle Logs vor einer späteren Worktree-Löschung sichern.
- Der T3-Browserhost war nicht verfügbar. Eine tatsächliche Produktionsanzeige ist noch nicht bewiesen.
