use super::{Field, Group, Kind};

const ID_MAX: i64 = i64::MAX;
const U32_MAX: i64 = u32::MAX as i64;
const PORT_MAX: i64 = 65_535;

pub(super) const GROUPS: &[Group] = &[
    Group {
        id: "twitch",
        label: "Twitch",
    },
    Group {
        id: "bot",
        label: "Bot-Betrieb",
    },
    Group {
        id: "brain",
        label: "Brain-Anbindung",
    },
    Group {
        id: "vod_archive",
        label: "VOD-Archiv",
    },
    Group {
        id: "discord",
        label: "Discord",
    },
    Group {
        id: "reliability",
        label: "Monitoring und Zuverlässigkeit",
    },
    Group {
        id: "services",
        label: "Interne Dienste",
    },
    Group {
        id: "dashboard",
        label: "Dashboard",
    },
    Group {
        id: "affiliate",
        label: "Partnerprogramm",
    },
    Group {
        id: "stt",
        label: "Spracherkennung",
    },
    Group {
        id: "media",
        label: "Medien und Wissen",
    },
    Group {
        id: "challenges",
        label: "Challenges",
    },
    Group {
        id: "social_ai_provider",
        label: "Social AI: Anbieter und Preise",
    },
    Group {
        id: "social_ai_runtime",
        label: "Social AI: Ablauf",
    },
    Group {
        id: "social_ai_resources",
        label: "Social AI: Medienressourcen",
    },
    Group {
        id: "social_ai_plan",
        label: "Social AI: Schnittplan",
    },
    Group {
        id: "social_ai_output",
        label: "Social AI: Vorschau, Texte und Überarbeitungen",
    },
    Group {
        id: "social_ai_stt",
        label: "Social AI: Spracherkennung",
    },
    Group {
        id: "social_ai_brain",
        label: "Social AI: Brain-Kontext",
    },
];

struct Builder {
    fields: Vec<Field>,
    group: &'static str,
    nullable: bool,
}

impl Builder {
    fn group(&mut self, group: &'static str, nullable: bool) -> &mut Self {
        self.group = group;
        self.nullable = nullable;
        self
    }

    fn push(
        &mut self,
        path: &'static str,
        label: &'static str,
        help: &'static str,
        kind: Kind,
    ) -> &mut Field {
        self.fields.push(Field {
            path,
            group: self.group,
            label,
            help,
            kind,
            nullable: self.nullable,
            restart_required: true,
            min: None,
            max: None,
            choices: &[],
            min_items: None,
            max_items: None,
        });
        let last = self.fields.len() - 1;
        &mut self.fields[last]
    }

    fn flag(&mut self, path: &'static str, label: &'static str, help: &'static str) -> &mut Self {
        self.push(path, label, help, Kind::Boolean);
        self
    }

    fn int(
        &mut self,
        path: &'static str,
        label: &'static str,
        help: &'static str,
        min: i64,
        max: i64,
    ) -> &mut Self {
        let field = self.push(path, label, help, Kind::Integer);
        field.min = Some(min.to_string());
        field.max = Some(max.to_string());
        self
    }

    fn opt_int(
        &mut self,
        path: &'static str,
        label: &'static str,
        help: &'static str,
        min: i64,
        max: i64,
    ) -> &mut Self {
        self.int(path, label, help, min, max).optional()
    }

    fn number(
        &mut self,
        path: &'static str,
        label: &'static str,
        help: &'static str,
        min: f64,
        max: Option<f64>,
    ) -> &mut Self {
        let field = self.push(path, label, help, Kind::Number);
        field.min = Some(min.to_string());
        field.max = max.map(|max| max.to_string());
        self
    }

    fn text(&mut self, path: &'static str, label: &'static str, help: &'static str) -> &mut Self {
        self.push(path, label, help, Kind::String);
        self
    }

    fn opt_text(
        &mut self,
        path: &'static str,
        label: &'static str,
        help: &'static str,
    ) -> &mut Self {
        self.text(path, label, help).optional()
    }

    fn choice(
        &mut self,
        path: &'static str,
        label: &'static str,
        help: &'static str,
        choices: &'static [&'static str],
    ) -> &mut Self {
        self.push(path, label, help, Kind::Choice).choices = choices;
        self
    }

    fn texts(
        &mut self,
        path: &'static str,
        label: &'static str,
        help: &'static str,
        items: (usize, usize),
    ) -> &mut Self {
        let field = self.push(path, label, help, Kind::StringList);
        field.min_items = Some(items.0);
        field.max_items = Some(items.1);
        self
    }

    fn ints(
        &mut self,
        path: &'static str,
        label: &'static str,
        help: &'static str,
        min: i64,
        max: i64,
        items: (usize, usize),
    ) -> &mut Self {
        let field = self.push(path, label, help, Kind::IntegerList);
        field.min = Some(min.to_string());
        field.max = Some(max.to_string());
        field.min_items = Some(items.0);
        field.max_items = Some(items.1);
        self
    }

    fn optional(&mut self) -> &mut Self {
        if let Some(field) = self.fields.last_mut() {
            field.nullable = true;
        }
        self
    }
}

pub(super) fn catalog() -> Vec<Field> {
    let mut b = Builder {
        fields: Vec::new(),
        group: "",
        nullable: true,
    };
    twitch(&mut b);
    bot(&mut b);
    brain(&mut b);
    vod_archive(&mut b);
    discord(&mut b);
    reliability(&mut b);
    services(&mut b);
    dashboard(&mut b);
    affiliate(&mut b);
    stt(&mut b);
    media(&mut b);
    challenges(&mut b);
    social_ai_provider(&mut b);
    social_ai_runtime(&mut b);
    social_ai_resources(&mut b);
    social_ai_plan(&mut b);
    social_ai_output(&mut b);
    social_ai_stt(&mut b);
    social_ai_brain(&mut b);
    b.fields
}

fn twitch(b: &mut Builder) {
    b.group("twitch", false)
        .text(
            "twitch.bot_user_id",
            "Bot-Konto-ID",
            "Numerische Twitch-ID des Bot-Kontos.",
        )
        .text(
            "twitch.notify_channel_id",
            "Benachrichtigungskanal-ID",
            "Numerische Twitch-ID des Kanals für Bot-Hinweise.",
        )
        .text(
            "twitch.eventsub_callback_url",
            "EventSub-Callback-Adresse",
            "Öffentliche HTTPS-Adresse, an die Twitch Ereignisse sendet.",
        )
        .text(
            "twitch.target_game",
            "Zielspiel",
            "Spielkategorie, die der Bot beobachtet.",
        )
        .optional()
        .texts(
            "twitch.language_filters",
            "Sprachfilter",
            "Stream-Sprachen in Kleinbuchstaben, zum Beispiel de oder de-at.",
            (1, 16),
        )
        .optional();
}

