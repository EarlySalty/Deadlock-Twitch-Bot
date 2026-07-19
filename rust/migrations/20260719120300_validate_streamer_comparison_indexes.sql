-- Ein abgebrochener Concurrent-Build darf nicht unbemerkt als erfolgreiche
-- Migration gelten. Der nächste Start bleibt dann sichtbar rot.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM (
            VALUES
                ('idx_twitch_sessions_login_lower_window'),
                ('idx_twitch_raid_retention_target_lower_executed')
        ) AS expected(index_name)
        LEFT JOIN pg_class index_relation
          ON index_relation.oid = to_regclass('public.' || expected.index_name)
        LEFT JOIN pg_index index_state
          ON index_state.indexrelid = index_relation.oid
        WHERE index_relation.oid IS NULL
           OR index_state.indisvalid IS DISTINCT FROM TRUE
    ) THEN
        RAISE EXCEPTION 'Streamer-Vergleichsindex fehlt oder ist INVALID';
    END IF;
END
$$;
