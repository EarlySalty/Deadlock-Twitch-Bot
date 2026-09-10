# Plan: Werbemanager Queue-Phase

status: aktiv
datum: 2026-09-10
stand: 2026-09-10 — M1 bis M4 verifiziert grün, M5 läuft (Review + Merge/Deploy)

Ziel steht im Contract (`.tasks/2026-09-10-admanager-steam-queue/CONTRACT.md`).

## Verifizierungsstand

- tb-analytics: 462 lib (inkl. 5 Steam-DB-Tests) + 12 decide + 2 store grün (Test-DB, TB_TEST_REQUIRE_DB=1)
- tb-bot: 289 grün
- tb-dashboard-api ad_manager: 7 grün
- dashboard_v2: 181 Tests grün, tsc+vite-Build grün
- Browser-Preview: Werbemanager-Karte Desktop+mobil, Steam-Feld mit Dirty-Tracking, Fehlerpfad ehrlich (HTTP 502 ohne Backend), keine Konsolenfehler
- Baseline-Beweis: ad_manager_store queue_lease war vor der Änderung im isolierten DB-Lauf rot (git-stash-Lauf), Fix ist hermetischer Test-Aufbau

## Milestone M1 — Entscheider kennt Match-Status (tb-analytics)

Änderungen:
- Neuer Typ `SteamMatchState { in_match: bool, hero: Option<String>, stage: Option<String>, observed_at: DateTime<Utc> }`.
- `DecisionInput` um `steam_match_state: Option<SteamMatchState>` erweitern.
- `decide()` Smart-Zweig: nach Startschutz, vor Chat-Ruhe-Logik:
  `Some(state)` → `in_match` = Snooze/none("in_match[_no_snooze]");
  `!in_match` = Cooldown ok → Commercial("in_queue"), sonst Snooze/none("in_queue_cooldown").
  `None` → exakt heutiger Pfad (INV-03).
- Loader `steam_match_state(pool, channel_login) -> Result<Option<SteamMatchState>, sqlx::Error>`:
  twitch_engagement_settings.steam_id (per channel_login) × activity.live_player_state,
  Frische `COALESCE(deadlock_updated_at,last_seen_at) >= now() - 180s` (Konstante `MATCH_STATUS_FRESH_SECS`).
- Unit-Tests: InMatch snoozt, InMatch ohne Snoozes = none, Queue mit Cooldown = Commercial,
  Queue ohne Cooldown = Snooze, Unknown = heutiger Pfad, Frische-Grenze, Loader (Testschema).
Validierung: `cargo test -p tb-analytics`
Stop-Regel: rote Tests → fix, kein weiterer Milestone.

## Milestone M2 — Worker füttert den Entscheider (tb-bot)

Änderungen:
- `process_channel`: nach erfolgreichem `get_ad_schedule` den Loader aus M1 rufen;
  Fehler/None → `None` an `DecisionInput` (Fallback), `tracing::debug` statt Fehler-Alarm.
- Kein Verhalten außerhalb des Lead-Fensters (INV-08); Helix-Muster unverändert.
- Test: Source-Verankerung (Loader-Aufruf zwischen schedule und decide), plus
  Unit für Loader-Fehler → None-Pfad (falls ohne DB abbildbar).
Validierung: `cargo test -p tb-bot`
Stop-Regel: wie M1.

## Milestone M3 — Dashboard-API zeigt Steam-Block

Änderungen:
- `StatusResponse` um `steam: Option<SteamStatusDto>` erweitern
  (linked: bool, state: "in_match"|"in_queue"|"out_of_game"|"stale"|null, hero, stage, observedAt);
  Quelle: derselbe Loader (M1) plus altersbezogene Einordnung.
- Test: Testschema analog ads_schedule.rs (Steam-Zeile gesetzt → Block gefüllt; nichts gesetzt → null).
Validierung: `cargo test -p tb-dashboard-api`
Stop-Regel: wie M1.

## Milestone M4 — UI (dashboard_v2)

Änderungen:
- `api/adManager.ts`: Typen für steam-Block.
- `AdManagerSection.tsx`: Status-Karte Match-Zustand (inkl. Held/Stage/Alter);
  ohne Steam-Link Hinweis mit Verweis auf den KI-Engagement-Bereich (steamId);
  Smart-Beschreibung um Queue-Verhalten ergänzen; keine neuen Eingabefelder.
- Tests: bestehende dashboard_v2-Tests laufen lassen; Ergänzung, wo das Muster es hergibt.
Validierung: `npm run build` in bot/dashboard_v2 + Test-Suite des Pakets.
Stop-Regel: wie M1; UI-Verifikation im Browser (Desktop + mobil) nach Deploy-Vorbereitung.

## Milestone M5 — Doku, Review, Aufräumen

- Streamer-Doku prüfen (docs/streamer/FEATURES.md bzw. DASHBOARD.md): Werbemanager-Abschnitt
  um Queue-Verhalten + Steam-ID-Voraussetzung ergänzen (Rolle Doku-Redakteur: echte Umlaute,
  kein Bindestrich-Zwang, absolute Wörter vermeiden).
- Review mit frischem Kontext gegen Contract (Wirkungs-Prüfer), Findings abarbeiten.
- `python3 /home/nathanael/Documents/claude-config/bin/diff-policy.py` vor Merge.
- Retro-Artefakt + Status auf erledigt, sobald der user-sichtbare Endzustand geprüft ist.

## Gesamtstop-Regel

Sobald ein Milestone seine Validierung nicht grün bekommt: stoppen, fixen, erst dann weiter.
Plan-Abweichungen (falsche Annahme im M1–M5) → Plan auf überholt, neuer Plan, kein Code-Improvisieren.
