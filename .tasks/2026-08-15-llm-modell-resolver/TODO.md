# Offen: Modell-Resolver sauber bekommen

Stand 2026-08-15, Branch `feature/llm-modell-resolver`, Commit `a908f2c7`
(sitzt direkt auf `99bc323e` aus `feature/vod-auto-save`).

**Nicht mergen, solange die drei BLOCKING-Punkte offen sind.** Prod ist nicht
betroffen: der Branch ist nur gepusht. Der Scam-Judge laeuft aktuell ueber den
Env-Fix `FIREWORKS_MODEL=accounts/fireworks/models/deepseek-v4-flash-0731` in
`~/.config/deadlock/bots.env` (Backup: `bots.env.bak-fwmodel`).

## Vorgeschichte in zwei Saetzen

Fireworks hat `deepseek-v4-flash` am 15.08.2026 abgeschaltet, der Endpunkt
antwortete 404, und der Conversation-Scam-Judge fiel einen ganzen Tag lang
fail-safe auf `unsure` zurueck, ohne dass es jemandem auffiel. Der Resolver soll
den Modellnamen beim Anbieter aufloesen, statt ihn einzukompilieren.

## BLOCKING

### 1. `pick_newest` kann die tote Fassung waehlen

`rust/crates/tb-llm/src/model_resolver.rs`, `pick_newest`.

`a.created.cmp(&b.created)` auf `Option<i64>`: in Rust ist `None < Some(_)`,
also verliert jeder Eintrag ohne Zeitstempel gegen jeden mit. Enthaelt die
Liste das tote `deepseek-v4-flash` mit `created` und die neue Fassung ohne,
loest der Resolver auf das tote Modell auf und schreibt es per UPSERT als
letzten guten Stand in `llm_model_cache` — genau der Ausfall, den das Feature
verhindern soll, nur mit vergifteter Persistenz. Der Doc-Kommentar behauptet
bereits das richtige Verhalten, der Code haelt es nicht.

Zu tun: Eintraege ohne Zeitstempel duerfen nicht automatisch verlieren.
Sinnvoll ist, zuerst nach Name absteigend zu sortieren (die datierten Fassungen
sortieren lexikografisch richtig, weil das Datum als `MMTT` hinten steht) und
den Zeitstempel nur als Tiebreak zu nehmen, oder `None` wie „unbekannt, aber
nicht aelter" zu behandeln. Test mit gemischter Liste ergaenzen: eine Fassung
mit `created`, eine neuere ohne.

### 2. Transienter Netzfehler degradiert den ganzen Prozess

`model_resolver.rs`, `invalidate_and_refresh` und `refresh_fireworks`.

`invalidate()` laeuft vor dem Refresh, und `minimax_chat.rs` ruft mit
`pool = None` auf. Scheitert der Listen-Call transient (Timeout, 5xx — also
genau die Lage waehrend eines Anbieter-Deploys), ist `fallback_from_db(None)`
sofort `None`, die Prozess-Zelle bleibt leer, und jeder folgende Endpunkt faellt
bis zum naechsten Tageslauf (24 h) auf `FIREWORKS_DEFAULT_MODEL` zurueck. Der
neu gefundene Name wird wegen `pool = None` ausserdem nie persistiert.

Zu tun: Den alten Wert erst verwerfen, wenn ein neuer feststeht. Den `PgPool`
bis in den 404-Pfad durchreichen, damit der geheilte Name die DB erreicht.

### 3. Der 404-Zweig prueft den Anbieter nicht

`rust/crates/tb-engagement/src/minimax_chat.rs`, `post_completion_with_limit`.

Der Zweig ruft bedingungslos den Fireworks-Resolver. Mit beiden Keys und
`TB_LLM_PROVIDER_DEFAULT=minimax` laeuft der Client gegen MiniMax; ein
MiniMax-404 setzt dann einen `accounts/fireworks/models/…`-Namen in den Call
gegen die MiniMax-Adresse. Garantierter zweiter Fehlschlag, doppelter Traffic,
irrefuehrende Fehlermeldung, und ein gesetztes `ENGAGEMENT_MINIMAX_MODEL` wird
ueberschrieben.