fn bot(b: &mut Builder) {
    b.group("bot", true)
        .text(
            "bot.runtime_role",
            "Laufzeitrolle",
            "Rolle dieses Bot-Prozesses für die Laufzeitsperre.",
        )
        .flag(
            "bot.runtime_enforce",
            "Laufzeitsperre erzwingen",
            "Verhindert, dass zwei Prozesse dieselbe Rolle übernehmen.",
        )
        .int(
            "bot.legacy_internal_api_port",
            "Port der alten internen API",
            "Nur für den Übergang; leer lassen, wenn nicht gebraucht.",
            1,
            PORT_MAX,
        )
        .flag(
            "bot.run_database_migrations",
            "Datenbankmigrationen beim Start",
            "Führt ausstehende Migrationen beim Bot-Start aus.",
        )
        .flag(
            "bot.highlight_clipper_enabled",
            "Highlight-Clipper",
            "Schneidet automatisch Highlights aus Streams.",
        )
        .flag(
            "bot.monitoring_poll_enabled",
            "Monitoring-Abfrage",
            "Fragt Stream-Status regelmäßig zusätzlich zu EventSub ab.",
        )
        .flag(
            "bot.clip_fetcher_enabled",
            "Clip-Abruf",
            "Lädt neue Twitch-Clips der Partner.",
        )
        .int(
            "bot.eventsub_receiver_port",
            "EventSub-Empfangsport",
            "Lokaler Port für eingehende EventSub-Nachrichten.",
            1,
            PORT_MAX,
        )
        .int(
            "bot.scam_guard_discord_channel_id",
            "Scam-Schutz-Kanal",
            "Discord-Kanal für Scam-Warnungen.",
            1,
            ID_MAX,
        )
        .int(
            "bot.live_ping_guild_id",
            "Live-Ping-Server",
            "Discord-Server, in dem Live-Hinweise erscheinen.",
            1,
            ID_MAX,
        )
        .text(
            "bot.alert_mention",
            "Alarm-Erwähnung",
            "Erwähnung, die bei Betriebsalarmen vorangestellt wird.",
        )
        .text(
            "bot.discord_ref_code",
            "Discord-Empfehlungscode",
            "Code für Discord-Einladungslinks im Chat.",
        )
        .flag(
            "bot.chat_enabled",
            "Chat-Bot",
            "Schaltet den Twitch-Chat-Bot ein.",
        )
        .flag(
            "bot.chat_persist_all_games",
            "Chat aller Spiele speichern",
            "Speichert Chatnachrichten auch außerhalb von Deadlock-Streams.",
        )
        .int(
            "bot.community_points_aggregation_interval_seconds",
            "Takt der Community-Punkte",
            "Sekunden zwischen zwei Berechnungen der Tageswerte.",
            1,
            3_600,
        )
        .flag(
            "bot.lfg_pitch_enabled",
            "LFG-Hinweise",
            "Weist im Chat auf die Mitspielersuche hin.",
        )
        .flag(
            "bot.impersonation_guard_enabled",
            "Schutz vor Nachahmern",
            "Timeoutet Nachahmer der Community in Partner-Chats und warnt in fremden Chats vor ihnen.",
        )
        .flag(
            "bot.golive_tips_enabled",
            "Go-Live-Tipps",
            "Sendet Streamern Tipps beim Stream-Start.",
        )
        .flag(
            "bot.irc_lurker_enabled",
            "IRC-Lurker",
            "Liest Chats ohne eigene Nachrichten mit.",
        )
        .flag(
            "bot.outreach_shadow_enabled",
            "Outreach im Schattenmodus",
            "Bewertet Outreach-Kandidaten, ohne Nachrichten zu senden.",
        )
        .flag(
            "bot.smalltalk_loop_enabled",
            "Smalltalk-Schleife",
            "Erzeugt Smalltalk-Vorschläge für den Chat.",
        )
        .flag(
            "bot.smalltalk_loop_live_send",
            "Smalltalk live senden",
            "Sendet Smalltalk-Vorschläge wirklich in den Chat.",
        )
        .int(
            "bot.auto_raid_offline_grace_seconds",
            "Auto-Raid-Wartezeit",
            "Sekunden nach Stream-Ende bis zum automatischen Raid.",
            0,
            ID_MAX,
        )
        .number(
            "bot.auto_unraid_window_seconds",
            "Auto-Unraid-Fenster",
            "Sekunden, in denen ein Raid bei Rückkehr der Quelle abgebrochen wird.",
            0.0,
            None,
        )
        .number(
            "bot.flip_repeat_window_seconds",
            "Flip-Wiederholungsfenster",
            "Sekunden, in denen ein zweiter Flip als Wiederholung zählt.",
            0.0,
            None,
        )
        .number(
            "bot.flip_pause_seconds",
            "Flip-Pause",
            "Sekunden, die Auto-Raids nach wiederholtem Flip pausieren.",
            0.0,
            None,
        )
        .int(
            "bot.mcp_port",
            "MCP-Port",
            "Lokaler Port des MCP-Servers.",
            1,
            PORT_MAX,
        )
        .text(
            "bot.raid_redirect_uri",
            "Raid-OAuth-Weiterleitung",
            "Rückkehradresse nach der Raid-Freigabe bei Twitch.",
        )
        .text(
            "bot.clip_raid_redirect_uri",
            "Clip-Raid-OAuth-Weiterleitung",
            "Rückkehradresse für Clip-Raids; leer für keine.",
        )
        .opt_text(
            "bot.legacy_proxy_base_url",
            "Alte Proxy-Adresse",
            "Basisadresse des alten Proxys für den Übergang.",
        )
        .text(
            "bot.legacy_greeter_base_url",
            "Alte Begrüßungsadresse",
            "Basisadresse des alten Begrüßungsdienstes.",
        )
        .text(
            "bot.chat_review_log_directory",
            "Ordner für Chat-Prüfprotokolle",
            "Pfad relativ zur Konfigurationsdatei oder absolut.",
        )
        .text(
            "bot.service_warning_log_directory",
            "Ordner für Dienstwarnungen",
            "Pfad relativ zur Konfigurationsdatei oder absolut.",
        )
        .text(
            "bot.yt_dlp_binary",
            "yt-dlp-Programm",
            "Pfad zum yt-dlp-Programm.",
        )
        .text(
            "bot.outreach_yt_dlp_binary",
            "yt-dlp für Outreach",
            "Eigener yt-dlp-Pfad für Outreach.",
        )
        .text(
            "bot.vod_export_remote_base",
            "VOD-Exportziel",
            "rclone-Ziel für exportierte VODs.",
        );
}

fn brain(b: &mut Builder) {
    const MODES: &[&str] = &["legacy", "shadow", "typed"];
    b.group("brain", true)
        .choice(
            "bot.brain_client.mode",
            "Bot: Brain-Modus",
            "legacy nutzt den alten Weg, shadow vergleicht nur, typed nutzt das Brain.",
            MODES,
        )
        .opt_text(
            "bot.brain_client.endpoint",
            "Bot: Brain-Adresse",
            "Lokale Adresse des Brain-Dienstes.",
        )
        .texts(
            "bot.brain_client.public_scopes",
            "Bot: Brain-Bereiche",
            "Freigegebene Wissensbereiche, zum Beispiel bot.public.",
            (0, 32),
        )
        .opt_int(
            "bot.brain_client.timeout_ms",
            "Bot: Brain-Zeitlimit",
            "Millisekunden bis zum Abbruch einer Brain-Anfrage.",
            1,
            65_000,
        )
        .flag(
            "bot.brain_client.title_ai_via_brain",
            "Bot: Titel über das Brain",
            "Erzeugt Stream-Titel über das Brain.",
        )
        .flag(
            "bot.brain_chat.enabled",
            "Brain im Chat",
            "Beantwortet Chatfragen über das Brain.",
        )
        .int(
            "bot.brain_chat.user_cooldown_seconds",
            "Abklingzeit je Nutzer",
            "Sekunden zwischen zwei Brain-Fragen derselben Person.",
            1,
            3_600,
        )
        .int(
            "bot.brain_chat.channel_hourly_limit",
            "Stundenlimit je Kanal",
            "Höchstzahl an Brain-Antworten pro Kanal und Stunde.",
            1,
            500,
        )
        .int(
            "bot.brain_chat.global_daily_limit",
            "Tageslimit gesamt",
            "Höchstzahl an Brain-Antworten pro Tag.",
            1,
            10_000,
        )
        .choice(
            "dashboard.options.brain_client.mode",
            "Dashboard: Brain-Modus",
            "legacy nutzt den alten Weg, shadow vergleicht nur, typed nutzt das Brain.",
            MODES,
        )
        .opt_text(
            "dashboard.options.brain_client.endpoint",
            "Dashboard: Brain-Adresse",
            "Lokale Adresse des Brain-Dienstes.",
        )
        .texts(
            "dashboard.options.brain_client.public_scopes",
            "Dashboard: Brain-Bereiche",
            "Freigegebene Wissensbereiche für das Dashboard.",
            (0, 32),
        )
        .opt_int(
            "dashboard.options.brain_client.timeout_ms",
            "Dashboard: Brain-Zeitlimit",
            "Millisekunden bis zum Abbruch einer Brain-Anfrage.",
            1,
            65_000,
        )
        .flag(
            "dashboard.options.brain_client.title_ai_via_brain",
            "Dashboard: Titel über das Brain",
            "Erzeugt Titelvorschläge im Dashboard über das Brain.",
        );
}

