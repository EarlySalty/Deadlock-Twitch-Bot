# Vertrag

## Persistenz

Neue Felder: `refresh_expires_at TIMESTAMPTZ`, `needs_reauth BOOLEAN NOT NULL DEFAULT FALSE`, `reauth_required_at TIMESTAMPTZ`, `reauth_notified_at TIMESTAMPTZ`. Das erste Vorfallsdatum bleibt beim Übergang von bald ablaufend zu endgültig ungültig erhalten. Ein erfolgreicher Callback löscht den Vorfall. Ein erfolgreicher Refresh kann einen bloßen Ablaufhinweis auflösen, nicht einen endgültig ungültigen Zugang.

Ein Worker-Ergebnis schreibt nur, wenn die verschlüsselte Access-Zeile noch dem gelesenen Stand entspricht. Damit überschreibt weder ein später Refresh noch ein später Fehler das zwischenzeitliche Neu-Verbinden. Die AAD-Bindung bleibt identisch.

## Status

Der bestehende Status-Endpunkt bleibt kompatibel und ergänzt `refresh_expires_at`, `needs_reauth`, `reauth_soon` und `automatically_renewed`. `expired` beschreibt einen endgültig ungültigen Zugang oder das tatsächliche Verbindungsende. Ein kurzfristig abgelaufenes, erneuerbares Access-Recht löst keinen falschen Neu-Verbinden-Hinweis aus.

## Benachrichtigung

Der Worker gibt Twitch-User-ID, Plattform und Zustand an einen Port. Die Bot-Komposition nutzt `discord_user_id_for` ohne Login-Fallback und `BrokerTokenLifecycleNotifier.send_user_dm`. Eine Zeilensperre verhindert parallele erfolgreiche Sendungen; ein persistierter Versandzeitpunkt verhindert Wiederholungen nach Neustarts. Der vorhandene Broker-Port verwendet bereits einen deterministischen Idempotenzschlüssel für denselben Empfänger und Inhalt. Die Integration prüft zusätzlich das Erfolgsfeld und die Nachrichten-ID der Antwort. Fehlende Discord-Zuordnung oder ein erfolgloser Versand bleiben zur Nachholung offen.

Der Inhalt bleibt während desselben Vorfalls unverändert, auch wenn der Zugang später endgültig abläuft. Der Dashboard-Link enthält den Vorfallszeitpunkt als Hinweiskennung. Dadurch erhält ein neuer Vorfall einen neuen Broker-Schlüssel, Wiederholungen desselben Vorfalls behalten ihren Schlüssel.

Text: „Deine Verbindung zu TikTok muss erneuert werden. Bitte verbinde TikTok im Social-Media-Dashboard neu: https://deutsche-deadlock-community.de/social-media-admin?hinweis=…“. Plattformen werden aus einer festen Liste benannt.