Zu tun: Retry nur, wenn der Client tatsaechlich Fireworks bedient.

## Wichtig, auch wenn als NIT gemeldet

### 4. Die Selbstheilung sitzt am falschen Ort

Der Conversation-Scam-Judge, auf den sich der ganze Commit beruft, laeuft ueber
`rust/crates/tb-chat/src/scam_pitch.rs`, `call_judge` — dort wird ein 404
unveraendert in `JudgeCallError::Provider` gewandelt, ohne Retry. Der Judge
profitiert also nur vom Startlauf, nicht von der beworbenen Selbstheilung.

Zu tun: Selbstheilung auch im Judge-Pfad verankern.

### 5. Es fehlt jede Alarmierung

Kernaussage des Vorfalls war „einen Tag lang hat es niemand gemerkt". Ein
dauerhaft ausfallender Judge muss sichtbar werden (Discord-Alarm in `#bot-logs`
ueber den bestehenden Weg), statt still auf `unsure` zu stehen.

## Kleinkram

- Praefix-Filter ohne Allowlist: `…deepseek-v4-flash` matcht auch `-lite`,
  `-preview`, `-thinking`, sollte der Anbieter so etwas veroeffentlichen. Fuer
  einen Ban-Judge unbeaufsichtigt zu viel. Test deckt nur `-pro` ab.
- `fallback_from_db` liest das Alter, loggt es und ignoriert es sonst. Ein
  beliebig alter DB-Eintrag schlaegt den Default, auch nach einem frischen
  Deploy. `model_created` wird geschrieben und nie gelesen.
- Testabdeckung: `wiremock` ist in `tb-llm` bereits Dev-Dependency, aber weder
  die Rangfolge in `fireworks_model()` noch `invalidate_and_refresh` noch der
  404-Retry sind abgedeckt. Der einzige End-to-End-Test ist `#[ignore]` und
  feuert gegen die Produktiv-API.
- Kein Single-Flight: N gleichzeitige 404 loesen N parallele Listen-Calls aus.
- Der Dateistempel der Migration `20260816090000_llm_model_cache.sql` liegt
  einen Tag in der Zukunft. Reihenfolge stimmt, der Name luegt.

## Danach

1. `cargo build`, `cargo clippy`, `cargo test` (nur eigene Dateien formatieren,
   das Repo folgt nicht durchgaengig den rustfmt-Defaults).
2. Review-Gate erneut laufen lassen, und zwar ueber
   `Documents/.claude/gpt-workers/codex_gate_hook.py` (JSON auf stdin), nicht
   ueber `review_gate.py` direkt — nur der Hook faehrt die Stufen-Kette mit
   Fallback. Codex `gpt-5.6-sol` haengt bis 20.08.2026 im Usage-Limit, Stufe 2
   `claude-opus-5` traegt.
3. Merge nach `feature/vod-auto-save` tippt der Nutzer, Deploy laeuft ueber die
   Session, die den Live-Branch haelt.
4. Erst nach dem Deploy `FIREWORKS_MODEL` aus `~/.config/deadlock/bots.env`
   entfernen und den Dienst neu starten. Solange die Variable steht,
   ueberspringt der Resolver seine Arbeit — das ist Absicht, macht ihn hier
   aber wirkungslos.

## Unabhaengig davon offen

- `anthropic::tests::complete_ohne_key_unavailable` ist flaky: `keys.rs` setzt
  `ANTHROPIC_API_KEY`, `anthropic.rs` liest ihn, beide ohne das gemeinsame
  `ENV_LOCK`, das `selection.rs` benutzt. Etwa jeder vierte Volllauf faellt.
- Der hartcodierte Default in `tb-engagement/src/crew_review.rs` sollte auf
  denselben Resolver ziehen, statt einen zweiten Modellnamen zu fuehren.
