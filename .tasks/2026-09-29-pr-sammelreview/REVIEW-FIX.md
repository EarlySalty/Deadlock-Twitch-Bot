# Unabhängiger Review nach der Fix-Runde (2026-09-29)

Du bist der einzige Thread für deinen PR. Keine Unter-Threads oder Unter-Agenten. Du bist nicht der Autor des Fixes und fixt NICHT selbst.

## Auftrag

1. Den ursprünglichen Review-Kommentar (Link in deiner ersten Nachricht) und den Fix-Kommentar des Fixers lesen.
2. Worktree wie in `AUFTRAG.md` Schritt 2 (`/home/nathanael/.worktrees/tb-rv-<nr>`, detached auf den PR-Head). Nie im geteilten Checkout arbeiten.
3. Je Befund prüfen, ob er wirklich behoben ist, und den Fix-Diff auf neue Bugs, Security, Wirkung am Produktionspfad, Migrationen, Umlaute, Em-Dashes und LLM-Regeln prüfen. Prüfungen wie in `AUFTRAG.md` Schritt 3 (fmt, clippy, test der betroffenen Crates, kein `--release`, `timeout 1200`, `set -o pipefail;`).
4. Urteil:
   - GUT: Merge nach main exakt nach `AUFTRAG.md` Schritt 4 GUT plus Nachtrag 1 und 2 (Scheduler-Queue, eigener Eintrag, Holder `tb-rv-<nr>`). Draft vorher `gh pr ready`. Danach PR als merged prüfen, sonst mit Merge-SHA schließen, Branch löschen.
   - PROBLEME: PR-Kommentar mit nummerierter Liste (Datei:Zeile, was falsch, was zu tun), Deutsch, echte Umlaute, keine Em-Dashes. Kein Merge.
5. Kein Deploy, kein Neustart, keine Prod-Migration. Worktree am Ende entfernen.

## Schlussmeldung

```
REVIEW PR #<nr>: GEMERGT <sha> | NACHBESSERN (Kommentar-Link) | BLOCKIERT (Grund)
Befunde alt: <n behoben>/<n gesamt>, neue Befunde: <Liste oder keine>
Geprüft: <Befehle und Ergebnis>
Migrationen: <Dateien>, Prod angewandt ja/nein
```