fn vod_archive(b: &mut Builder) {
    b.group("vod_archive", true)
        .text(
            "bot.vod_archive.download_dir",
            "Download-Ordner",
            "Ablage für heruntergeladene VODs.",
        )
        .int(
            "bot.vod_archive.max_downloads_per_run",
            "Downloads pro Lauf",
            "Höchstzahl an VOD-Downloads je Lauf.",
            1,
            100,
        )
        .int(
            "bot.vod_archive.max_uploads_per_run",
            "Uploads pro Lauf",
            "Höchstzahl an YouTube-Uploads je Lauf.",
            1,
            3,
        )
        .int(
            "bot.vod_archive.min_free_gb",
            "Mindestens freier Speicher",
            "Gigabyte, die auf der Platte frei bleiben müssen.",
            0,
            ID_MAX,
        )
        .opt_text(
            "bot.vod_archive.rate_limit",
            "Download-Bandbreite",
            "Bandbreitenlimit für yt-dlp, zum Beispiel 20M.",
        )
        .text(
            "bot.vod_archive.ffmpeg",
            "ffmpeg-Programm",
            "Pfad oder Name des ffmpeg-Programms.",
        )
        .text(
            "bot.vod_archive.ffprobe",
            "ffprobe-Programm",
            "Pfad oder Name des ffprobe-Programms.",
        )
        .text(
            "bot.vod_archive.rclone",
            "rclone-Programm",
            "Pfad oder Name des rclone-Programms.",
        )
        .int(
            "bot.vod_archive.download_timeout_seconds",
            "Download-Zeitlimit",
            "Sekunden bis zum Abbruch eines Downloads.",
            60,
            86_400,
        )
        .int(
            "bot.vod_archive.interval_hours",
            "Archivtakt",
            "Stunden zwischen zwei Archivläufen.",
            12,
            168,
        )
        .text(
            "bot.vod_archive.category_id",
            "YouTube-Kategorie",
            "Numerische YouTube-Kategorie der Uploads.",
        )
        .text(
            "bot.vod_archive.title_template",
            "Titelvorlage",
            "Vorlage mit {title}, {date} und {part}.",
        )
        .opt_text(
            "bot.vod_archive.playlist_id",
            "YouTube-Playlist",
            "Playlist, in die Uploads einsortiert werden.",
        )
        .int(
            "bot.vod_archive.youtube_poll_seconds",
            "YouTube-Abfragetakt",
            "Sekunden zwischen zwei Statusabfragen.",
            30,
            3_600,
        )
        .int(
            "bot.vod_archive.youtube_check_hours",
            "YouTube-Prüffenster",
            "Stunden, in denen Uploads nachgeprüft werden.",
            1,
            168,
        )
        .int(
            "bot.vod_archive.youtube_processing_minutes",
            "YouTube-Verarbeitungszeit",
            "Minuten bis zur erneuten Prüfung laufender Verarbeitung.",
            5,
            1_440,
        )
        .int(
            "bot.vod_archive.youtube_error_minutes",
            "Wartezeit nach Fehler",
            "Minuten bis zum neuen Versuch nach einem YouTube-Fehler.",
            10,
            1_440,
        )
        .int(
            "bot.vod_archive.youtube_quota_hours",
            "Wartezeit nach Kontingentende",
            "Stunden Pause, wenn das YouTube-Kontingent erschöpft ist.",
            1,
            168,
        )
        .int(
            "bot.vod_archive.youtube_requests_per_run",
            "YouTube-Anfragen pro Lauf",
            "Höchstzahl an YouTube-Anfragen je Lauf.",
            3,
            100,
        );
}

fn discord(b: &mut Builder) {
    b.group("discord", true)
        .int(
            "discord.chat.moderation_alert_channel_id",
            "Moderationsalarm-Kanal",
            "Discord-Kanal für Moderationshinweise aus dem Chat.",
            1,
            ID_MAX,
        )
        .opt_text(
            "discord.chat.promo_invite",
            "Discord-Einladung im Chat",
            "Einladungslink, den der Chat-Bot teilt.",
        )
        .text(
            "discord.internal.owner_id",
            "Besitzer-ID",
            "Discord-ID der Person mit Besitzerrechten.",
        )
        .flag(
            "discord.streamer_link.enabled",
            "Streamer-Verknüpfung",
            "Verknüpft Discord- und Twitch-Konten von Streamern.",
        )
        .int(
            "discord.streamer_link.notify_channel_id",
            "Kanal für Verknüpfungen",
            "Discord-Kanal für Hinweise zu neuen Verknüpfungen.",
            1,
            ID_MAX,
        )
        .int(
            "discord.streamer_link.streamer_role_id",
            "Streamer-Rolle (Verknüpfung)",
            "Muss mit der Rolle der Rollenabgleiche übereinstimmen.",
            1,
            ID_MAX,
        )
        .int(
            "discord.streamer_link.guild_id",
            "Server (Verknüpfung)",
            "Muss mit dem Server der Rollenabgleiche übereinstimmen.",
            1,
            ID_MAX,
        )
        .text(
            "discord.streamer_link.state_path",
            "Statusdatei der Verknüpfung",
            "Pfad der Statusdatei für Verknüpfungen.",
        )
        .int(
            "discord.oauth_followup.guild_id",
            "Server für Rollenabgleich",
            "Discord-Server, in dem Streamer-Rollen vergeben werden.",
            1,
            ID_MAX,
        )
        .int(
            "discord.oauth_followup.streamer_role_id",
            "Streamer-Rolle",
            "Discord-Rolle für verifizierte Streamer.",
            1,
            ID_MAX,
        )
        .int(
            "discord.oauth_followup.live_streamer_role_id",
            "Live-Streamer-Rolle",
            "Discord-Rolle für Streamer, die gerade Deadlock live streamen.",
            1,
            ID_MAX,
        )
        .int(
            "discord.oauth_followup.role_sync_interval_secs",
            "Takt des Rollenabgleichs",
            "Sekunden zwischen zwei Rollenabgleichen.",
            15,
            3_600,
        )
        .int(
            "discord.oauth_followup.role_keep_window_days",
            "Haltefenster der Streamer-Rolle",
            "Tage, in denen ein Rolleninhaber mindestens einmal Deadlock gestreamt haben muss.",
            1,
            365,
        )
        .int(
            "discord.oauth_followup.role_gain_window_days",
            "Fenster für neue Streamer-Rollen",
            "Tage, in denen die Deadlock-Streamtage für eine neue Rolle gezählt werden.",
            1,
            365,
        )
        .int(
            "discord.oauth_followup.role_gain_min_days",
            "Streamtage für neue Streamer-Rollen",
            "Verschiedene Tage mit Deadlock-Stream, die für eine neue Rolle nötig sind.",
            1,
            365,
        )
        .opt_int(
            "discord.token_lifecycle.guild_id",
            "Server für Token-Ablauf",
            "Leer übernimmt keinen eigenen Server.",
            1,
            ID_MAX,
        )
        .int(
            "discord.token_lifecycle.streamer_role_id",
            "Streamer-Rolle (Token-Ablauf)",
            "Muss mit der Streamer-Rolle übereinstimmen.",
            1,
            ID_MAX,
        )
        .ints(
            "discord.raid_oauth.allowed_guild_ids",
            "Erlaubte Server für Raid-Freigabe",
            "Leer lassen schaltet die Prüfung ab, eine leere Liste erlaubt niemanden.",
            1,
            ID_MAX,
            (0, 512),
        )
        .ints(
            "discord.raid_oauth.allowed_channel_ids",
            "Erlaubte Kanäle für Raid-Freigabe",
            "Leer lassen schaltet die Prüfung ab, eine leere Liste erlaubt niemanden.",
            1,
            ID_MAX,
            (0, 512),
        )
        .ints(
            "discord.raid_oauth.allowed_role_ids",
            "Erlaubte Rollen für Raid-Freigabe",
            "Leer lassen schaltet die Prüfung ab, eine leere Liste erlaubt niemanden.",
            1,
            ID_MAX,
            (0, 512),
        )
        .text(
            "discord.raid_oauth.success_redirect_url",
            "Weiterleitung nach Raid-Freigabe",
            "Seite nach erfolgreicher Freigabe.",
        )
        .opt_int(
            "discord.shadow_review_channel_id",
            "Kanal für Schattenprüfung",
            "Discord-Kanal für Prüfungen im Schattenmodus.",
            1,
            ID_MAX,
        );
}

