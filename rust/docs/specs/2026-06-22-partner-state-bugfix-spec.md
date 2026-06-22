# Partner-State / Subscription / Auto-Raid — Bugfix-Spec (2026-06-22)

**Status:** Design — wartet auf User-Review, dann writing-plans → GPT-Worker-Delegation.
**Quelle:** Adversarial verifizierter Bug-Hunt (2 Workflow-Läufe, 63 Rohbefunde → ~38 unique, je gegen echten Code UND echte Prod-DB gegengeprüft).
**Detail je Bug:** in den Workflow-Output-Files (persistent):
`/tmp/claude-1000/-home-naniadm-Claude-Native-Workspace/fb74f638-5288-4e98-b7d5-a8af8c19ebe5/tasks/w4owgx6ij.output` (Lauf 1, 33) und `…/wuxvv2703.output` (Gap-Fill, 30).

---

## 1. Problem & Blast-Radius

Auslöser: Auto-Raid feuerte nicht bei earlysaltys Offline. Ursache ist systemisch, nicht earlysalty-spezifisch.

- **38 von 53 aktiven Streamern haben KEINE EventSub-Core-Subs** (`stream.online/offline/channel.update`). Poller-Tracked-Set (`status='active'`) = 53, Core-Sub-Reconcile-Set (`is_partner_active=1`) = 15.
- **Größter Treiber: `admin_archived_at`.** `on_auto_archive` (Inaktivität >N Tage kein Deadlock) hat ~32 aktive Partner auf `admin_archived_at` gesetzt → View `is_partner_active=0`.
- **Zweiter Treiber: `raid_bot_enabled=0`** (Token-Error-Pfad) für earlysalty, xoralle, ismile_e.
- **Systemweit keine neuen Stream-Sessions seit 2026-06-16** (i32→BOOLEAN-Bind in `start_session` + `apply_finalize`) + 6 Zombie-Sessions (nie finalisiert, unbegrenztes Sample-Wachstum).
- Folgen: kein Auto-Raid, keine Sessions/Analytics, kein Go-Live-Tracking für die Mehrheit der Partner.

## 2. Cross-Cutting-Direktiven (User, verbindlich)

1. `manual_verified_*` komplett aus Rust entfernen.
2. `is_partner_active` von `raid_bot_enabled` **entkoppeln** — Raid-Toggle darf nicht das gesamte Stream-Lifecycle-Tracking killen.
3. Status sauber trennen — eigene, disjunkte Achsen: `archived_at` = reine Dashboard-Flag; `admin_archived_at` = **Operator**-Deaktivierung; **Ban** = eigener Status (≠ admin_archived); **Token-Error** = eigener technischer Status (≠ `manual_partner_opt_out`). Inaktivität = eigener Status (≠ admin_archived).
4. `on_auto_archive` darf **nicht** `admin_archived_at` schreiben.
5. Reauth muss reaktivieren (raid_bot_enabled + technical_pause_reason heilen), **unabhängig** von einer Discord-ID im OAuth-State.
6. Auto-Raid zusätzlich aus Poller-Offline-Erkennung auslösen (Redundanz zum EventSub-Pfad).

