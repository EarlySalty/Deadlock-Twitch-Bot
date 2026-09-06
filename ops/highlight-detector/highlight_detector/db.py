import psycopg


class TabelleFehlt(Exception):
    pass


def verbinde(dsn, dbname="twitch_analytics"):
    return psycopg.connect(dsn, dbname=dbname, autocommit=True)


def aufloesen_streamer_id(conn, login):
    with conn.cursor() as cur:
        cur.execute(
            "SELECT twitch_user_id FROM twitch_streamers WHERE twitch_login = %s",
            (login.lower(),),
        )
        row = cur.fetchone()
        return row[0] if row else None


def korpus_clips(conn, limit, nur_deadlock=True):
    with conn.cursor() as cur:
        cur.execute(
            """
            SELECT clip_id, clip_url, coalesce(view_count,0), coalesce(duration_seconds,0),
                   streamer_login, twitch_user_id, coalesce(game_name,'')
              FROM twitch_clips_social_media
             WHERE clip_url IS NOT NULL
               AND (%s = false OR lower(coalesce(game_name,'')) LIKE '%%deadlock%%'
                    OR game_name IS NULL)
             ORDER BY view_count DESC NULLS LAST
             LIMIT %s
            """,
            (nur_deadlock, limit),
        )
        return [
            {
                "clip_id": r[0], "clip_url": r[1], "views": r[2], "dauer_s": float(r[3]),
                "streamer_login": r[4], "streamer_twitch_id": r[5], "game_name": r[6],
            }
            for r in cur.fetchall()
        ]


def echte_clips(conn, streamer_twitch_id):
    with conn.cursor() as cur:
        cur.execute(
            """
            SELECT clip_id, clip_url,
                   extract(epoch from created_at::timestamptz)::bigint,
                   coalesce(duration_seconds,0), coalesce(view_count,0), clip_title
              FROM twitch_clips_social_media
             WHERE twitch_user_id = %s
             ORDER BY view_count DESC NULLS LAST
            """,
            (streamer_twitch_id,),
        )
        return [
            {
                "clip_id": r[0], "clip_url": r[1], "created_epoch": r[2],
                "dauer_s": float(r[3]), "views": r[4], "titel": r[5],
            }
            for r in cur.fetchall()
        ]


def schreibe_merkmale(conn, zeilen):
    try:
        with conn.cursor() as cur:
            cur.executemany(
                """
                INSERT INTO twitch_clip_merkmale
                    (clip_id, streamer_twitch_id, views, dauer_s, ersteller,
                     signal, offset_vom_ende_s, wert)
                VALUES (%(clip_id)s, %(streamer_twitch_id)s, %(views)s, %(dauer_s)s,
                        %(ersteller)s, %(signal)s, %(offset_vom_ende_s)s, %(wert)s)
                """,
                zeilen,
            )
    except psycopg.errors.UndefinedTable as e:
        raise TabelleFehlt("twitch_clip_merkmale") from e


def schreibe_highlights(conn, zeilen):
    import json

    try:
        with conn.cursor() as cur:
            for z in zeilen:
                cur.execute(
                    """
                    INSERT INTO twitch_vod_highlights
                        (streamer_twitch_id, vod_id, start_s, end_s, score, signale, status)
                    VALUES (%s, %s, %s, %s, %s, %s::jsonb, %s)
                    ON CONFLICT (vod_id, start_s, end_s) DO NOTHING
                    """,
                    (
                        z["streamer_twitch_id"], z["vod_id"], z["start_s"], z["end_s"],
                        z["score"], json.dumps(z["signale"]), z.get("status", "neu"),
                    ),
                )
    except psycopg.errors.UndefinedTable as e:
        raise TabelleFehlt("twitch_vod_highlights") from e
