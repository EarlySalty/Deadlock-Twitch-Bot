# OFFEN: Bot-Ban-Lifecycle (Stand 2026-07-16)

Übergabe-Notiz. Arbeit ist **nicht abgeschlossen**. Branch `fix/bot-ban-lifecycle` ist gepusht und NICHT nach `main` gemergt.

Diese Datei löschen, sobald alles unter "Offene Schritte" erledigt ist.

---

## Das Problem (bewiesen, nicht vermutet)

Der Bot bewarb auf Discord Partner-Kanäle weiter, in denen er auf Twitch **gebannt** war.

- Betroffen: `umiwaver` und `pixelpiratemarvin`, seit mindestens **2026-07-08**.
- Der Bot erkannte den Ban stündlich korrekt und warf das Wissen weg. Journal-Beleg: **2197 Treffer in 14 Tagen** mit `ensure_bot_is_mod: Bot ist im Kanal gebannt channel="umiwaver"`.

Das Announce-Gate (`poller/engine.rs`, `entry.is_partner_active`) war immer korrekt. Der Bug war die **Datenquelle**: Nichts schrieb den Ban-Zustand in die DB.

### Drei Detektoren, alle ins Leere

1. `send_message`-Fehler (`tb-chat/src/timeout_tracking.rs:181`) — funktioniert, feuert aber nur, wenn der Bot schreibt. In stillen Kanälen nie.
2. `channel.ban` EventSub (wird abonniert!) — `tb-monitoring/src/dispatch.rs:891` speichert nur Telemetrie und prüft **nie, ob das Ban-Ziel der eigene Bot ist**. Braucht ohnehin Mod-Status.
3. `ensure_bot_is_mod` → `AddModeratorOutcome::BotBanned` (`tb-bot/src/eventsub_hooks.rs:579`) — erkennt den Ban perfekt und stündlich, aber `tb-monitoring/src/subscriptions.rs:2001` machte nur `mark_perm_failed()` (In-Memory) + `tracing::info!`. **Das war der Haupt-Fix.**

### Der eigentliche Killer

`tb-raid/src/token_lifecycle.rs:521` `restore_ready_bot_banned_channels()` hob die `bot_banned`-Pause auf, sobald `needs_reauth = FALSE` — das ist der **OAuth-Token des Streamers**, orthogonal zu einem Chat-Ban.

Folge: Jeder Partner mit gesundem Token wurde binnen 1h entpausiert, Ban egal. `duzzel` steht **nur deshalb** noch korrekt auf `bot_banned`, weil sein Token zufällig kaputt ist (`needs_reauth=t`). `umiwaver` hat `needs_reauth=f` → der Detektor-Fix allein wäre **wirkungslos** gewesen.

---

## Was auf dem Branch fertig ist (verifiziert)

| Aufgabe | Inhalt |
|---|---|
| A | `BotBanned`-Signal auf den vorhandenen `BotBannedChannelHandler`-Port verdrahtet (kein zweiter Port) |
| B | Restore prüft echten Ban-Status via `ensure_bot_is_mod` statt `needs_reauth`. `BotBanStatus{NotBanned,Banned,Unknown}`, fail-closed, exhaustives match. Jede Entscheidung geloggt (`urteil`/`grund`), auch Ablehnungen |
| C | `twitch_exclusions` scharfgeschaltet (war tot, siehe unten) |
| Rework | `query_as!` in `tracked.rs` wiederhergestellt (Worker hatte es durch Runtime-Variante ersetzt = Verlust des compile-time Schema-Checks) |

**Selbst nachgeprüft, nicht dem Worker geglaubt:**
- Build grün mit `SQLX_OFFLINE=true` → Schema-Check nachweislich aktiv
- **3411 Tests bestanden, 0 fehlgeschlagen** auf dem mit `main` kombinierten Stand
- Clippy **byte-identisch** zur `main`-Baseline (11 vorbestehende Zeilen, keine neue)
- `.sqlx`: genau **1** neue Cache-Datei (nicht 838 — `cargo sqlx prepare` löscht immer erst alles und schreibt identische Hashes zurück; ein Zwischenstand mit 838 gelöschten Dateien ist normal)
- Die vollständige Query gegen die **echte Prod-View** getestet: 58 Zeilen, `is_partner_active` ist `integer`, kein Typbruch

**Drei vorbestehende rote Tests** (NICHT von diesem Fix, auf unverändertem `main` identisch rot verifiziert — beim Testen skippen):
`chat_analytics::tests::payload_aggregiert`, `chat_analytics::tests::tz_histogramm_verschiebt_stunde`, `row_structs_map_real_columns` (der ist in `tb-db`, nur via `cargo test -p tb-db --test hermetic` auffindbar).

