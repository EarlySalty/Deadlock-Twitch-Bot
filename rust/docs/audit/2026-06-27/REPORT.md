# Re-Verifikation Python→Rust-Parität (Twitch-Bot) — Bericht

**Stand:** 2026-06-27, HEAD `97bcc56`. **Methode:** Codex-delegiert (gpt-5.5/xhigh), Claude als Orchestrator/Reviewer.
**Frage des Owners:** Ist wirklich alles 1:1 (oder als bewusste, belegte Verbesserung) nach Rust portiert — weniger Bugs, sauber?

## Kurzfazit

Die Migration ist **substanziell vollständig und treu**. Von 232 Befunden der letzten Vollaudit sind 192 belegt
behoben, 10 sind bewusste, dokumentierte Drops. Der **frische, skeptische Sweep aller Domänen** (nicht nur ein
Nachzählen alter Befunde) bestätigt für den Großteil Parität oder belegte Verbesserung — findet aber **echte
Lücken, die frühere Audits übersahen**. Keine davon ist ein akuter Live-Ausfall (das System läuft, #291), aber
mehrere sind reale latente bzw. Verhaltens-Bugs. Genau diese Liste war das Ziel.

**Die Wurzeln (statt 35 Einzelprobleme):**
1. **Schema-Typ-Drift (P1, latent):** Die Rust-*Baseline-Migration* legt 6 Analytics-Spalten als `int4/real/text` an,
   während Rust-Code **und** der `prod_contract`-Test `bigint/double/timestamptz` erwarten. Live-DB unbetroffen
   (Inferenz), aber ein **Neuaufbau aus Rust-Migrationen würde die Kern-Analytics still brechen** — eine Disaster-
   Recovery-/Deploy-Falle.
2. **„Implementiert aber nie verdrahtet" (5×, bestätigt tot):** Raid-Analytics-Datensicht, Social-Token-Refresh-Worker,
   Social-Admin-SPA, Raid-Orphan-Replay, reauth-all (Port nicht injiziert). Tests grün, Feature trotzdem tot.
3. **Outbound-Suppression/Opt-out (P2):** Mehrere Sendepfade (Targeted-Promo, Auto-Ban-Notice, Eskalation, Go-Live-
   Tipps, ReAuth) umgehen den in Python zentralen Opt-out/Suppression-Guard.
4. **Billing-Logik (P1, eng):** Checkout-Referenz auf Login gedreht, Trial-/Paid-Check prüft aber user_id → ein
   bezahltes Abo kann als „kein Plan" gelesen und von einem `analytics_trial`-Override überdeckt werden.
5. **Go-Live-Announcement (P1.15, P1):** Gespeicherte UI-Anpassungen der Streamer werden nicht ins Template
   normalisiert → greifen still nicht (Fallback auf Defaults).

Volle Belege je Befund: `findings/B*.md` (Roh) und `verified/C*.md` (Gegenprüfung). `00-baseline.md` = Intent-Ledger + Re-Status.

---

## Verifizierte Befunde nach Priorität

### P1 — echte Funktions-/Datenbugs (Fix empfohlen)

| ID | Befund | Wirkung | Live? | Fix (Stelle) |
|---|---|---|---|---|
| B4-023 | Raid-Retention-Collector zählt `known_from_raider` ohne `first_seen_at < executed_at` | Retention-Metrik falsch (Post-Raid-Ankömmlinge als „bekannte Raider") | **JA** (periodischer Collector schreibt falsch; Dashboard-Recalc ist korrekt) | Bedingung ergänzen `raid_retention.rs:173-195` |
| B4-024 | `target_session_id` als `$6::int4` gecastet, gelesen als i64 | Bricht für Session-IDs > int4-Bereich | Latent-Live | Cast bigint + Baseline-Spalte `raid_retention.rs:118-132` |
| B7-022 | Trial-/Paid-Check nutzt user_id, Checkout-`customer_reference` auf login gedreht | Bezahltes Abo gilt evtl. als „kein Plan"; Trial-Override überdeckt paid sub. **Eng/bedingt** (siehe `verified/C4`) | **JA** (für betroffene Refs) | `trial.rs`/`plan.rs` an login-Referenz angleichen |
| B3-P1-01 (P1.15) | Go-Live-Announcement UI-Config wird nicht ins Template normalisiert | Streamer-Custom-Content/Titel/Felder/Rollen-Ping greifen still nicht (Default). Teilkorrektur: `button.label` wird gelesen | **JA** (user-sichtbar) | Python-Mapping `_to_template_config`/`_normalize_live_announcement_config` in Rust-Ladepfad portieren (`announce/dashboard_config.rs` → `sink.rs`) |
| B5b-05 | Raid-Analytics-Datensicht (partner_stats/leecher/manual_raids) nicht erreichbar — Handler nirgends registriert | Alte Datensicht unnutzbar (mehr als SSR→SPA) | **JA** (user-sichtbar) | Route registrieren (`raid_network_analytics_handler`) |
| B8-009 | Social `TokenRefreshWorker` nie in `tb-bot` gespawnt | Proaktiver Plattform-Token-Refresh tot → Plattform-Ops scheitern nach Token-Ablauf | Latent (Social opt-in/aus) | Spawn in `main.rs:1215-1236` ergänzen |
| **B9-001..004 (Cluster)** | Baseline-Migration `int4/real/text` statt `bigint/double/timestamptz` für `twitch_stream_sessions.{id,avg_viewers}`, `twitch_session_viewers.{session_id,ts_utc}`, `twitch_chat_messages.{session_id,message_ts}` | **Neuaufbau** aus Rust-Migrationen bricht Kern-Analytics still (sqlx-decode / `text < timestamptz` / int4-FK). Live-DB unbetroffen (Inferenz) | **Nur Fresh/Rebuild** | Korrektur-Migration `ALTER … TYPE` analog zu bestehenden `*_bigint`/`*_timestamptz`-Repair-Migrationen |

> **B4-016..022, B4-026, B4-027** sind Folgen des Schema-Clusters (gleiche Spalten) — sie brechen nur auf einer frisch
> aus Rust-Migrationen gebauten DB, nicht live. Fix des Clusters erledigt sie mit.

**Empfohlene 1-Zeilen-Live-Verifikation** (wandelt die einzige Inferenz in Fakt; bitte du oder mit Freigabe ich):
`psql … -c "\d+ twitch_stream_sessions" \d+ twitch_session_viewers \d+ twitch_chat_messages` → bestätigt, dass die
Live-DB `bigint/timestamptz/double` führt (erwartet: ja, da aus Python migriert).

### P2 — Teil-/Edge-Funktionsverluste

| ID | Befund | Anmerkung |
|---|---|---|
| B1-019 | Targeted-Promo sendet ohne Promo-Suppression-Check | jeder fällige Slot kann trotz aktiver Suppression senden |
| B1-022 (P2.5) | Auto-Ban-Notice ohne Opt-out/Suppression-Gate | frische Auto-Bans in aktiven Partner-Channels |
| B1-024 (P2.2) | Eskalations-Timeout-Chattext (`StrongTimeout.text`) wird nie gesendet | Timeout+Alert passieren, öffentlicher Text fehlt jedes Mal |
| B1-021 (P2.3) | Kein zentraler Outbound-Opt-out/Suppression-Guard | **herabgestuft**: Hauptpipeline über `is_partner_active` vorgefiltert; DB-Suppression fragmentiert, einzelne Direktpfade (Go-Live-Tipps/ReAuth/OAuth-Greeter) ungegated |
| B8-019 | Social-Admin-SPA: Live-Router bindet Stub-Redirect statt echtem Handler | P2.66 faktisch **nicht** behoben (toter Pfad) |
| B2-RAID-10 | Raid-Orphan-Replay implementiert, aber nicht verdrahtet | frühe Chat-Notification erst verzögert als Orphan korreliert (self-heilt) |
| B5b-06 | Public-API: `null` statt `""`/`0`; DB-Fehler nackte 500 ohne JSON | potenziell client-brechend für Public-Consumer |
| B5b-07 | Mehrere Analytics-401 ohne `loginUrl` | `dashboard_v2` leitet nur mit `loginUrl` um → Login-Redirect bricht |
| B9-020 | `connect_timeout` aus Config geladen, aber in `tb-db::connect` nicht angewandt | verändertes Failure-Verhalten bei DB-Stalls |
| B9-021 (Alert) | `TWITCH_ALERT_CHANNEL_ID` hardcoded statt Env | nicht im Ledger als Drop belegt (Stats-Teil dagegen bewusst) |
| B5a-004 | `/healthz`,`/readyz`,`/health` Root-Probes nativ fehlend | Ops-/Monitoring-Probes können 404en (kein bewusster Drop belegt) |
| B9-019 | `twitch_engagement_sender_auth` nur Runtime-DDL, nicht in SQLx-Migrationen | widerspricht DDL-SSOT (ADR-0002); Fresh-DB hängt an Runtime-Seiteneffekt |
| B5a-001 | Invoice-Preview/Page-Links beworben, Route nicht registriert → 404 | sollte auf Stripe-Portal/„kein Feature" zeigen statt 404 |
| B5a-002 (P2.132) | Affiliate-Self-Service (OAuth/Connect/Claim/Profile-API) nativ fehlend | Portal-Read-Model existiert; PDF/Payout bewusst raus → **Intent-Frage** (s.u.) |

### P3 — kosmetisch / operational / Rand

B4-008 = B5b-11 (dyn. Bot-Filter, nur bei abweichendem Bot-Nick sichtbar) · B5b-08 (500 ohne `code`-Feld) ·
B5b-09 (market `error_id`) · B5b-10 (title Request-/History-Shape, kleiner als behauptet) ·
B7-017 (tips `record_feature_used` nie geschrieben → Feature-Ranking uninformiert) ·
B6-P3-002 (reauth-all 503 bis Port-Injektion) · B6-P3-003 (internal-API bind-Backoff fehlt) · B6-P3-004 (/raid/requirements nicht 1:1).
*(B6-P2-001 App-Token-Circuit-Breaker verpasst `error`-only-Bodies — Befund aus Welle 1, in Phase C nicht separat gegengeprüft.)*

---

## Verifiziert geklärt — KEIN Bug (Skepsis entlastet)

- **B8-011 Transcription-Engine fehlt** → **bewusster Drop**, Grillme Block 15 („Transkription raus", OpenAI/Whisper gestrichen).
- **B1-015/B7-004 `!engagement_on/off`-Chat-Command fehlt** → **LOW**: Aktivierung übers Dashboard erreichbar (`/engagement/toggle`, `/engagement/mode`); nur Chat-Komfort fehlt.
- **B9-024 `tb-llm`-Abstraktion nicht adoptiert** → **LOW**: Architektur-Schuld, kein Funktionsverlust (Direktpfade nutzen MiniMax/Anthropic, Ledger gebucht).
- **B9-021 Stats-Teil (`TWITCH_STATS_CHANNEL_IDS`)** → bewusst (Discord-`!twl`-Leaderboard gedroppt, Web-Leaderboard neu).
- **P3.13 Blacklist-Raid-Whisper** → bewusst deferred bis Chat-Cutover (Code-Kommentar + Ledger).
- **P2.10 Stale-EventSub-Cleanup** → war im Altaudit „offen", ist aber **verdrahtet** (Altbefund stale).
- 192/232 Altbefunde belegt behoben; 10 bewusste Drops im Intent-Ledger.

---

## Empfohlener Fix-Plan (DAG, nach Aktionsklasse)

**Hinweis:** Ich habe **noch nichts geändert**. Die wertvollsten Fixes sind Rücksprache-Klasse (Schema/Live-DB,
Architektur, API-Vertrag, Intent). Bei „go" ziehe ich sie via Codex durch (implement → frischer Codex-Kritiker →
rework → Build/Test), user-sichtbare Texte schreibe ich selbst, danach CHANGELOG + Restart.

**Welle Fix-1 (Autopilot, risikoarm, sofort startbar auf dein Wort):**
- B4-023 `first_seen_at < executed_at` ergänzen (deckt sich mit korrektem Dashboard-Recalc).
- B1-024 Eskalations-Text tatsächlich senden.
- B5b-07 fehlende `loginUrl` in 401-Bodies vereinheitlichen; B5b-06 Public-Null/Errorbody angleichen.
- B9-020 `connect_timeout` anwenden; B5a-004 Root-Health/Readyz-Routen ergänzen.
- B5b-05/B8-019 tote Routen verdrahten (reine Wiring-Fixes, klare Stellen).

**Welle Fix-2 (Rücksprache — brauchen deine Freigabe/Entscheidung):**
- **Schema-Cluster (B9-001..004 + B4-024):** Korrektur-Migration `ALTER TYPE`. *Achtung:* eine neue Migration läuft
  beim nächsten Deploy auch gegen die **Live-DB** (großer Table-Rewrite/Lock möglich) — Plan + Wartungsfenster nötig.
- **B7-022 Billing-Logik:** Trial/Paid-Resolver an login-Referenz angleichen (Geld-relevant, sorgfältig + Test).
- **B3-P1-01 (P1.15) Announcement-Normalisierung:** UI→Template-Mapping portieren (user-sichtbares Verhalten).
- **Outbound-Suppression-Guard (B1-019/021/022):** zentralen Opt-out/Suppression-Gate im Rust-Sendepfad einführen (Architektur).
- **B8-009 Token-Refresh-Worker spawnen:** verändert Laufzeitverhalten eines (opt-in) Live-Dienstes.

**Intent-Entscheidungen für dich (kein Bug ohne deine Linie):**
- **Affiliate-Self-Service (B5a-002/P2.132):** OAuth/Connect/Claim/Profile bewusst gedroppt oder nachbauen?
- **B5a-001 Invoice-Links:** auf Stripe-Portal umbiegen oder Feature wirklich raus (dann Links entfernen)?
- **B9-021 Alert-Channel:** als Env konfigurierbar machen (wie Python) oder Hardcode akzeptieren?

---

## Coverage / Methodendisziplin (warum belastbar)

- 16 Python-Domänen frisch gegen 22 Rust-Crates geprüft (nicht nur Altbefunde nachgezählt).
- Jeder Nicht-Parität-Befund: Python-Ref **und** Rust-Ref (oder belegter „kein rg-Treffer").
- Jeder Befund durch unabhängigen Codex **refute-by-default** gegengeprüft; mehrere herab-/heraufgestuft, einige als Fehlalarm/deliberate verworfen.
- Einzige verbleibende Inferenz: Live-DB-Spaltentypen (Schema-Cluster) — per 1-Zeilen-`psql` final beweisbar.