fn reliability(b: &mut Builder) {
    b.group("reliability", true)
        .flag(
            "monitoring.scout_enabled",
            "Scout",
            "Sucht nach neuen passenden Streams.",
        )
        .int(
            "monitoring.capacity_sample_seconds",
            "Takt der Kapazitätsmessung",
            "Sekunden zwischen zwei Kapazitätsmessungen.",
            30,
            3_600,
        )
        .int(
            "monitoring.capacity_retention_days",
            "Aufbewahrung der Kapazitätsdaten",
            "Tage, die Kapazitätsdaten gespeichert bleiben.",
            7,
            365,
        )
        .int(
            "monitoring.observability_retention_days",
            "Aufbewahrung der Betriebsdaten",
            "Tage, die Betriebsdaten gespeichert bleiben.",
            7,
            365,
        )
        .int(
            "database.retry.attempts",
            "Transaktionsversuche",
            "Versuche bei vorübergehenden Datenbankfehlern.",
            1,
            U32_MAX,
        )
        .number(
            "database.retry.base_delay_seconds",
            "Erste Wartezeit",
            "Sekunden vor dem ersten Wiederholungsversuch.",
            0.01,
            None,
        )
        .number(
            "database.retry.max_delay_seconds",
            "Längste Wartezeit",
            "Höchstens so viele Sekunden zwischen zwei Versuchen.",
            0.05,
            None,
        );
}

fn services(b: &mut Builder) {
    b.group("services", true)
        .int(
            "internal_api.port",
            "Port der internen API",
            "Lokaler Port der internen API.",
            1,
            PORT_MAX,
        )
        .opt_text(
            "internal_api.client_base_url",
            "Adresse der internen API",
            "Abweichende Adresse für Aufrufer der internen API.",
        )
        .flag(
            "internal_api.probe_allow_non_loopback",
            "Prüfungen von außen",
            "Erlaubt Zustandsprüfungen von anderen Rechnern.",
        )
        .int(
            "dashboard.port",
            "Dashboard-Port",
            "Lokaler Port des Dashboards.",
            1,
            PORT_MAX,
        )
        .flag(
            "dashboard.run_database_migrations",
            "Dashboard-Migrationen",
            "Führt ausstehende Migrationen beim Dashboard-Start aus.",
        )
        .text(
            "broker.base_url",
            "Broker-Adresse",
            "Lokale Adresse des Discord-Brokers.",
        )
        .choice(
            "logging.level",
            "Protokollstufe",
            "Ab dieser Stufe werden Meldungen protokolliert.",
            &["error", "warn", "info", "debug", "trace"],
        );
}

fn dashboard(b: &mut Builder) {
    b.group("dashboard", true)
        .flag(
            "dashboard.options.runtime_enforce",
            "Laufzeitsperre erzwingen",
            "Verhindert, dass zwei Dashboards dieselbe Rolle übernehmen.",
        )
        .text(
            "dashboard.options.runtime_role",
            "Laufzeitrolle",
            "Rolle dieses Dashboard-Prozesses.",
        )
        .text(
            "dashboard.options.runtime_lock_dir",
            "Ordner der Laufzeitsperre",
            "Ablage der Sperrdateien.",
        )
        .opt_text(
            "dashboard.options.legacy_fallback_url",
            "Alte Ersatzadresse",
            "Adresse des alten Dashboards für den Übergang.",
        )
        .flag(
            "dashboard.options.cookie_insecure",
            "Cookies ohne HTTPS",
            "Nur für lokale Tests einschalten.",
        )
        .flag(
            "dashboard.options.noauth_readiness",
            "Bereitschaft ohne Anmeldung",
            "Erlaubt die Bereitschaftsprüfung ohne Anmeldung.",
        )
        .opt_text(
            "dashboard.options.oauth_redirect_uri",
            "OAuth-Weiterleitung",
            "Rückkehradresse nach der Twitch-Anmeldung.",
        )
        .opt_text(
            "dashboard.options.affiliate_oauth_redirect_uri",
            "OAuth-Weiterleitung Partnerprogramm",
            "Rückkehradresse für die Anmeldung im Partnerprogramm.",
        )
        .opt_text(
            "dashboard.options.public_dashboard_url",
            "Öffentliche Dashboard-Adresse",
            "Adresse, unter der Streamer das Dashboard erreichen.",
        )
        .opt_text(
            "dashboard.options.admin_public_url",
            "Öffentliche Admin-Adresse",
            "Adresse der Admin-Oberfläche.",
        )
        .text(
            "dashboard.options.discord_oauth_broker_url",
            "Discord-Anmeldebroker",
            "Lokale Adresse des Brokers für die Discord-Anmeldung.",
        )
        .opt_int(
            "dashboard.options.admin_owner_user_id",
            "Admin-Besitzer",
            "Discord-ID mit vollen Adminrechten.",
            1,
            ID_MAX,
        )
        .opt_text(
            "dashboard.options.admin_twitch_user_id",
            "Admin-Twitch-Konto",
            "Numerische Twitch-ID mit Adminrechten.",
        )
        .ints(
            "dashboard.options.admin_guild_ids",
            "Admin-Server",
            "Discord-Server, deren Admins Zugriff erhalten.",
            1,
            ID_MAX,
            (0, 512),
        )
        .text(
            "dashboard.options.shared_admin_cookie_domain",
            "Cookie-Domain",
            "Domain für das gemeinsame Admin-Cookie.",
        )
        .opt_text(
            "dashboard.options.demo_login_twitch_user_id",
            "Demo-Konto",
            "Numerische Twitch-ID für die Demo-Anmeldung.",
        )
        .text(
            "dashboard.options.demo_login_display_name",
            "Demo-Anzeigename",
            "Name, der in der Demo angezeigt wird.",
        )
        .texts(
            "dashboard.options.demo_embed_origins",
            "Erlaubte Demo-Einbettungen",
            "Seiten, die die Demo einbetten dürfen.",
            (0, 64),
        )
        .opt_text(
            "dashboard.options.billing_public_origin",
            "Öffentliche Abrechnungsadresse",
            "Adresse für Rücksprünge aus der Abrechnung.",
        )
        .text(
            "dashboard.options.discord_ref_code",
            "Discord-Empfehlungscode",
            "Code für Discord-Einladungen im Dashboard.",
        )
        .text(
            "dashboard.options.steam_rank_url",
            "Steam-Rangabfrage",
            "Lokale Adresse der Rangabfrage.",
        )
        .text(
            "dashboard.options.steam_title_context_url",
            "Steam-Titelkontext",
            "Lokale Adresse für Spielkontext zu Titeln.",
        )
        .text(
            "dashboard.options.steam_link_start_base_url",
            "Steam-Verknüpfung",
            "Startadresse der Steam-Verknüpfung.",
        )
        .text(
            "dashboard.options.turnier_internal_base_url",
            "Turnier-Dienst",
            "Lokale Adresse des Turnier-Dienstes.",
        )
        .text(
            "dashboard.options.deadlock_assets_base_url",
            "Deadlock-Grafiken",
            "Basisadresse für Spielgrafiken.",
        )
        .opt_text(
            "dashboard.options.helix_base_url",
            "Twitch-API-Adresse",
            "Abweichende Adresse der Twitch-API, nur für Tests.",
        )
        .text(
            "dashboard.options.admin_dist_path",
            "Admin-Oberfläche",
            "Ordner der gebauten Admin-Oberfläche.",
        )
        .text(
            "dashboard.options.dashboard_dist_path",
            "Streamer-Dashboard",
            "Ordner des gebauten Streamer-Dashboards.",
        )
        .text(
            "dashboard.options.website_dist_path",
            "Website",
            "Ordner der gebauten Website.",
        )
        .text(
            "dashboard.options.legal_pages_path",
            "Rechtsseiten",
            "Datei mit den Rechtsseiten.",
        )
        .texts(
            "dashboard.options.excluded_bot_logins",
            "Ausgeschlossene Bot-Konten",
            "Twitch-Logins, die nicht als Zuschauer zählen.",
            (0, 4_096),
        )
        .texts(
            "dashboard.options.additional_bot_user_ids",
            "Weitere Bot-IDs",
            "Numerische Twitch-IDs weiterer Bots.",
            (0, 4_096),
        );
}

