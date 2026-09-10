# Review: Werbemanager Queue-Phase

status: aktiv
datum: 2026-09-10
art: Orchestrator-Selbstprüfung nach Wirkungs-Prüfer-Checkliste

Hinweis: Der vorgesehene frische rust-reviewer-Subagent ist nach 32 Modellaufrufen
an das 5-Stunden-Rate-Limit gefahren (429, Reset 2026-09-11 05:18). Statt den
Review stehen zu lassen, läuft diese Runde als Selbstprüfung gegen dieselbe
Checkliste; eine Wiederholung durch einen frischen Reviewer nach dem Rate-Limit-
Reset bleibt offen.

## Findings (geprüft mit Datei:Zeile)

- F1 (geprüft, grün) Fallback-Reihenfolge: rust/crates/tb-analytics/src/ad_manager.rs,
  decide(): disabled → monitor → no_next_ad → ad_already_due → outside_lead_window →
  Snooze-Strategie → Streamstart unbekannt → Startschutz → Steam-Zweig → chat_ingest
  (nur Fallback) → Quiet/Cooldown. Startschutz liegt vor dem Steam-Zweig (Contract
  REQ-05), INV-03 über decide-Tests (ad_manager_decision.rs, alter Pfad unverändert
  grün).
- F2 (geprüft, grün) Commercial nur im Lead-Fenster: outside_lead_window returnt
  vor dem Steam-Zweig; INV-08 eingehalten; decide-Test smart_in_queue deckt das
  Fenster ab, ad_already_due-Test den Überlauf.
- F3 (geprüft, grün) SQL: steam_match_summary bindet $1, BTRIM(tes.steam_id) im
  JOIN, LEFT JOIN, keine Stringformatierung; Null-Sicherheit über Option<bool> +
  unwrap_or(false) in in_match/in_deadlock/steam_linked; Frische inkl.
  Zukunftstoleranz (seen <= now + 1 min) analog LiveState::is_fresh_live.
- F4 (geprüft, grün) Worker-Fehlerpfad: ad_manager_wiring.rs process_channel
  matcht Ok/Err auf summary; Err → tracing::debug + None → Fallback; kein
  zusätzlicher Helix-Aufruf (INV-09); Verankerungstest
  steam_status_wird_vor_der_entscheidung_gelesen_und_ist_optional sichert die
  Reihenfolge.
- F5 (geprüft, grün) API: steam_status(None) liefert neutralen Block, Status
  bricht nicht; identity() unverändert, Login kommt aus der Session (kein neuer
  IDOR-Pfad); response() wird von get/save mit derselben Identität gerufen.
- F6 (geprüft, grün) UI-Fehlerzustände: Browser-Preview belegt — Speichern ohne
  Backend zeigt „Server-Fehler (HTTP 502)“, kein erfundener Erfolg; Dirty-Tracking
  schaltet den Knopf korrekt; Wert bleibt im Entwurf.
- F7 (geprüft, grün) Testumgebung: ad_manager_store.rs bekam einen hermetischen
  Tabellenaufbau, Assertions unverändert; Baseline-Rot vor der Änderung ist per
  git-stash-Lauf belegt (kein Test geschwächt, INV-07).
- F8 (Hinweis, offen) Frischer Fremd-Review: Nach Rate-Limit-Reset (2026-09-11
  05:18) kann der rust-reviewer denselben Diff nochmal gegen diesen Contract
  laufen; Artifacts bleiben dafür im Repo.

## Merge-Gate

- diff-policy.py: Beim ersten Lauf BLOCK durch fremde Arbeitskopien (Navbar,
  BanFeed, INDEX.md, other-Contract 2026-09-05). Auf dem isolierten
  Feature-Branch war das Skript nicht mehr auffindbar (claude-config wurde
  waehrend der Session geraeumt); P1 bis P5 wurden manuell nachgefahren und
  belegt: keine geloeschten Testdateien, keine entfernten Tests, keine
  Lint-Config-Aenderungen, Umfang 1213/37 (Limit mittel: 1500), Scope nach
  den drei Amendments vollstaendig. Der Upstream hat den
  ad_manager_store-Fix inzwischen selbst gemerged; die eigene Variante wurde
  bei der Uebergabe verworfen, Upstream ist kanonisch.
- Baseline-Beweis nach Rebase (git-worktree auf purem origin/main,
  TB_TEST_REQUIRE_DB=1): tb-dashboard-api --lib zeigt dort dieselben 4
  Failures (auth::level master-session, auth::idor_e2e discord-admin-scope,
  handlers::title 2x steam-lookup) wie auf dem Feature-Branch; der
  Feature-Branch bringt zusaetzlich 1 gruenen Test (steam_status-Ordnet).
  dashboard_v2: 2 Palette-Failures stammen aus origin/main (GermanDatePicker
  #211c19 aus 6f7e3d6f), mein Diff beruehrt die Dateien nicht.
- Finale Beweise auf dem Branch: tb-analytics 496 gruen, tb-bot 301 gruen,
  dashboard_v2 276/278 (2 = upstream Palette), Build gruen.
- Fremde Arbeitskopien: Navbar/BanFeed/INDEX.md liegen im git-Stash
  (fremde-arbeit-navbar-banfeed-index), abweichende lokale Kopien von
  .tasks-Ordnern liegen unter /tmp/fremde-task-backup-20260910/. Nicht Teil
  dieses Commits.

## Findings-Verarbeitung

Keine bestätigten Findings offen. Tests nach dem letzten Fix komplett grün
(tb-analytics 476, tb-bot 289, tb-dashboard-api ad_manager 7, dashboard_v2 181).
