# Paket 1a: gemeinsame bereits typisierte Werte

Keine neuen Defaults oder Modellregeln. Fachverbraucher erhalten die beim Start validierte Momentaufnahme; fehlt sie, wird ein Fehler weitergereicht und kein zweiter Default-/ENV-Pfad geöffnet.

| Bisherige Quelle/Verbraucher | Neuer Anschluss | Semantik |
|---|---|---|
| MASTER_BROKER_BASE_URL/HOST/PORT, Internal-API self_explainer_log | broker.base_url | bestehendes Ziel, trailing slash entfernt; Tokenkette unverändert |
| MASTER_BROKER_BASE_URL/HOST/PORT, Dashboard community/sources | broker.base_url | bestehendes Ziel, fehlende Laufzeit bleibt sichtbar unavailable |
| TWITCH_TARGET_GAME_NAME, Internal-API streamers und MCP list_partners | twitch.target_game | bisheriger Default Deadlock bleibt im zentralen Schema, trim am bisherigen Fachpfad |
| TWITCH_BOT_USER_ID, oauth_followups | twitch.bot_user_id | bereits beim Start geprüfte identische Bot-ID; kein stiller ENV-Zweitwert |
| TWITCH_NOTIFY_CHANNEL_ID, Chat-Invite-Resolver | twitch.notify_channel_id, explizit im ChatRuntimePorts übergeben | dieselbe bereits geprüfte Ankündigungs-ID, kein zweiter Leser |

Separater interner Client-URL-Override und erlaubte Remote-Health-Verbindung sind noch nicht Teil dieses kleinen Pakets. Sie benötigen eigene typisierte Felder; Listener-Adresse und Client-Ziel werden nicht still zusammengelegt. Ebenso bleiben Stream-Audit-Standalone und Telemetrie-Regeln für Folgepakete offen.

Prüfung Paket 1a: Bot-/Dashboard-Debugcheck erfolgreich, Runtime-Test bestätigt fehlenden Snapshot als Fehler und unveränderliche installierte Werte. Bestehende Self-Explainer-Log-Suite: 14 Tests grün. Bestehender tb-llm-Wächter `no_minimax_identifiers`: grün. LLM-Client/Modelle/Reasoning wurden nicht geändert. Einzige Warnung im API-Test ist die vorhandene ungenutzte Variable in session_detail.rs:702.