fn affiliate(b: &mut Builder) {
    b.group("affiliate", true)
        .opt_text(
            "dashboard.options.affiliate_mail.host",
            "Mailserver",
            "Hostname des SMTP-Servers ohne Protokoll.",
        )
        .int(
            "dashboard.options.affiliate_mail.port",
            "Mailserver-Port",
            "Port des SMTP-Servers.",
            1,
            PORT_MAX,
        )
        .opt_text(
            "dashboard.options.affiliate_mail.from_email",
            "Absenderadresse",
            "E-Mail-Adresse für Partner-Mails.",
        )
        .text(
            "dashboard.options.affiliate_mail.from_name",
            "Absendername",
            "Name für Partner-Mails.",
        )
        .flag(
            "dashboard.options.affiliate_mail.starttls",
            "STARTTLS",
            "Verschlüsselt die Verbindung nach dem Aufbau.",
        )
        .flag(
            "dashboard.options.affiliate_mail.use_ssl",
            "SSL",
            "Verschlüsselt die Verbindung von Anfang an.",
        )
        .text(
            "dashboard.options.affiliate_seller.name",
            "Rechnungssteller",
            "Name auf Partnerrechnungen.",
        )
        .text(
            "dashboard.options.affiliate_seller.company",
            "Firmierung",
            "Firmenbezeichnung auf Partnerrechnungen.",
        )
        .text(
            "dashboard.options.affiliate_seller.street",
            "Straße",
            "Anschrift auf Partnerrechnungen.",
        )
        .text(
            "dashboard.options.affiliate_seller.postal_code",
            "Postleitzahl",
            "Postleitzahl auf Partnerrechnungen.",
        )
        .text(
            "dashboard.options.affiliate_seller.city",
            "Ort",
            "Ort auf Partnerrechnungen.",
        )
        .text(
            "dashboard.options.affiliate_seller.country",
            "Land",
            "Ländercode auf Partnerrechnungen.",
        )
        .text(
            "dashboard.options.affiliate_seller.email",
            "Rechnungs-E-Mail",
            "Kontaktadresse auf Partnerrechnungen.",
        )
        .opt_text(
            "dashboard.options.affiliate_seller.website",
            "Website",
            "Website auf Partnerrechnungen.",
        )
        .text(
            "dashboard.options.affiliate_seller.tax_id",
            "Steuernummer",
            "Steuernummer oder USt-IdNr. auf Partnerrechnungen.",
        );
}

fn stt(b: &mut Builder) {
    b.group("stt", true)
        .int(
            "stt.port",
            "Port",
            "Lokaler Port des Spracherkennungsdienstes.",
            1,
            PORT_MAX,
        )
        .text("stt.model", "Modell", "Name oder Pfad des Whisper-Modells.")
        .opt_text(
            "stt.model_directory",
            "Modellordner",
            "Ordner mit lokalen Modellen.",
        )
        .int(
            "stt.threads",
            "Threads",
            "Rechenthreads für die Spracherkennung.",
            1,
            64,
        )
        .opt_text(
            "stt.language",
            "Sprache",
            "Feste Sprache, zum Beispiel de; leer für automatische Erkennung.",
        )
        .number(
            "stt.no_speech_max",
            "Grenze für Stille",
            "Segmente über diesem Wert gelten als Stille.",
            0.0,
            Some(1.0),
        )
        .number(
            "stt.avg_logprob_min",
            "Mindestsicherheit",
            "Segmente unter diesem Wert werden verworfen.",
            -20.0,
            Some(0.0),
        )
        .int(
            "stt.max_upload_bytes",
            "Größte Audiodatei",
            "Bytes je Audiodatei.",
            1_024,
            26_214_400,
        )
        .int(
            "stt.timeout_seconds",
            "Zeitlimit",
            "Sekunden bis zum Abbruch einer Erkennung.",
            1,
            3_600,
        )
        .int(
            "stt.extraction_timeout_seconds",
            "Zeitlimit für Audioauszug",
            "Sekunden bis zum Abbruch des Audioauszugs.",
            1,
            3_600,
        )
        .opt_text(
            "stt.remote_endpoint",
            "Externer Dienst",
            "Nur die OpenAI-Transkriptionsadresse ist erlaubt.",
        )
        .text(
            "stt.remote_model",
            "Externes Modell",
            "Modell beim externen Dienst.",
        );
}

fn media(b: &mut Builder) {
    b.group("media", true)
        .text(
            "knowledge.directory",
            "Wissensordner",
            "Ordner mit dem lokalen Wissensbestand.",
        )
        .flag(
            "media.youtube_audit_passed",
            "YouTube-Prüfung bestanden",
            "Erst nach bestandener YouTube-Prüfung einschalten.",
        )
        .text(
            "media.social_media_public_origin",
            "Öffentliche Medienadresse",
            "Adresse, unter der Medien abrufbar sind.",
        )
        .flag(
            "category_archive.enabled",
            "Kategoriearchiv",
            "Archiviert Streams aus der Kategorie.",
        )
        .flag(
            "category_archive.remove_enabled",
            "Archivdateien entfernen",
            "Entfernt lokale Dateien nach dem Archivieren.",
        )
        .text(
            "category_archive.temporary_directory",
            "Temporärer Ordner",
            "Absoluter Pfad für Zwischendateien.",
        )
        .int(
            "category_archive.max_file_bytes",
            "Größte Datei",
            "Bytes je archivierter Datei.",
            1_048_576,
            8_589_934_592,
        )
        .int(
            "category_archive.min_free_bytes",
            "Mindestens freier Speicher",
            "Bytes, die auf der Platte frei bleiben müssen.",
            1_073_741_824,
            ID_MAX,
        )
        .int(
            "category_archive.command_timeout_seconds",
            "Zeitlimit je Befehl",
            "Sekunden bis zum Abbruch eines Archivbefehls.",
            10,
            7_200,
        );
}