---

## ⚠️ Notbremse in Prod — MUSS zurückgesetzt werden

Ich habe manuell in der Twitch-Analytics-DB gesetzt:

```sql
UPDATE twitch_partners SET technical_pause_reason = 'blocked'
 WHERE LOWER(twitch_login) IN ('umiwaver','pixelpiratemarvin');
```

Beide stehen damit auf `is_partner_active=0` und werden **nicht mehr beworben**. Das ist ein **Provisorium und semantisch falsch**: `blocked` heißt "wir haben ihn rausgeworfen", tatsächlich hat *er* uns gebannt. `blocked` heilt außerdem nie von selbst.

`blocked` statt `bot_banned` deshalb, weil der kaputte Restore-Sweep `bot_banned` binnen 1h wieder aufgehoben hätte.

**Nach dem Deploy zurücksetzen** (das ist gleichzeitig der Live-Beweis):

```sql
UPDATE twitch_partners SET technical_pause_reason = NULL
 WHERE LOWER(twitch_login) IN ('umiwaver','pixelpiratemarvin');
```

Dann muss der Detektor innerhalb einer Stunde **von selbst** `technical_pause_reason='bot_banned'` setzen. Prüfen mit:

```sql
SELECT twitch_login, technical_pause_reason, operational_state, is_partner_active
  FROM twitch_partners_all_state
 WHERE LOWER(twitch_login) IN ('umiwaver','pixelpiratemarvin');
```

Passiert das **nicht**, ist der Fix wirkungslos → nicht wegerklären, sondern Ursache suchen. Solange nichts passiert, sind beide Kanäle wieder aktiv und werden beworben — dann lieber die Notbremse erneut setzen.

---

## Offene Schritte (Reihenfolge)

1. **Gate-Fix abwarten + reviewen** — Worker `822a36083b33` lief zuletzt. Siehe nächster Abschnitt.
2. **Merge nach `main`** — `git merge --no-ff fix/bot-ban-lifecycle`, aus dem Worktree `~/.worktrees/Deadlock-Twitch-Bot-main`. Läuft durch Test-Gate + Merge-Kritiker.
3. **Deploy** — `cargo build --release --workspace` im **kanonischen Checkout** `/home/naniadm/Documents/Deadlock-Twitch-Bot/rust` (NICHT im Worktree!). Der Service startet `rust/target/release/tb-bot` von dort, via `rust/scripts/run_tb_bot_service.sh`. Dann `systemctl --user restart deadlock-twitch-bot-rust`.
4. **Live-Beweis, alle drei**: PID-Wechsel (`systemctl --user show deadlock-twitch-bot-rust -p MainPID --value`, vorher war `3602603`), `readlink -f /proc/<pid>/exe` zeigt auf die frische Binary, `journalctl --user -u deadlock-twitch-bot-rust --since "1 minute ago"` ohne `error|panic|fatal`.
5. **Notbremse zurücksetzen** → siehe oben. Live-Beweis der Kette.
6. **Aufräumen**: `git worktree remove ~/.worktrees/Deadlock-Twitch-Bot-botban`, `git worktree prune`, Branch löschen (erst nach `git merge-base --is-ancestor` + Exit-Code prüfen!). Docker-Test-DB `tb-review-pg` (Port 5437) stoppen: `docker rm -f tb-review-pg`. Evtl. auch `tb-gatefix-pg` (5439) / `tb-rework-pg` (5438) / `tb-test-postgres-botban` (5436).
7. **Diese Datei löschen.**

Discord-Post nur auf ausdrücklichen Wunsch des Users.

---

## Der Gate-Fund (Worker 822a36083b33)

Der Merge-Kritiker hat den Merge **blockiert** und dabei eine echte Lücke gefunden, die weder die 3411 Tests noch das Code-Review erwischt haben. Er hatte recht.

**Bug:** `is_partner_active` gatet nur das *neue* Go-Live-Posting. Hat der Kanal bereits `last_discord_message_id`:
- `engine.rs:~668` `if ... && !should_post` → `sync_live_announcement` läuft **weiter** und aktualisiert den bestehenden Post
- `engine.rs:~688` `ended_posting = ... && (!is_live || !is_deadlock)` → beendet nur bei offline/Kategorie-Wechsel, **nicht** bei inaktivem Partner

Folge: Bannt ein Streamer den Bot *während* er live ist, bleibt der Discord-Werbepost stehen und wird bis Stream-Ende gepflegt. Widerspricht CHANGELOG #382.

