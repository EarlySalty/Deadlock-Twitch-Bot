# Fixer-Briefing: Zeitplan-Retry schreibt fremde Werte zurück

## Rolle

Du bist der einzige Thread für dieses Paket. Keine Unter-Threads oder Unter-Agenten spawnen.

## Stand

- Repo: Deadlock-Twitch-Bot
- Worktree: `/home/nathanael/.worktrees/tb-social-studio-redesign`
- Branch: `feat/social-studio-redesign-20260921`, Head `2c4f4c46`
- Nie auf main pushen, nur auf dem Feature-Branch committen und pushen.

## Review-Befund (BLOCKING, externes Urteil 2026-09-22 00:45 gegen 2c4f4c4)

Datei: `bot/dashboard_v2/src/components/socialmedia/PostingPlanDraft.tsx`, Funktion `save()`, catch-Pfad:

1. Nach fehlgeschlagenem Save lädt catch den aktuellen Serverplan (`fetchPostingPlan`) und ruft `onSaved(latest)` auf. Damit wird der Plan-Prop der Elternkomponente auf den Serverstand gesetzt, fremde Änderungen eingeschlossen.
2. Gleichzeitig setzt catch `setBaseline(latest)` und behält mit `setDraft(effective)` den alten Vollentwurf.
3. Der nächste `save()` difft gegen `const base = plan`, also gegen den frischen Serverstand. Jede Abweichung des alten Entwurfs zu diesem Stand gilt als Nutzeränderung.
4. Repro des Reviewers: Nutzer ändert nur TikTok posts_per_week; ein anderer Akteur ändert YouTube max_posts_per_day von 3 auf 7; der TikTok-PUT scheitert mit 503; Refresh liest 7; der Retry sendet zusätzlich `PUT /platform/youtube {max_posts_per_day: 3}` und überschreibt den fremden Wert. Datenverlust im Fehlerpfad.

## Vertrag des Fixes

- Ein Retry nach fehlgeschlagenem Save darf ausschließlich Felder schreiben, die der Nutzer gegenüber der Basis dieses Entwurfszyklus geändert hat.
- Die Basis des Zyklus muss den Refresh überleben. Der save()-Diff darf nicht der live aktualisierten plan-Prop folgen; halte die Basis des Zyklus eigenständig (State oder Ref), gesetzt beim Beginn des Zyklus.
- Fremdänderungen an unberührten Feldern erzeugen kein Payload.
- Echte Konflikte (Nutzer und Fremder ändern dasselbe Feld) laufen weiterhin über den bestehenden conflict-Pfad; Verhalten außerhalb des Fehlerpfads bleibt unverändert.

## Testpflicht

- Regressionstest für „Fremdänderung + fehlgeschlagener Erstversuch + Retry" ergänzen. Der Reviewer bemängelt genau diese Lücke; die vorhandene Browserprobe deckt nur Fremdänderung bei erfolgreichem Erstversuch.
- Fokussierte Dashboard-Vertragstests und den produktionsbundle-basierten Social-Studio-Browsertest ausführen, Kommandos und Ergebnisse im Bericht nennen.
- Die volle Suite hat 5 bekannte Basisfehler (384/389); diese nicht jagen.

## Abschluss

- Commit auf dem Branch, danach `git push origin feat/social-studio-redesign-20260921`.
- Bump-up in den Intent-Thread `0d56f126-eb40-4b0c-b18d-2b14446d69dd` im Format:
  `[Bump-up] Paket zeitplan-retry: Grund: ... Erledigt: ... Worktree: ... Offen: ...`
  mit Commit-SHA, Testkommandos und Ergebnissen. Danach ist der Thread fertig.