fn challenges(b: &mut Builder) {
    const ACHIEVEMENT: &str = "Aufsteigende Schwellen für die Stufen dieses Erfolgs.";
    b.group("challenges", true)
        .flag(
            "challenges.enabled",
            "Challenges",
            "Schaltet Community-Challenges ein.",
        )
        .int(
            "challenges.poll_seconds",
            "Abfragetakt",
            "Sekunden zwischen zwei Auswertungen.",
            15,
            120,
        )
        .int(
            "challenges.evidence_max_gap_seconds",
            "Größte Nachweislücke",
            "Mindestens doppelter Abfragetakt, höchstens 300 Sekunden.",
            30,
            300,
        )
        .int(
            "challenges.source_timeout_seconds",
            "Zeitlimit der Quellen",
            "Sekunden je Quellabfrage.",
            1,
            30,
        )
        .int(
            "challenges.source_batch_size",
            "Datensätze je Abfrage",
            "Höchstzahl an Datensätzen je Quellabfrage.",
            1,
            1_000,
        )
        .int(
            "challenges.community_guild_id",
            "Community-Server",
            "Discord-Server der Challenges.",
            1,
            ID_MAX,
        )
        .int(
            "challenges.points.qualified_invite",
            "Punkte: Einladung",
            "Punkte für eine bestätigte Einladung.",
            1,
            10_000,
        )
        .int(
            "challenges.points.streamer_referral",
            "Punkte: Streamer geworben",
            "Punkte für einen geworbenen Streamer.",
            1,
            10_000,
        )
        .int(
            "challenges.points.co_stream",
            "Punkte: Co-Stream",
            "Punkte für einen gemeinsamen Stream.",
            1,
            10_000,
        )
        .int(
            "challenges.points.party_play",
            "Punkte: Gruppenspiel",
            "Punkte für ein gemeinsames Spiel.",
            1,
            10_000,
        )
        .int(
            "challenges.points.clip_submitted",
            "Punkte: Clip eingereicht",
            "Punkte für einen eingereichten Clip.",
            1,
            10_000,
        )
        .ints(
            "challenges.points.clip_top3",
            "Punkte: Top-3-Clips",
            "Punkte für Platz eins, zwei und drei.",
            1,
            10_000,
            (3, 3),
        )
        .int(
            "challenges.points.quest_done",
            "Punkte: Aufgabe erledigt",
            "Punkte je erledigter Aufgabe.",
            1,
            10_000,
        )
        .int(
            "challenges.points.quest_all_three_bonus",
            "Punkte: alle drei Aufgaben",
            "Bonus für alle drei Aufgaben.",
            1,
            10_000,
        )
        .int(
            "challenges.weekly_caps.party_play",
            "Wochengrenze Gruppenspiel",
            "Höchstzahl gewerteter Gruppenspiele pro Woche.",
            1,
            100,
        )
        .int(
            "challenges.weekly_caps.clip_submitted",
            "Wochengrenze Clips",
            "Höchstzahl gewerteter Clips pro Woche.",
            1,
            100,
        )
        .ints(
            "challenges.level_thresholds",
            "Stufenschwellen",
            "Beginnt mit 0, danach streng aufsteigend.",
            0,
            1_000_000_000,
            (2, 100),
        )
        .int(
            "challenges.stream_extra_minutes",
            "Zusätzliche Stream-Minuten",
            "Minuten, die nach Stream-Ende noch zählen.",
            1,
            600,
        )
        .ints(
            "challenges.achievements.recruiter",
            "Erfolg: Anwerber",
            ACHIEVEMENT,
            1,
            1_000_000_000,
            (1, 20),
        )
        .ints(
            "challenges.achievements.team_player",
            "Erfolg: Teamspieler",
            ACHIEVEMENT,
            1,
            1_000_000_000,
            (1, 20),
        )
        .ints(
            "challenges.achievements.duo",
            "Erfolg: Duo",
            ACHIEVEMENT,
            1,
            1_000_000_000,
            (1, 20),
        )
        .ints(
            "challenges.achievements.stamina",
            "Erfolg: Ausdauer",
            ACHIEVEMENT,
            1,
            1_000_000_000,
            (1, 20),
        )
        .ints(
            "challenges.achievements.talent_scout",
            "Erfolg: Talentsucher",
            ACHIEVEMENT,
            1,
            1_000_000_000,
            (1, 20),
        )
        .ints(
            "challenges.achievements.clip_hunter",
            "Erfolg: Clipjäger",
            ACHIEVEMENT,
            1,
            1_000_000_000,
            (1, 20),
        );
}

fn social_ai_provider(b: &mut Builder) {
    const RATE: (i64, i64) = (1, 100_000_000);
    b.group("social_ai_provider", false)
        .choice(
            "social_ai.provider.kind",
            "Anbieter",
            "KI-Anbieter des Schnitt-Editors.",
            &["anthropic"],
        )
        .text(
            "social_ai.provider.endpoint",
            "Anbieteradresse",
            "Offizielle HTTPS-Adresse des Anbieters.",
        )
        .text(
            "social_ai.provider.model",
            "Modell",
            "Muss mit dem Modell der Preistabelle übereinstimmen.",
        )
        .choice(
            "social_ai.provider.effort",
            "Denkaufwand",
            "Aufwandsstufe je Anfrage.",
            &["low", "medium", "high", "xhigh", "max"],
        )
        .int(
            "social_ai.provider.max_input_tokens",
            "Größte Eingabe",
            "Eingabe-Tokens je Anfrage.",
            1,
            128_000,
        )
        .int(
            "social_ai.provider.max_output_tokens",
            "Größte Ausgabe",
            "Ausgabe-Tokens je Anfrage.",
            1,
            4_096,
        )
        .int(
            "social_ai.provider.max_frames",
            "Bilder je Anfrage",
            "Höchstzahl an Standbildern je Anfrage.",
            0,
            8,
        )
        .int(
            "social_ai.provider.max_frame_bytes",
            "Größe je Bild",
            "Bytes je Standbild.",
            1,
            256_000,
        )
        .int(
            "social_ai.provider.max_text_bytes",
            "Größter Text",
            "Bytes Text je Anfrage.",
            1,
            32_000,
        )
        .int(
            "social_ai.provider.max_schema_bytes",
            "Größtes Antwortschema",
            "Bytes des Antwortschemas.",
            1,
            32_000,
        )
        .int(
            "social_ai.provider.request_timeout_ms",
            "Zeitlimit je Anfrage",
            "Millisekunden bis zum Abbruch einer Anfrage.",
            1_000,
            120_000,
        )
        .int(
            "social_ai.provider.total_deadline_ms",
            "Gesamtzeitlimit",
            "Millisekunden für alle Versuche zusammen, mindestens das Anfragelimit.",
            1_000,
            360_000,
        )
        .int(
            "social_ai.provider.max_retries",
            "Wiederholungen",
            "Weitere Versuche nach einem Fehler.",
            0,
            2,
        )
        .int(
            "social_ai.provider.retry_backoff_ms",
            "Wartezeit vor Wiederholung",
            "Millisekunden vor dem nächsten Versuch.",
            0,
            3_000,
        )
        .int(
            "social_ai.provider.initial_total_cap_micro_usd",
            "Kostendeckel",
            "Gesamtbudget in Mikro-US-Dollar.",
            1,
            2_000_000,
        )
        .int(
            "social_ai.provider.pricing_version",
            "Preisversion",
            "Muss mit der Version der Preistabelle übereinstimmen.",
            0,
            U32_MAX,
        )
        .int(
            "social_ai.provider.pricing.input_micro_usd_per_million",
            "Eingabepreis",
            "Mikro-US-Dollar je Million Eingabe-Tokens.",
            RATE.0,
            RATE.1,
        )
        .int(
            "social_ai.provider.pricing.output_micro_usd_per_million",
            "Ausgabepreis",
            "Mikro-US-Dollar je Million Ausgabe-Tokens.",
            RATE.0,
            RATE.1,
        )
        .int(
            "social_ai.provider.pricing.cache_creation_micro_usd_per_million",
            "Preis Cache-Aufbau",
            "Mikro-US-Dollar je Million Tokens beim Cache-Aufbau.",
            RATE.0,
            RATE.1,
        )
        .int(
            "social_ai.provider.pricing.cache_read_micro_usd_per_million",
            "Preis Cache-Lesen",
            "Mikro-US-Dollar je Million gelesener Cache-Tokens.",
            RATE.0,
            RATE.1,
        )
        .int(
            "social_ai.pricing.version",
            "Preistabelle: Version",
            "Vertrauenswürdige Preisversion.",
            0,
            U32_MAX,
        )
        .text(
            "social_ai.pricing.model",
            "Preistabelle: Modell",
            "Modell, für das die Preise gelten.",
        )
        .int(
            "social_ai.pricing.rates.input_micro_usd_per_million",
            "Preistabelle: Eingabe",
            "Mikro-US-Dollar je Million Eingabe-Tokens.",
            RATE.0,
            RATE.1,
        )
        .int(
            "social_ai.pricing.rates.output_micro_usd_per_million",
            "Preistabelle: Ausgabe",
            "Mikro-US-Dollar je Million Ausgabe-Tokens.",
            RATE.0,
            RATE.1,
        )
        .int(
            "social_ai.pricing.rates.cache_creation_micro_usd_per_million",
            "Preistabelle: Cache-Aufbau",
            "Mikro-US-Dollar je Million Tokens beim Cache-Aufbau.",
            RATE.0,
            RATE.1,
        )
        .int(
            "social_ai.pricing.rates.cache_read_micro_usd_per_million",
            "Preistabelle: Cache-Lesen",
            "Mikro-US-Dollar je Million gelesener Cache-Tokens.",
            RATE.0,
            RATE.1,
        );
}

