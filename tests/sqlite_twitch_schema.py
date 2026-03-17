from __future__ import annotations

import sqlite3


def ensure_sqlite_twitch_schema(conn: sqlite3.Connection) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_streamers (
            twitch_login TEXT PRIMARY KEY,
            twitch_user_id TEXT,
            require_discord_link INTEGER DEFAULT 0,
            next_link_check_at TEXT,
            discord_user_id TEXT,
            discord_display_name TEXT,
            is_on_discord INTEGER DEFAULT 0,
            manual_verified_permanent INTEGER DEFAULT 0,
            manual_verified_until TEXT,
            manual_verified_at TEXT,
            manual_partner_opt_out INTEGER DEFAULT 0,
            archived_at TEXT,
            raid_bot_enabled INTEGER DEFAULT 0,
            silent_ban INTEGER DEFAULT 0,
            silent_raid INTEGER DEFAULT 0,
            is_monitored_only INTEGER DEFAULT 0,
            live_ping_role_id TEXT,
            live_ping_enabled INTEGER DEFAULT 1,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_streamer_identities (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT NOT NULL,
            discord_user_id TEXT,
            discord_display_name TEXT,
            is_on_discord INTEGER DEFAULT 0,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_partners (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            twitch_user_id TEXT NOT NULL,
            twitch_login TEXT NOT NULL,
            require_discord_link INTEGER DEFAULT 0,
            last_description TEXT,
            last_link_ok INTEGER,
            added_by TEXT,
            last_link_checked_at TEXT,
            next_link_check_at TEXT,
            manual_verified_permanent INTEGER DEFAULT 0,
            manual_verified_until TEXT,
            manual_verified_at TEXT,
            manual_partner_opt_out INTEGER DEFAULT 0,
            raid_bot_enabled INTEGER DEFAULT 0,
            silent_ban INTEGER DEFAULT 0,
            silent_raid INTEGER DEFAULT 0,
            live_ping_role_id TEXT,
            live_ping_enabled INTEGER DEFAULT 1,
            partnered_at TEXT DEFAULT CURRENT_TIMESTAMP,
            departnered_at TEXT,
            status TEXT NOT NULL DEFAULT 'active'
        )
        """
    )
    conn.execute("DROP VIEW IF EXISTS twitch_streamers_partner_state")
    conn.execute(
        """
        CREATE VIEW twitch_streamers_partner_state AS
        SELECT
            p.twitch_login,
            p.twitch_user_id,
            p.require_discord_link,
            p.next_link_check_at,
            i.discord_user_id,
            i.discord_display_name,
            COALESCE(i.is_on_discord, 0) AS is_on_discord,
            p.manual_verified_permanent,
            p.manual_verified_until,
            p.manual_verified_at,
            p.manual_partner_opt_out,
            p.partnered_at AS created_at,
            CASE WHEN p.status = 'active' THEN NULL ELSE p.departnered_at END AS archived_at,
            p.raid_bot_enabled,
            p.silent_ban,
            p.silent_raid,
            0 AS is_monitored_only,
            CASE
                WHEN (
                    COALESCE(p.manual_verified_permanent, 0) = 1
                    OR p.manual_verified_at IS NOT NULL
                    OR (
                        p.manual_verified_until IS NOT NULL
                        AND datetime(p.manual_verified_until) >= datetime('now')
                    )
                ) THEN 1 ELSE 0
            END AS is_verified,
            1 AS is_partner,
            CASE WHEN p.status = 'active' THEN 1 ELSE 0 END AS is_partner_active,
            p.live_ping_role_id,
            COALESCE(p.live_ping_enabled, 1) AS live_ping_enabled
        FROM twitch_partners p
        LEFT JOIN twitch_streamer_identities i
          ON i.twitch_user_id = p.twitch_user_id
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_live_state (
            twitch_user_id TEXT PRIMARY KEY,
            streamer_login TEXT NOT NULL,
            last_stream_id TEXT,
            last_started_at TEXT,
            last_title TEXT,
            last_game_id TEXT,
            last_discord_message_id TEXT,
            last_discord_message_url TEXT,
            is_live INTEGER DEFAULT 0,
            last_game TEXT,
            last_viewer_count INTEGER DEFAULT 0,
            last_follower_count INTEGER DEFAULT 0,
            had_deadlock_in_session INTEGER DEFAULT 0,
            last_deadlock_seen_at TEXT,
            active_session_id INTEGER
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            streamer_login TEXT NOT NULL,
            stream_id TEXT,
            started_at TEXT,
            ended_at TEXT,
            duration_seconds INTEGER DEFAULT 0
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            access_token_enc TEXT,
            refresh_token_enc TEXT,
            enc_version INTEGER DEFAULT 1,
            token_expires_at TEXT,
            scopes TEXT,
            raid_enabled INTEGER DEFAULT 0,
            authorized_at TEXT DEFAULT CURRENT_TIMESTAMP,
            needs_reauth INTEGER DEFAULT 0,
            reauth_notified_at TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_raid_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_broadcaster_id TEXT,
            from_broadcaster_login TEXT,
            to_broadcaster_id TEXT,
            to_broadcaster_login TEXT,
            executed_at TEXT,
            viewer_count INTEGER DEFAULT 0,
            success INTEGER DEFAULT 0,
            reason TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_raid_arrival_tracking (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            detected_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            last_signal_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            from_broadcaster_id TEXT,
            from_broadcaster_login TEXT NOT NULL,
            to_broadcaster_id TEXT NOT NULL,
            to_broadcaster_login TEXT NOT NULL,
            viewer_count INTEGER DEFAULT 0,
            classification TEXT NOT NULL,
            confirmation_signals TEXT DEFAULT '',
            primary_signal TEXT,
            correlation_status TEXT,
            correlation_detail TEXT,
            source_resolution TEXT,
            raid_history_id INTEGER,
            raid_history_executed_at TEXT,
            unraid_seen INTEGER DEFAULT 0,
            last_unraid_at TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_raid_blacklist (
            target_id TEXT,
            target_login TEXT PRIMARY KEY,
            reason TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS streamer_plans (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            plan_name TEXT,
            raid_boost_enabled INTEGER DEFAULT 0,
            manual_plan_id TEXT,
            manual_plan_expires_at TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_partner_raid_scores (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT NOT NULL,
            avg_duration_sec INTEGER DEFAULT 0,
            time_pattern_score_base REAL DEFAULT 0.5,
            received_successful_raids_total INTEGER DEFAULT 0,
            is_new_partner_preferred INTEGER DEFAULT 1,
            new_partner_multiplier REAL DEFAULT 1.0,
            raid_boost_multiplier REAL DEFAULT 1.0,
            is_live INTEGER DEFAULT 0,
            current_started_at TEXT,
            current_uptime_sec INTEGER DEFAULT 0,
            duration_score REAL DEFAULT 0.5,
            time_pattern_score REAL DEFAULT 0.5,
            base_score REAL DEFAULT 0.5,
            final_score REAL DEFAULT 0.5,
            today_received_raids INTEGER DEFAULT 0,
            last_computed_at TEXT DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_partner_raid_score_tracking (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            raid_history_id INTEGER,
            raid_history_executed_at TEXT,
            from_broadcaster_id TEXT,
            from_broadcaster_login TEXT,
            to_broadcaster_id TEXT,
            to_broadcaster_login TEXT,
            viewer_count INTEGER DEFAULT 0,
            confirmed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            target_session_id INTEGER,
            target_stream_started_at TEXT,
            score_last_computed_at TEXT,
            final_score REAL DEFAULT 0.0,
            base_score REAL DEFAULT 0.0,
            duration_score REAL DEFAULT 0.5,
            time_pattern_score REAL DEFAULT 0.5,
            new_partner_multiplier REAL DEFAULT 1.0,
            raid_boost_multiplier REAL DEFAULT 1.0,
            today_received_raids INTEGER DEFAULT 0,
            was_deadlock_at_raid INTEGER DEFAULT 0,
            deadlock_continued_until TEXT,
            deadlock_continued_sec INTEGER,
            resolved_at TEXT,
            resolution_reason TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_channel_updates (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            twitch_user_id TEXT NOT NULL,
            game_name TEXT,
            recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )
        """
    )