**Zielmodell `is_partner_active` (nach Fix, mit User-Entscheidungen 2026-06-22):**
`is_partner_active = status='active' AND manual_partner_opt_out=0 AND technical_pause_reason='' AND admin_archived_at IS NULL`
- **Raus:** der `raid_bot_enabled=1`-Konjunkt (Direktive 2 — reiner Raid-Toggle deaktiviert NICHT das Lifecycle-Tracking). `raid_bot_enabled` gatet danach NUR noch die Raid-Eligibility (`offline_eligibility.rs`).
- **Bleibt deaktivierend:** `technical_pause_reason` — Token-Error UND Ban sollen den Partner bis Reauth/Entsperrung **komplett** deaktivieren (User Q2: „token_error deaktiviert alles"). Token-Error wird über `technical_pause_reason='token_error'` kodiert (NICHT mehr über `manual_partner_opt_out`, Direktive 3). Ban = eigener Status, ebenfalls deaktivierend.
- **NICHT deaktivierend:** Inaktivität (User Q1) — eigener, rein informativer Status, `is_partner_active` bleibt 1.

## 3. Workstreams (DAG)

Reihenfolge ist Teil des Fixes — Keystone zuerst, sonst werden Folgebugs gefixt, die ohnehin wegfallen.

### Phase 0 — KEYSTONE (zuerst, blockiert alles andere)

**WS-A — View-Entkopplung + Status-Achsen (DB-Migration `CREATE OR REPLACE VIEW`)**
- `public.twitch_partners_all_state`: `is_partner_active`-CASE → `raid_bot_enabled=1`-Konjunkt **entfernen**; Token-Error/Inaktivität nicht einrechnen. Endform siehe Zielmodell §2.
- `operational_state`-CASE: muss `admin_archived_at` berücksichtigen (aktuell ignoriert es das → 34 Partner zeigen Dashboard „active", Bot behandelt sie inaktiv). Konsistent mit is_partner_active machen.
- Eine kanonische „partner active"-Definition: `tb-dashboard-api/src/auth/session.rs:1118-1161` führt eine **zweite, abweichende** Inline-CASE (ignoriert raid_bot_enabled) → auf die View-Definition vereinheitlichen.
- Betroffene Bugs: View-Coupling (`twitch_partners_all_state`), session.rs-Divergenz, operational_state-vs-is_partner_active (34 Zeilen), Ziel-Roster `partner_roster.rs:69-75`, Quell-Eligibility `offline_eligibility.rs:23-25` (NICHT anfassen — korrekte raid_bot_enabled-Kopplung), manual-!raid Ziel `auto_raid.rs:341-440`.
- **DoD:** earlysalty + alle nur-wegen-raid_bot_enabled inaktiven → is_partner_active=1; 13 Code-Konsumenten von `is_partner_active` (u. a. `tb-chat/channel_classifier.rs:117`, subscription_maintenance_loop, stats_native, leaderboard, network, partner_roster, title_jobs, post_stream, bans, market, streamers) regress-getestet; Dashboard-/Bot-Sicht konsistent.

**WS-E-core — Auto-Archive ≠ admin_archived_at (Direktive 4)**
- `on_auto_archive`/`on_auto_unarchive` (`bin/tb-bot/src/main.rs:150-198`): **User-Entscheidung Q1 = eigener nicht-deaktivierender Inaktiv-Status.** on_auto_archive schreibt NICHT mehr `admin_archived_at` und deaktiviert NICHT — `is_partner_active` bleibt 1, Subs/Sessions/Raid laufen weiter. Inaktivität nur als rein informative Dashboard-Markierung (eigene Spalte, z. B. `inactivity_flagged_at`, oder `operational_state='inactive'` rein anzeigend, NICHT in is_partner_active eingerechnet).
- `archive_candidates`-Cutoff (`poller/tracked.rs:109-149`) misst nur Deadlock-Sessions → aktive Nicht-Deadlock-Streamer fälschlich Kandidat. Mit Direktive-4-Umbau entschärfen.
- Datenfix (nach Code-Fix): die ~32 fälschlich `admin_archived_at`-gesetzten **aktiven** Partner zurücksetzen.
- Über die Lifecycle-Fassade statt direktem UPDATE (`streamers_crud.rs`), gemischte Zeitstempelformate (NOW() vs ISO) vereinheitlichen.

> Phase 0 erst nach Code-Fix + DB-Migration **gegen frisch-migrierte DB** verifizieren. Danach Datenfixes (admin_archived_at + raid_bot_enabled reset für die Betroffenen).

### Phase 1 — unabhängiger Critical (parallel zu Phase 0)

**WS-B — Session-Schema-Drift (i32→BOOLEAN)**
- `crates/tb-monitoring/src/sessions/store.rs`: `start_session` (Z. 186/189), `apply_finalize` (Z. 423), `adopt_incomplete` (Z. 297) binden i32 in BOOLEAN-Spalten `twitch_stream_sessions.is_mature`/`had_deadlock_in_session`. → `.bind(bool)` direkt; `GREATEST(COALESCE(bool,0),i32)` → `had_deadlock_in_session = had_deadlock_in_session OR $x`.
- Modul-Doc `store.rs:2-5` („INTEGER 0/1") korrigieren (Quelle des falschen Bind-Typs). Decode-Seite auf bool prüfen.
- Schema-Drift: Baseline-Migration `20260601000000_baseline_schema.sql` deklariert `integer`, Live-DB ist `boolean` → kanonisch auf boolean angleichen, Tests/Fixtures gegen frisch-migrierte DB.
- Error-Swallowing: `tracker.rs:214` von `debug!` → `error!` (systemweiter Ausfall blieb stumm).
- **CONFLICT-RESOLUTION:** Ein Verifier meldet „FALSE POSITIVE, Inserts laufen". Empirische Prod-Evidenz (reproduzierter `column is_mature is of type boolean but expression is of type integer` + 0 Sessions seit 16.6. + 6 Zombies) widerlegt das. → **TDD-Pflicht:** erst ein Test, der `start_session` gegen das echte migrierte Schema laufen lässt (muss rot sein), dann fixen.
- Datenfix: 6 Zombie-Sessions finalisieren, nachdem finalize funktioniert.

### Phase 2 — Reaktivierung & Token (nach Phase 0)

**WS-C — Reauth-Reaktivierung (Direktive 5)**
- `bin/tb-bot/src/raid_oauth_impl.rs:1133-1150`: Reauth-Zweig `(Some(setup), true)` ruft `sync_partner_state_after_auth` nur, wenn `state_discord_user_id` vorhanden → **Discord-ID-Gate entfernen**, unbedingt reaktivieren. (earlysalty-Wurzel.)
- `crates/tb-raid/src/auth_writer.rs:185-190` (`store_new_auth`): heilt `technical_pause_reason='token_error'`, aber NICHT `'token_error_expired'` → CASE erweitern (Idiom wie `session.rs:733`); `raid_bot_enabled=1` mitheilen.
- `crates/tb-internal-api/src/streamer_lifecycle.rs:619-636` (`reactivate_partner`): stellt `raid_bot_enabled` nicht wieder her. (Nach WS-A weniger fatal, trotzdem korrigieren.)
- `crates/tb-raid/src/partner_setup.rs:308-345`: `promote_streamer_to_partner` BLOCKT Reauth, wenn nicht-aktive Partner-Zeile `admin_archived_at` trägt → entschärfen.

**WS-G — Token-Error-Semantik (Direktive 3)**
- `crates/tb-raid/src/token_lifecycle.rs:613-626` (`mark_grace_expired`): `manual_partner_opt_out=1` **entfernen** (Token-Fehler ist kein manueller Opt-out, Direktive 3). `technical_pause_reason='token_error'` BLEIBT (deaktiviert is_partner_active bis Reauth — User Q2). Wert auf `'token_error'` vereinheitlichen (nicht `'token_error_expired'`).
- Grace-Deadlock: `error_count` bleibt bei 1 (`token_refresher.rs:316` + `token_blacklist.rs:199-209`) → `check_grace_periods` (≥3) feuert nie. `load_expired_grace` (`token_lifecycle.rs:588-603`) von `error_count` entkoppeln.
- Restore-Sweep: `restore_bot_banned_inner` reaktiviert raid_bot_enabled nur für `bot_banned`, nicht für `token_error*` → erweitern bzw. über den Auth-Restore-Pfad heilen.
- technical_pause_reason-Wertdrift `token_error` vs `token_error_expired` vereinheitlichen.

### Phase 3 — Subscriptions & Raid-Redundanz

**WS-D — Subscription-Churn**
- `cleanup_stale` (`subscriptions.rs:1035-1073`) löscht Subs aller Broadcaster nicht im `active_ids`-Set; Chat-Reconkile (`chat_wiring.rs:770-784`) legt sie 30 min später neu an → 176–191 Deletes/6h. → **gemeinsames Source-of-Truth-Set** für beide Loops (nach WS-A enthält das Set wieder alle aktiven Partner).
- `engine.rs:329-343` Go-Live-Gate für `ensure_offline_subscription` ebenfalls von raid_bot_enabled entkoppeln.
- `callback_url`-Exact-Match (`subscriptions.rs:1045-1047`): Alt-Subs bei URL-Wechsel nie aufgeräumt → app-token-basierte Zugehörigkeit prüfen statt blind `continue`.
- Fail-open bei leerem `active_ids` (`subscriptions.rs:1049` + `main.rs:1461-1488`): defensiver Guard, der bei leerem Set `cleanup_stale` überspringt (Query-Fehler ≠ „alles löschen").
- Periodendivergenz 6h vs 30 min als Folge der Set-Vereinheitlichung erledigt.

**WS-F — Auto-Raid aus Poller-Offline (Direktive 6)**
- Poller-Offline-Transition (`engine.rs:600-606`) als zweite Raid-Quelle verdrahten: `on_stream_offline_raid` an `PollHooks` (`poller/hooks.rs`) + Aufruf in engine.rs; analog zum EventSub-Pfad (`handlers.rs:346-358`).

### Phase 4 — Aufräumen (zuletzt)

**WS-H — `manual_verified_*` entfernen (Direktive 1)** — Cross-Subsystem: View `is_verified`-CASE + 3 Spalten, `streamer_lifecycle.rs` (51 Treffer), `partner_setup.rs:650`, `streamers_crud.rs`, dashboard admin_streamers/admin_audit_log, `tb-chat/commands.rs`, `archive_candidates`. Eigenes Ticket, schrittweise; verursacht aktuell keine Fehlfunktion. `tracked.rs:52` aliast is_partner_active als is_verified → entwirren.

**WS-I — Schema-Shadowing / Test-Schema-Hygiene** — ~37–58 Test-Fixture-Schemas (`t6f_roster`, `ps_*`, `t6a_*` …) in der Prod-Analytics-DB mit Schatten-Tabellen (`twitch_streamers_partner_state`, `twitch_raid_auth`). Akut harmlos (Bot connectet als `postgres`, search_path löst auf `public`), aber latente search_path-Bombe + verfälscht Diagnose. Ops-Task: nach Verifikation `DROP SCHEMA … CASCADE`. **Vor WS-A** kurz absichern, dass die Live-Bot-Connection wirklich die `public`-View trifft.

## 4. Migrations-Sicherheit

- View-Änderung als `CREATE OR REPLACE VIEW`-Migration; int/bool-Spalten-Angleichung als Migration. Beides gegen **frisch-migrierte** DB testen (Fixtures, die gegen Alt-Schema „lügen", neu ziehen — vgl. DB-Fidelity-Lektionen).
- search_path/Schema-Shadowing vor + nach jeder View-Migration aus Sicht des realen Bot-DB-Users gegenchecken (earlysalty is_partner_active).
- Reihenfolge Daten- vs Code-Fix: erst Code+Migration live, dann Daten-Reset (admin_archived_at, raid_bot_enabled) der Betroffenen — sonst Rückfall.

## 5. Entschiedene Status-Modell-Fragen (User, 2026-06-22)

1. **Inaktivität:** ✅ Eigener nicht-deaktivierender Inaktiv-Status — bleibt getrackt (Subs/Sessions/Raid), nur Dashboard-Sichtbarkeit. `admin_archived_at` wird von Auto-Archive NICHT mehr berührt.
2. **Token-Error & Lifecycle:** ✅ Token-Error **deaktiviert alles** bis Reauth (is_partner_active=0 via `technical_pause_reason='token_error'`), nicht nur Raid. → Reauth-Heilung (WS-C) ist damit kritisch.
3. **Ban:** ✅ Eigener Status, ≠ admin_archived (bereits vom User entschieden). Deaktivierend; nur Operator setzt/hebt.

## 6. Umsetzung

- Pro Workstream ein GPT-Worker (`gpt-5.5`, effort `xhigh`), eigener Worktree+Branch, **TDD** (Red→Green→Refactor), user-sichtbare Texte bleiben bei Claude (Platzhalter).
- Claude reviewt `changed_files`, baut (`cargo build --release`), testet (`cargo test`), verifiziert gegen Prod-DB+Logs, startet Service neu (`systemctl --user restart deadlock-twitch-bot-rust`), CHANGELOG → commit → push → Discord/In-App.
- Reaktivierung earlysalty + Betroffene als LETZTER Schritt, nachdem alle Wurzeln tot sind.