fn social_ai_runtime(b: &mut Builder) {
    b.group("social_ai_runtime", false)
        .text(
            "social_ai.runtime.source_root",
            "Quellordner",
            "Ordner der Ausgangsclips, gleich dem Ausgabeordner.",
        )
        .text(
            "social_ai.runtime.work_root",
            "Arbeitsordner",
            "Unterordner des Ausgabeordners für Zwischenstände.",
        )
        .text(
            "social_ai.runtime.output_root",
            "Ausgabeordner",
            "Ordner der fertigen Schnitte.",
        )
        .int(
            "social_ai.runtime.stage_attempts",
            "Versuche je Schritt",
            "Versuche je Bearbeitungsschritt.",
            1,
            3,
        )
        .int(
            "social_ai.runtime.worker_poll_ms",
            "Abfragetakt",
            "Millisekunden zwischen zwei Auftragsabfragen.",
            100,
            60_000,
        )
        .int(
            "social_ai.runtime.job_deadline_ms",
            "Zeitlimit je Auftrag",
            "Millisekunden für einen ganzen Auftrag.",
            60_000,
            3_600_000,
        )
        .int(
            "social_ai.runtime.recovery_limit",
            "Wiederaufnahmen je Start",
            "Höchstzahl wieder aufgenommener Aufträge beim Start.",
            1,
            100,
        )
        .int(
            "social_ai.runtime.list_limit",
            "Einträge je Liste",
            "Höchstzahl an Aufträgen je Listenabfrage.",
            1,
            50,
        )
        .int(
            "social_ai.runtime.max_snapshot_bytes",
            "Größter Zwischenstand",
            "Bytes je gespeichertem Zwischenstand.",
            1_024,
            32_768,
        )
        .int(
            "social_ai.runtime.max_skill_bytes",
            "Größte Skill-Datei",
            "Bytes je Skill-Anweisung.",
            128,
            4_096,
        );
}

fn social_ai_resources(b: &mut Builder) {
    const BYTES: i64 = 512 * 1024 * 1024;
    b.group("social_ai_resources", false)
        .int(
            "social_ai.runtime.resources.max_source_bytes",
            "Größte Quelldatei",
            "Bytes je Ausgangsclip.",
            1,
            BYTES,
        )
        .int(
            "social_ai.runtime.resources.max_output_bytes",
            "Größte Ausgabedatei",
            "Bytes je fertigem Schnitt.",
            1,
            BYTES,
        )
        .int(
            "social_ai.runtime.resources.max_probe_bytes",
            "Größte Medienanalyse",
            "Bytes der ffprobe-Ausgabe.",
            1_024,
            65_536,
        )
        .int(
            "social_ai.runtime.resources.max_media_duration_ms",
            "Längster Clip",
            "Millisekunden je Clip.",
            1,
            120_000,
        )
        .int(
            "social_ai.runtime.resources.max_media_width",
            "Größte Breite",
            "Pixel Breite je Clip.",
            1_920,
            7_680,
        )
        .int(
            "social_ai.runtime.resources.max_media_height",
            "Größte Höhe",
            "Pixel Höhe je Clip.",
            1_920,
            4_320,
        )
        .int(
            "social_ai.runtime.resources.max_frames",
            "Standbilder je Clip",
            "Höchstens so viele wie der Anbieter je Anfrage erlaubt.",
            1,
            8,
        )
        .int(
            "social_ai.runtime.resources.frame_width",
            "Breite der Standbilder",
            "Gerade Pixelzahl.",
            2,
            1_920,
        )
        .int(
            "social_ai.runtime.resources.max_frame_bytes",
            "Größe je Standbild",
            "Höchstens die Bildgröße des Anbieters.",
            1,
            256_000,
        )
        .int(
            "social_ai.runtime.resources.max_ass_bytes",
            "Größte Untertiteldatei",
            "Bytes je Untertiteldatei.",
            1_024,
            262_144,
        )
        .int(
            "social_ai.runtime.resources.process_deadline_ms",
            "Zeitlimit je Prozess",
            "Millisekunden je ffmpeg-Aufruf.",
            1_000,
            600_000,
        )
        .int(
            "social_ai.runtime.resources.total_frames_deadline_ms",
            "Zeitlimit für Standbilder",
            "Millisekunden für alle Standbilder zusammen.",
            1_000,
            600_000,
        )
        .int(
            "social_ai.runtime.resources.max_duration_drift_ms",
            "Erlaubte Längenabweichung",
            "Millisekunden Abweichung der Medienlänge.",
            1,
            1_000,
        )
        .int(
            "social_ai.runtime.resources.encoder_threads",
            "Encoder-Threads",
            "Threads je Encoder.",
            1,
            3,
        );
}

fn social_ai_plan(b: &mut Builder) {
    b.group("social_ai_plan", false)
        .int(
            "social_ai.runtime.plan.max_plan_bytes",
            "Größter Schnittplan",
            "Bytes, höchstens der größte Zwischenstand.",
            1,
            32_768,
        )
        .int(
            "social_ai.runtime.plan.max_source_duration_ms",
            "Längste Quelle",
            "Millisekunden, höchstens die Cliplänge.",
            1,
            120_000,
        )
        .int(
            "social_ai.runtime.plan.max_source_duration_drift_ms",
            "Erlaubte Quellabweichung",
            "Millisekunden, höchstens die Medienabweichung.",
            1,
            1_000,
        )
        .int(
            "social_ai.runtime.plan.max_output_duration_ms",
            "Längster Schnitt",
            "Millisekunden, höchstens die Cliplänge.",
            1,
            120_000,
        )
        .int(
            "social_ai.runtime.plan.max_intervals",
            "Abschnitte",
            "Höchstzahl an Schnittabschnitten.",
            1,
            64,
        )
        .int(
            "social_ai.runtime.plan.max_highlights",
            "Highlights",
            "Höchstzahl an Highlights.",
            1,
            64,
        )
        .int(
            "social_ai.runtime.plan.max_review_reasons",
            "Prüfgründe",
            "Höchstzahl an Prüfgründen.",
            1,
            32,
        )
        .int(
            "social_ai.runtime.plan.max_text_bytes",
            "Textlänge",
            "Bytes je Text im Plan.",
            1,
            512,
        )
        .int(
            "social_ai.runtime.plan.max_subtitle_segments",
            "Untertitelsegmente",
            "Höchstzahl an Untertitelsegmenten.",
            1,
            512,
        )
        .int(
            "social_ai.runtime.plan.max_subtitle_text_bytes",
            "Untertitellänge",
            "Bytes je Untertitelsegment.",
            1,
            512,
        )
        .int(
            "social_ai.runtime.plan.min_kept_percent",
            "Mindestens behalten",
            "Prozent des Clips, die im Schnitt bleiben.",
            0,
            100,
        );
}