**Tücke:** `should_post == false` ist mehrdeutig — entweder "schon gepostet, nur aktualisieren" (soll laufen!) oder "Kanal inaktiv" (muss enden). Beim Fix darf der Normalfall nicht brechen; deshalb gehört ein Regressionstest dazu.

**Richtung:** `ended_posting` um `|| !entry.is_partner_active` erweitern, `sync`-Zweig zusätzlich mit `&& entry.is_partner_active` gaten. Auf Doppelwirkung achten (nicht erst syncen und dann enden, `end_announcement` nicht doppelt).

Ist der Worker nicht durchgelaufen: Auftrag steht in seinem Job-Log unter `~/.claude/gpt-workers/jobs/822a36083b33/`.

**Ein Gate-Deny ist kein Vorschlag** — Ursache beheben, nicht umgehen. Override kann nur der User selbst.

---

## Größere offene Baustelle: Flags raus, Tabellen rein

Vom User am 2026-07-16 ausdrücklich gewünscht. **Braucht Planung per `/grillme`, nicht nebenbei starten.**

### `twitch_exclusions` war tot

Die Tabelle (CHANGELOG **#225**, "saubere Ablösung von Flags") existiert live und ist befüllt — `fr4gm1nt`/`snaqeu` als `opt_out`, `skifahrertv` als `banned` — wurde aber von **null Produktions-Queries** gelesen. Nur 2 Migrationen und 3 Test-Fixtures kannten sie. Folgenlos nur zufällig, weil alle drei zusätzlich `departnered` sind. Aufgabe C hat sie scharfgeschaltet.

### Der Flag-Bestand in `twitch_partners`

`status`, `manual_partner_opt_out`, `technical_pause_reason`, `admin_archived_at`, `inactivity_flagged_at`, `raid_bot_enabled`, `silent_ban`, `silent_raid`, `verified`, `require_discord_link`

Die Views `twitch_partners_all_state` / `twitch_streamers_partner_state` rechnen daraus `is_partner_active` + `operational_state`. Das ist eine Ableitung **aus** den Flags, kein Ersatz.

### ⚠️ Zwei Achsen, nicht vermischen

Der wichtigste Punkt für den Cutover:

1. **Wir wollen den Kanal nicht** — Opt-out / harter Ausschluss. Permanent (Opt-out reversibel via `reactivated_at`). → `twitch_exclusions`.
2. **Der Kanal will uns nicht / technisch blockiert** — Bot gebannt, Token kaputt, Mod-Rechte weg. Betriebszustand, **selbstheilend**. → gehört **NICHT** in eine Permanent-Tabelle. Braucht eigene Zustandstabelle mit Historie.

`umiwaver` ist Achse 2: Der User will ausdrücklich, dass der Bot von selbst zurückkehrt, sobald der Ban aufgehoben ist. Ein Exclusion-Eintrag wäre falsch.

`technical_pause_reason` ist streng genommen kein Flag, sondern ein Zustandsfeld mit Werten (`bot_banned`, `token_error`, `blocked`) — nur als `TEXT` ohne Constraint modelliert, deshalb fühlt es sich wie eins an. Der schmerzhafte Teil sind die echten 0/1-Spalten daneben, die dieselbe Frage konkurrierend beantworten.

---

## Twitch-API-Fakten (recherchiert, nicht raten)

- **Twitch liefert kein passives Ban-Signal.** EventSub-Revocation kennt nur `user_removed`, `authorization_revoked`, `notification_failures_exceeded`, `version_removed`. Ein Chat-Ban ist nicht dabei.
- `GET /moderation/banned` bräuchte `moderation:read` — steckt in **keinem** Scope-Profil (`tb-raid/src/scope_profiles.rs`). Nachrüsten hieße Re-Auth aller 56 Partner.
- Deshalb ist der `add_moderator`-Probe (`channel:manage:moderators`, ist im Profil) der einzige praktikable Weg — und er ist gleichzeitig die Recovery-Aktion: Wer wieder Mod werden kann, ist nicht gebannt.
- `channel.ban` EventSub braucht `moderator:manage:banned_users`, also Mod-Status — den verliert man beim Ban. Deshalb ist Detektor 2 als alleinige Quelle unzuverlässig; in `umiwaver`s Kanal steht **kein einziges** Ban-Event in der DB.

## Sonstiges

- `duzzel` steht auf `bot_banned` mit `needs_reauth=t`. Nach dem Deploy prüfen, ob er korrekt behandelt wird (Restore darf ihn nicht anfassen, solange der Ban besteht).
- `albiiionlu` und `m1lanofps` stehen auf `token_error` — anderer Pfad, nicht Teil dieser Arbeit.
