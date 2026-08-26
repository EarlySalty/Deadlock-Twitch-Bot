-- Ring-Tabelle des Event-Busses fuer die eigenen OBS-Docks.
--
-- Der Bot (tb-bot) schreibt hier jedes Ereignis hinein, das ein Dock sehen
-- soll, und meldet die neue Zeile ueber `pg_notify('obs_dock', ...)`. Das
-- Gateway (tb-dashboard-api) haengt mit LISTEN an demselben Kanal und holt den
-- eigentlichen Inhalt aus dieser Tabelle. Die NOTIFY-Nutzlast traegt deshalb
-- absichtlich nur `{"channel_id":"<id>","id":<id>}`: NOTIFY ist auf 8000 Byte
-- begrenzt, eine ganze Chatnachricht mit Emote-Fragmenten passt dort nicht
-- verlaesslich hinein.
--
-- Die Tabelle ist kein Archiv, sondern ein kurzer Nachlaufpuffer: ein Dock, das
-- nach einem OBS-Neustart neu verbindet, will die letzten Minuten sehen und
-- sonst nichts. Aufbewahrt wird alles, was juenger als 15 Minuten ist; der Bot
-- raeumt im Abstand von 5 Minuten auf (`cleanup_obs_dock_events`,
-- bin/tb-bot/src/obs_dock.rs), eine Zeile kann also im schlechtesten Fall bis
-- zu 20 Minuten liegen bleiben. Wer laengere Zeitreihen braucht, nimmt die
-- vorhandenen Telemetrie-Tabellen, nicht diese hier.
--
-- Bewusst ohne Schema-Praefix, damit derselbe Text in einem Testschema
-- (`search_path`) ausgefuehrt werden kann und Test und Produktion garantiert
-- dasselbe DDL sehen.
CREATE TABLE IF NOT EXISTS obs_dock_events (
    id BIGSERIAL PRIMARY KEY,
    channel_id TEXT NOT NULL,
    payload JSONB NOT NULL,
    dedupe_key TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Einzige Leseform des Gateways: "alles fuer diesen Kanal ab Lauf-ID X".
CREATE INDEX IF NOT EXISTS obs_dock_events_channel_id_id_idx
    ON obs_dock_events (channel_id, id);

-- Ein Vorkommnis, eine Zeile. Twitch meldet dasselbe Vorkommnis teilweise ueber
-- zwei Wege (ein eingehender Raid kommt als `channel.raid` UND als
-- `channel.chat.notification` mit notice_type=raid), und beide Wege haben
-- Luecken, also darf keiner der beiden wegfallen. Zusammengelegt werden sie
-- hier: der Schreiber bildet einen vorkommnis-stabilen Schluessel und laesst
-- die zweite Meldung an diesem Index auflaufen (ON CONFLICT DO NOTHING).
-- Teilindex, weil `PlatformEvent::Info` bewusst keinen Schluessel hat.
CREATE UNIQUE INDEX IF NOT EXISTS obs_dock_events_dedupe_key_idx
    ON obs_dock_events (dedupe_key)
    WHERE dedupe_key IS NOT NULL;

COMMENT ON TABLE obs_dock_events IS
    'Kurzer Nachlaufpuffer des OBS-Dock-Busses (Aufbewahrung 15 Minuten, Aufraeumtakt 5 Minuten, im schlechtesten Fall also bis zu 20 Minuten). Schreiber: tb-bot, Leser: tb-dashboard-api ueber LISTEN obs_dock.';
COMMENT ON COLUMN obs_dock_events.channel_id IS
    'Kanal, an den das Ereignis gehoert, bei Twitch die numerische Broadcaster-ID. Der Leser bindet daran seine Berechtigungspruefung.';
COMMENT ON COLUMN obs_dock_events.payload IS
    'Ein PlatformEvent (tb-platform-core) als JSON, intern getaggt mit dem Feld "typ". Eingefroren. Das WebSocket-Drahtformat ist nicht diese Spalte selbst, sondern eine Huelle {"id":<zeilen-id>,"ereignis":<payload>}: das Dock braucht die id, um nach einem Neuverbinden mit ?seit= dort weiterzulesen, wo es aufgehoert hat.';
COMMENT ON COLUMN obs_dock_events.dedupe_key IS
    'Vorkommnis-stabiler Schluessel aus dem Ereignis selbst (PlatformEvent::dedupe_key). Eindeutig ueber den Teilindex, damit zwei Twitch-Signale fuer dasselbe Vorkommnis genau eine Zeile ergeben. NULL nur bei Ereignissen ohne Schluessel.';