fn social_ai_output(b: &mut Builder) {
    b.group("social_ai_output", false)
        .int(
            "social_ai.runtime.original_preview.max_width",
            "Vorschau: Breite",
            "Gerade Pixelzahl.",
            320,
            1_920,
        )
        .int(
            "social_ai.runtime.original_preview.max_height",
            "Vorschau: Höhe",
            "Gerade Pixelzahl.",
            180,
            1_080,
        )
        .int(
            "social_ai.runtime.original_preview.max_output_bytes",
            "Vorschau: Dateigröße",
            "Bytes, höchstens die größte Ausgabedatei.",
            1,
            512 * 1024 * 1024,
        )
        .int(
            "social_ai.runtime.original_preview.encoder_threads",
            "Vorschau: Encoder-Threads",
            "Threads für die Vorschau.",
            1,
            3,
        )
        .int(
            "social_ai.runtime.original_preview.nice",
            "Vorschau: Priorität",
            "Nice-Wert des Encoders.",
            1,
            19,
        )
        .int(
            "social_ai.runtime.original_preview.crf",
            "Vorschau: Qualität",
            "CRF-Wert, kleiner heißt besser.",
            18,
            35,
        )
        .choice(
            "social_ai.runtime.original_preview.preset",
            "Vorschau: Encoder-Preset",
            "Schnellere Presets sparen Rechenzeit.",
            &["ultrafast", "superfast", "veryfast", "faster", "fast"],
        )
        .int(
            "social_ai.runtime.original_preview.process_deadline_ms",
            "Vorschau: Zeitlimit",
            "Millisekunden, höchstens das Auftragslimit.",
            1_000,
            600_000,
        )
        .int(
            "social_ai.runtime.copy.max_title_chars",
            "Titel: Zeichen",
            "Zeichen je Titel.",
            1,
            100,
        )
        .int(
            "social_ai.runtime.copy.max_description_chars",
            "Beschreibung: Zeichen",
            "Zeichen je Beschreibung.",
            1,
            2_200,
        )
        .int(
            "social_ai.runtime.copy.max_hashtags",
            "Hashtags",
            "Höchstzahl an Hashtags.",
            1,
            30,
        )
        .int(
            "social_ai.runtime.copy.max_hashtag_chars",
            "Hashtag: Zeichen",
            "Zeichen je Hashtag.",
            2,
            64,
        )
        .group("social_ai_output", true)
        .int(
            "social_ai.runtime.revisions.max_feedback_bytes",
            "Überarbeitung: Rückmeldung",
            "Bytes je Rückmeldung.",
            1,
            4_000,
        )
        .int(
            "social_ai.runtime.revisions.max_revisions",
            "Überarbeitungen",
            "Höchstzahl an Überarbeitungen je Auftrag.",
            1,
            20,
        )
        .int(
            "social_ai.runtime.revisions.max_auto_changed_percent",
            "Automatisch geändert",
            "Prozent, die eine Überarbeitung ohne Prüfung ändern darf.",
            1,
            20,
        )
        .int(
            "social_ai.runtime.revisions.max_variants",
            "Varianten",
            "Höchstzahl an Varianten je Überarbeitung.",
            1,
            3,
        );
}

fn social_ai_stt(b: &mut Builder) {
    b.group("social_ai_stt", false)
        .text(
            "social_ai.runtime.stt.binary",
            "Programm",
            "Pfad zum Spracherkennungsprogramm.",
        )
        .int(
            "social_ai.runtime.stt.nice",
            "Priorität",
            "Nice-Wert des Prozesses.",
            1,
            19,
        )
        .int(
            "social_ai.runtime.stt.startup_deadline_ms",
            "Startzeitlimit",
            "Millisekunden bis der Dienst bereit sein muss.",
            100,
            600_000,
        )
        .int(
            "social_ai.runtime.stt.shutdown_deadline_ms",
            "Stoppzeitlimit",
            "Millisekunden für das Beenden.",
            100,
            60_000,
        )
        .int(
            "social_ai.runtime.stt.poll_interval_ms",
            "Abfragetakt",
            "Millisekunden, höchstens das Startzeitlimit.",
            10,
            10_000,
        )
        .group("social_ai_stt", true)
        .int(
            "social_ai.runtime.stt.settings.port",
            "Port",
            "Eigener lokaler Port, nicht 8791.",
            1,
            PORT_MAX,
        )
        .text(
            "social_ai.runtime.stt.settings.model",
            "Modell",
            "Name des Whisper-Modells.",
        )
        .text(
            "social_ai.runtime.stt.settings.model_directory",
            "Modellordner",
            "Absoluter Pfad außerhalb von /home.",
        )
        .int(
            "social_ai.runtime.stt.settings.threads",
            "Threads",
            "Rechenthreads.",
            1,
            4,
        )
        .number(
            "social_ai.runtime.stt.settings.no_speech_max",
            "Grenze für Stille",
            "Segmente über diesem Wert gelten als Stille.",
            0.0,
            Some(1.0),
        )
        .number(
            "social_ai.runtime.stt.settings.avg_logprob_min",
            "Mindestsicherheit",
            "Segmente unter diesem Wert werden verworfen.",
            -20.0,
            Some(0.0),
        )
        .int(
            "social_ai.runtime.stt.settings.max_upload_bytes",
            "Größte Audiodatei",
            "Bytes je Audiodatei.",
            1_024,
            26_214_400,
        )
        .int(
            "social_ai.runtime.stt.settings.timeout_seconds",
            "Zeitlimit",
            "Sekunden je Erkennung.",
            1,
            3_600,
        )
        .int(
            "social_ai.runtime.stt.settings.extraction_timeout_seconds",
            "Zeitlimit für Audioauszug",
            "Sekunden je Audioauszug.",
            1,
            3_600,
        )
        .text(
            "social_ai.runtime.stt.settings.remote_model",
            "Externes Modell",
            "Wird nur bei externem Dienst genutzt.",
        );
}

fn social_ai_brain(b: &mut Builder) {
    b.group("social_ai_brain", true)
        .text(
            "social_ai.runtime.brain.endpoint",
            "Adresse",
            "Lokale Adresse mit Pfad /v1/retrieve.",
        )
        .text(
            "social_ai.runtime.brain.expected_release",
            "Erwarteter Stand",
            "Release-Kennung des Brain, nicht current.",
        )
        .int(
            "social_ai.runtime.brain.deadline_ms",
            "Zeitlimit",
            "Millisekunden je Abfrage.",
            1,
            60_000,
        )
        .int(
            "social_ai.runtime.brain.max_response_bytes",
            "Größte Antwort",
            "Bytes je Antwort.",
            512,
            131_072,
        )
        .int(
            "social_ai.runtime.brain.max_evidence_count",
            "Belege",
            "Höchstzahl an Belegen je Antwort.",
            1,
            8,
        )
        .int(
            "social_ai.runtime.brain.max_evidence_bytes",
            "Größe je Beleg",
            "Bytes je Beleg.",
            64,
            8_192,
        )
        .texts(
            "social_ai.runtime.brain.allowed_game_terms",
            "Erlaubte Spielbegriffe",
            "Begriffe, die der Kontext enthalten darf.",
            (1, 4_096),
        );
}
