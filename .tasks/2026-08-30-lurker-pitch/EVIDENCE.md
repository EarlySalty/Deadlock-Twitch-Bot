status: aktiv
Datum: 2026-08-30

# Evidence: Analogien, Abstraktionen, Schnittstellen

1. `rust/crates/tb-chat/src/lfg_pitch.rs:492-530` — decide()-Kette mit Silent-Gründen;
   Anknüpfung: Kanal-Gate als weitere Silent-Stufe `ForeignChannel` nach dem enabled-Check,
   Muster `SilentReason::as_str` (:300) folgen.
2. `rust/crates/tb-chat/src/lfg_pitch.rs:425-470` — Konstruktor `new`/`new_with_clock` mit
   Parameterliste; Anknüpfung: `own_channel_broadcaster_id: String` als zusätzlicher
   Parameter, beide Konstruktionsstellen (chat_wiring.rs:834, :2828) und Test-Helfer
   (pipeline.rs:2485, :2722, lfg_pitch.rs:1021) anpassen.
3. `rust/crates/tb-chat/src/promos.rs:1679-1826` — `maybe_send_lurker_tax_reminder` als
   Wirts-Funktion; Anknüpfung: Pitch-Zweig nach der Kandidatenauswahl (:1770-1793), gleiches
   Sende- und Merk-Muster (send_announcement "orange", nur bei Erfolg merken, :1798-1815).
4. `rust/crates/tb-chat/src/promos.rs:1852-1935` — `get_lurker_tax_candidates`; Anknüpfung:
   wird unverändert wiederverwendet, Identity-Key-Bildung (`id:`/`login:`) übernommen für
   den Pitch-Log-Primärschlüssel.
5. `rust/crates/tb-chat/src/promos.rs:1937-1950` — `build_lurker_tax_text`; Anknüpfung:
   `build_lurker_pitch_text` analog, mit `{invite}`-Platzhalter.
6. `rust/crates/tb-dashboard-api/src/handlers/lurker_tax_settings.rs:37-47,110,159` —
   GET/POST-Muster mit `resolve_target`; Anknüpfung: zweites Flag `lurker_pitch_enabled`
   im selben Request/Response, kein neuer Endpoint (Bestands-Späher: konsolidieren).
7. `rust/crates/tb-chat/src/commands.rs:537-538,1434-1447` — `cmd_lurkersteuer_off`;
   INV-5: unangetastet.
8. `rust/migrations/20260829090000_twitch_scout_candidates.sql` — additives
   Migrationsmuster (CREATE TABLE IF NOT EXISTS, Spalten-Additive); Anknüpfung: neue
   Migration in gleicher Konvention.
9. `rust/crates/tb-chat/src/promos.rs:283-289` — Konstanten-Schwellen; Anknüpfung:
   Pitch-Konstanten daneben, keine Magie im SQL.
10. `rust/crates/tb-chat/src/api.rs:101` — `ChatApi::bot_user_id()`; Anknüpfung:
    Eigenkanal-Erkennung ohne ENV/Config: Broadcaster-ID == Bot-Account-ID.
