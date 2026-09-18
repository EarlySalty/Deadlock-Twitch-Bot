# Fixer-Auftrag: Merge-Gate-BLOCK beheben (Paket A)

## Ausgangslage
Branch `feat/werbemanager-budget-backend`, Worktree
`/home/nathanael/.worktrees/tb-werbemanager-a`. Der Merge-Kritiker
(`claude-opus-4-8`) hat den Merge nach main mit BLOCK verweigert. Der Befund ist
berechtigt und muss an der Ursache behoben werden, nicht am Symptom.

## Der Befund woertlich
`rust/crates/tb-analytics/src/ad_manager.rs` (`plan_next_block`): In Fix-Runde 1
wurde `min_interval_minutes` in die Blockperiode gekoppelt
(`floor = max(retry, min_interval*60)`). Bei den ausgelieferten Defaults
(budget=3, min_interval=30) wird `floor=1800`: also 60s-Bloecke statt 30s,
Abstand 30 Min, nur 2 Bloecke/Stunde = 120s. Das gesetzte Stundenbudget von 180s
wird nie erreicht, `blocks_per_hour=3` widerspricht der real erreichbaren Zahl,
und `docs/streamer/WERBEMANAGER.md` (Abschnitt "Budget") behauptet weiter
"3 Minuten als sechs Bloecke von 30 Sekunden, etwa alle 10 Min". Das trifft jeden
neuen und jeden von `monitor` migrierten Kanal out-of-box. AUFTRAG Punkt 2 nennt
als Planer-Eingaenge Budget/Streamstart/gelaufene Werbung/Sperrzeit, **nicht**
`min_interval`.

Zweiter, gleichartiger Punkt aus dem Review: der 8-Minuten-Fall meldet
`blocks_per_hour=8`, obwohl bei `period=480` nur ~7 Bloecke in eine Stunde passen.

## Was zu tun ist (Ursache, entschieden)
AUFTRAG Punkt 2 ist der verbindliche Kern und gewinnt gegen die spaetere
Festlegung 6.

1. **Kopplung zuruecknehmen.** In `plan_next_block` `min_interval_minutes` wieder
   aus der Periodenrechnung entfernen: Blockperiode nur aus Budget-Gleichverteilung
   und Helix-Sperrzeit (`retry`) bilden, wie vor Fix-Runde 1. Den zusaetzlichen
   Parameter `min_interval_minutes` entfernen und die Aufrufer
   (`rust/bin/tb-bot/src/ad_manager_wiring.rs`) sowie die Tests
   (`rust/crates/tb-analytics/tests/ad_manager_decision.rs`) nachziehen.
2. **`blocks_per_hour` ehrlich machen.** Der gemeldete Wert muss der real in eine
   Stunde passenden Blockzahl bei der tatsaechlichen Periode entsprechen
   (`3600 / period`, abgerundet, mindestens 1). Der 8-Minuten-Fall darf nicht mehr
   8 melden, wenn nur 7 passen. Test entsprechend anpassen.
3. **Mangel 6 anders erfuellen.** Der sichtbare Mindestabstand darf nicht
   luegen. Im Dashboard (`AdManagerSection.tsx`, Paket B, Worktree
   `/home/nathanael/.worktrees/tb-werbemanager-b`, Branch
   `feat/werbemanager-budget-dashboard`) am Feld "Mindestabstand" einen kurzen,
   nutzersprachlichen Hinweis ergaenzen: der Mindestabstand betrifft Twitch-Pausen
   und die manuelle Werbung, nicht die automatische Budget-Verteilung im
   Smart-Modus. Kein internes Vokabular, keine Em-Dashes, echte Umlaute.
4. **Doku versoehnen.** `docs/streamer/WERBEMANAGER.md` (Abschnitt "Budget") so
   fassen, dass Text und ausgeliefertes Verhalten uebereinstimmen.

## Was NICHT angefasst wird
- Die Post-Matchende-Logik, die Migration-GRANTs, das `plan_fit`-Lesen im Status
  und `blocks_in_window` (Fix-Runde 1, bereits abgenommen).
- Der Default `min_interval_minutes` wird **nicht** gesenkt und die Migration
  setzt ihn **nicht** zurueck (kein Produkt-Default-Umbau).
- Keine Code-Kommentare hinzufuegen.

## Testkommando (Repo Deadlock-Twitch-Bot)
Toolchain 1.97.1 aus `~/.rustup`, PATH und RUSTC auf
`~/.rustup/toolchains/1.97.1-x86_64-unknown-linux-gnu/bin`. Ausgabe in Datei, nie
Pipe hinter cargo, Exit-Code pruefen:
```
cargo test -p tb-analytics --test ad_manager_decision -- --include-ignored > /tmp/fix.txt 2>&1; echo EXIT=$?
```
Vorbestehende rote Baseline: 0 (Fix-Runde 1 lief mit 16 passed, 0 failed).

## Fertig-Kriterium
Build gruen, Tests gruen, `blocks_per_hour` ehrlich, Doku deckungsgleich, nur die
zwei eigenen Branches (A und ggf. B) gepusht, nichts Richtung main. Fertigmeldung
mit Commits je Branch und je Punkt ein Satz, was geprueft wurde. Danach stoppen;
der Orchestrator faehrt Review und Merge.
