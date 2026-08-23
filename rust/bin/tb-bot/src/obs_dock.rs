//! Event-Bus der eigenen OBS-Docks: EventSub rein, `obs_dock_events` raus.
//!
//! Der Bot sieht jedes Twitch-Ereignis bereits, das ein Dock anzeigen soll. Statt
//! einen zweiten Anschluss zu Twitch zu bauen, haengt sich dieses Modul als
//! [`EventSubHooks`]-Wrapper vor die bestehende Hook-Kette (dasselbe Muster wie
//! `ChatHooks` in `chat_wiring.rs`), uebersetzt die rohen Twitch-Nutzlasten in
//! das eingefrorene [`PlatformEvent`]-Drahtformat aus `tb-platform-core`,
//! schreibt sie nach `obs_dock_events` und meldet die neue Zeile ueber
//! `pg_notify('obs_dock', '{"channel_id":"<id>","id":<id>}')`.
//!
//! Zwei Regeln sind nicht verhandelbar:
//!
//! 1. **Der Wrapper delegiert immer.** Faellt der eigene Schreibpfad aus, wird
//!    das geloggt und der innere Hook trotzdem unveraendert aufgerufen. Ein
//!    kaputter Dock-Bus darf weder Moderation noch Raids noch Telemetrie
//!    anhalten.
//! 2. **Er aendert nichts an den Argumenten.** Jede Hook-Methode reicht genau
//!    das durch, was sie bekommen hat; auch die Methoden, aus denen kein
//!    Dock-Ereignis entsteht, sind ausgeschrieben und delegieren.
//!
//! Der Schalter liegt in der Config-DATEI `~/.config/deadlock-twitch-bot/bot.json`
//! unter `obs_docks.enabled` und steht standardmaessig auf aus. Bewusst keine
//! Umgebungsvariable: Betriebswerte gehoeren in eine Config-Datei, nur Secrets
//! kommen aus Infisical.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use tb_monitoring::{ChatNotificationKind, EventSubHooks};
use tb_platform_core::{
    ActivityEvent, ActivityMeta, Actor, Badge, ChatMessage, Fragment, Platform, PlatformEvent,
    ReplyRef,
};

use crate::task_supervisor::TaskSupervisor;

/// Postgres-Kanal, auf dem das Gateway lauscht.
pub const NOTIFY_KANAL: &str = "obs_dock";

/// Aufbewahrung des Nachlaufpuffers in Minuten.
///
/// Ein Dock zieht beim Verbinden hoechstens die letzten Minuten nach; alles
/// aeltere ist fuer die Anzeige wertlos und wuerde die Tabelle nur aufblaehen.
pub const OBS_DOCK_RETENTION_MINUTES: i64 = 15;

/// Abstand zweier Aufraeumlaeufe.
///
/// Bewusst deutlich kuerzer als die 60 Minuten des allgemeinen
/// Retention-Ticks (`chatters_wiring::RETENTION_INTERVAL`): mit stuendlichem
/// Takt waeren aus 15 Minuten Aufbewahrung faktisch bis zu 75 Minuten
/// geworden, und dann waere die Zusage in der Migration falsch.
const RETENTION_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// URL-Muster der Twitch-Emote-CDN.
///
/// Die EventSub-Nutzlast traegt nur die Emote-ID; das Muster ist bei Twitch
/// fest. Die doppelten geschweiften Klammern sind Teil des Musters und werden
/// erst vom Dock ersetzt (`{{format}}`, `{{theme_mode}}`, `{{scale}}`).
const TWITCH_EMOTE_URL_TEMPLATE: &str =
    "https://static-cdn.jtvnw.net/emoticons/v2/{id}/{{format}}/{{theme_mode}}/{{scale}}";

// ---------------------------------------------------------------------------
// Konfiguration (Datei, kein ENV)
// ---------------------------------------------------------------------------

/// Wurzel der Bot-Config-Datei. Unbekannte Abschnitte werden ignoriert, damit
/// andere Bereiche spaeter danebenliegen koennen, ohne dass hier etwas bricht.
#[derive(Debug, Clone, Default, Deserialize)]
struct BotDatei {
    #[serde(default)]
    obs_docks: ObsDocksConfig,
}

/// Abschnitt `obs_docks` der Config-Datei.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ObsDocksConfig {
    /// `true` schaltet den Schreibpfad ein. Default: aus.
    #[serde(default)]
    pub enabled: bool,
}

impl ObsDocksConfig {
    /// Laedt den Abschnitt aus der Standard-Config-Datei.
    ///
    /// Fehlt die Datei, ist sie unlesbar oder kaputt, bleibt der Bus aus. Ein
    /// Tippfehler in der Config darf keinen zweiten Schreibpfad in die
    /// Datenbank aufmachen.
    pub fn laden() -> Self {
        match standard_config_pfad() {
            Some(pfad) => Self::aus_datei(&pfad),
            None => {
                tracing::debug!("obs_docks: kein HOME bekannt, Bus bleibt aus");
                Self::default()
            }
        }
    }

    /// Liest den Abschnitt aus einer bestimmten Datei (fuer Tests).
    pub fn aus_datei(pfad: &Path) -> Self {
        let roh = match std::fs::read_to_string(pfad) {
            Ok(roh) => roh,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!(pfad = %pfad.display(), "obs_docks: keine Config-Datei, Bus bleibt aus");
                return Self::default();
            }
            Err(error) => {
                tracing::warn!(%error, pfad = %pfad.display(), "obs_docks: Config-Datei nicht lesbar, Bus bleibt aus");
                return Self::default();
            }
        };
        match serde_json::from_str::<BotDatei>(&roh) {
            Ok(datei) => datei.obs_docks,
            Err(error) => {
                tracing::warn!(%error, pfad = %pfad.display(), "obs_docks: Config-Datei nicht lesbar (JSON), Bus bleibt aus");
                Self::default()
            }
        }
    }
}

/// `~/.config/deadlock-twitch-bot/bot.json`.
fn standard_config_pfad() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".config/deadlock-twitch-bot/bot.json"))
}

// ---------------------------------------------------------------------------
// Schreibpfad
// ---------------------------------------------------------------------------

/// Fehler des Dock-Schreibpfads.
#[derive(Debug, thiserror::Error)]
pub enum ObsDockError {
    /// Das Ereignis liess sich nicht in JSON ueberfuehren.
    #[error("Ereignis nicht serialisierbar: {0}")]
    Serde(#[from] serde_json::Error),
    /// Insert oder NOTIFY sind fehlgeschlagen.
    #[error("Datenbankfehler: {0}")]
    Db(#[from] sqlx::Error),
}

/// Ziel, in das der Wrapper seine Ereignisse legt.
///
/// Als Trait geschnitten, damit der Wrapper ohne Datenbank testbar bleibt: die
/// interessante Frage ist "genau ein Schreibaufruf, innerer Hook unveraendert",
/// nicht "kann Postgres INSERT".
#[async_trait]
pub trait ObsDockSink: Send + Sync {
    /// Schreibt ein Ereignis und meldet es. Liefert die vergebene Lauf-ID.
    async fn write(&self, event: &PlatformEvent) -> Result<i64, ObsDockError>;
}

/// Postgres-Ziel: eine Zeile in `obs_dock_events`, dann `pg_notify`.
pub struct PgObsDockSink {
    pool: PgPool,
}

impl PgObsDockSink {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ObsDockSink for PgObsDockSink {
    async fn write(&self, event: &PlatformEvent) -> Result<i64, ObsDockError> {
        let payload = serde_json::to_value(event)?;
        let channel_id = event.channel_id().to_string();

        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO obs_dock_events (channel_id, payload) VALUES ($1, $2) RETURNING id",
        )
        .bind(&channel_id)
        .bind(&payload)
        .fetch_one(&self.pool)
        .await?;

        // Bewusst als zweite Anweisung und nicht als CTE am INSERT: ob ein
        // Planer eine unbenutzte Spalte einer Unterabfrage auswertet, ist nicht
        // zugesichert, und ein stillschweigend verschlucktes NOTIFY waere genau
        // der Fehler, den niemand findet. Faellt es aus, ist die Zeile trotzdem
        // da und das Dock holt sie beim naechsten Verbinden ueber `seit=`.
        sqlx::query(&format!("SELECT pg_notify('{NOTIFY_KANAL}', $1)"))
            .bind(notify_nutzlast(&channel_id, id))
            .execute(&self.pool)
            .await?;

        Ok(id)
    }
}

/// Baut die NOTIFY-Nutzlast: genau zwei Felder, nichts sonst.
fn notify_nutzlast(channel_id: &str, id: i64) -> String {
    serde_json::json!({ "channel_id": channel_id, "id": id }).to_string()
}

// ---------------------------------------------------------------------------
// Aufbewahrung
// ---------------------------------------------------------------------------

/// Loescht alles, was aelter als [`OBS_DOCK_RETENTION_MINUTES`] ist.
pub async fn cleanup_obs_dock_events(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let cutoff = Utc::now() - chrono::Duration::minutes(OBS_DOCK_RETENTION_MINUTES);
    cleanup_obs_dock_events_before(pool, cutoff).await
}

/// Loescht alles vor `cutoff` (eigener Einstieg fuer Tests).
pub async fn cleanup_obs_dock_events_before(
    pool: &PgPool,
    cutoff: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM obs_dock_events WHERE created_at < $1")
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Startet den Aufraeum-Loop. Nur aufrufen, wenn der Bus eingeschaltet ist.
pub fn spawn_retention_loop(supervisor: &TaskSupervisor, pool: PgPool) {
    supervisor.spawn("obs_dock_retention", async move {
        let mut tick = tokio::time::interval(RETENTION_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            match cleanup_obs_dock_events(&pool).await {
                Ok(deleted) if deleted > 0 => tracing::debug!(
                    deleted,
                    retention_minutes = OBS_DOCK_RETENTION_MINUTES,
                    "obs_dock_retention: alte Ereignisse entfernt"
                ),
                Ok(_) => {}
                Err(error) => {
                    tracing::error!(%error, "obs_dock_retention: Aufraeumen fehlgeschlagen")
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Uebersetzung Twitch-Nutzlast nach PlatformEvent
// ---------------------------------------------------------------------------

fn str_field(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(text) = value.get(*key).and_then(Value::as_str) {
            let getrimmt = text.trim();
            if !getrimmt.is_empty() {
                return Some(getrimmt.to_string());
            }
        }
    }
    None
}

fn u32_field(value: &Value, keys: &[&str]) -> Option<u32> {
    for key in keys {
        let roh = value.get(*key);
        let gelesen = roh.and_then(Value::as_u64).or_else(|| {
            roh.and_then(Value::as_str)
                .and_then(|text| text.trim().parse::<u64>().ok())
        });
        if let Some(zahl) = gelesen {
            return u32::try_from(zahl).ok();
        }
    }
    None
}

/// Stufe laut Twitch (`sub_tier`), Default `1000`, Prime bleibt `Prime`.
fn tier_of(payload: &Value) -> String {
    if payload
        .get("is_prime")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return "Prime".to_string();
    }
    str_field(payload, &["sub_tier", "tier"]).unwrap_or_else(|| "1000".to_string())
}

/// Nachrichtentext einer `channel.chat.notification`, falls einer mitkam.
fn notification_text(event: &Value) -> Option<String> {
    event
        .get("message")
        .and_then(|message| message.get("text"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// Ausloeser einer `channel.chat.notification` (der Chatter).
fn chatter_actor(event: &Value) -> Option<Actor> {
    let id = str_field(event, &["chatter_user_id"])?;
    let login = str_field(event, &["chatter_user_login"]).unwrap_or_else(|| id.clone());
    let display = str_field(event, &["chatter_user_name"]).unwrap_or_else(|| login.clone());
    Some(Actor::new(id, login, display))
}

/// Kennzeichen fuer den Dedupe-Schluessel.
///
/// Die EventSub-Message-ID ist die beste Wahl, weil Twitch sie bei einer
/// Wiederzustellung beibehaelt. Fehlt sie, faellt der Schluessel auf den
/// Zeitpunkt zurueck; das entdoppelt dann nicht mehr, erzeugt aber auch keine
/// falsche Gleichheit zwischen zwei verschiedenen Ereignissen.
fn kennzeichen(message_id: Option<&str>, jetzt: DateTime<Utc>) -> String {
    message_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| jetzt.to_rfc3339())
}

/// Uebersetzt ein `channel.chat.message`-Event.
///
/// `sent_at` kommt vom Aufrufer: die EventSub-Nutzlast von
/// `channel.chat.message` traegt selbst keinen Zeitstempel, der steckt in den
/// Metadaten der Zustellung und ist an dieser Stelle nicht mehr greifbar. Die
/// Empfangszeit ist die einzige ehrliche Naeherung.
///
/// Bei Twitch Shared Chat gilt bewusst der **empfangende** Kanal
/// (`broadcaster_user_*`) und nicht der Quellkanal: das Dock zeigt den Chat, den
/// der Streamer in seinem eigenen Kanal sieht, und jeder Kanal der Session
/// bekommt seine eigene Zeile.
pub fn chat_message_zu_event(event: &Value, sent_at: DateTime<Utc>) -> Option<PlatformEvent> {
    let channel_id = str_field(event, &["broadcaster_user_id"])?;
    let message_id = str_field(event, &["message_id"])?;
    let sender_id = str_field(event, &["chatter_user_id"])?;
    let channel_login = str_field(event, &["broadcaster_user_login"])
        .or_else(|| str_field(event, &["broadcaster_user_name"]))
        .unwrap_or_else(|| channel_id.clone());
    let sender_login =
        str_field(event, &["chatter_user_login"]).unwrap_or_else(|| sender_id.clone());
    let sender_display =
        str_field(event, &["chatter_user_name"]).unwrap_or_else(|| sender_login.clone());

    let badges = event
        .get("badges")
        .and_then(Value::as_array)
        .map(|liste| liste.iter().filter_map(badge_lesen).collect::<Vec<_>>())
        .unwrap_or_default();

    let nachricht = event.get("message");
    let fragments = nachricht
        .and_then(|body| body.get("fragments"))
        .and_then(Value::as_array)
        .map(|liste| liste.iter().filter_map(fragment_lesen).collect::<Vec<_>>())
        .unwrap_or_default();
    // Ohne Fragmente (aeltere oder abgespeckte Nutzlast) bleibt der reine Text,
    // damit im Dock keine leere Zeile steht.
    let fragments = if fragments.is_empty() {
        match nachricht
            .and_then(|body| body.get("text"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            Some(text) => vec![Fragment::text(text)],
            None => Vec::new(),
        }
    } else {
        fragments
    };

    Some(PlatformEvent::Chat(ChatMessage {
        platform: Platform::Twitch,
        channel_id,
        channel_login,
        message_id,
        sender_id,
        sender_login,
        sender_display,
        color: str_field(event, &["color"]),
        badges,
        fragments,
        sent_at,
        // `channel.chat.message` unterscheidet `/me` nicht: `message_type`
        // kennt nur text, channel_points_*, user_intro und power_ups_*. Wer die
        // Auszeichnung will, braucht IRC-Tags, nicht dieses Event.
        is_action: false,
        reply_to: reply_lesen(event),
    }))
}

fn badge_lesen(roh: &Value) -> Option<Badge> {
    let set_id = str_field(roh, &["set_id"])?;
    let id = str_field(roh, &["id"]).unwrap_or_default();
    Some(Badge {
        set_id,
        id,
        info: str_field(roh, &["info"]),
        // Die Bild-URL steht nicht in der EventSub-Nutzlast; sie kaeme aus
        // /helix/chat/badges. Das Dock loest sie selbst auf, statt dass der Bot
        // je Nachricht eine Helix-Abfrage feuert.
        image_url: None,
    })
}

fn fragment_lesen(roh: &Value) -> Option<Fragment> {
    let text = roh
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let art = roh
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("text")
        .trim();
    match art {
        "emote" => {
            let emote = roh.get("emote")?;
            let emote_id = str_field(emote, &["id"])?;
            Some(Fragment::Emote {
                url_template: TWITCH_EMOTE_URL_TEMPLATE.replace("{id}", &emote_id),
                text,
                emote_id,
            })
        }
        "mention" => {
            let mention = roh.get("mention")?;
            let user_id = str_field(mention, &["user_id"])?;
            let user_login = str_field(mention, &["user_login"]).unwrap_or_else(|| user_id.clone());
            Some(Fragment::Mention {
                text,
                user_id,
                user_login,
            })
        }
        "cheermote" => {
            let cheermote = roh.get("cheermote")?;
            let prefix = str_field(cheermote, &["prefix"])?;
            let bits = u32_field(cheermote, &["bits"]).unwrap_or(0) as u64;
            let tier = u32_field(cheermote, &["tier"]).unwrap_or(1);
            Some(Fragment::Cheermote {
                // Cheermote-Bilder kommen aus /helix/bits/cheermotes und haengen
                // an Prefix und Stufe, nicht an einer ID in der Nutzlast. Das
                // Dock setzt das Muster selbst zusammen.
                url_template: String::new(),
                text,
                prefix,
                bits,
                tier,
            })
        }
        _ => Some(Fragment::text(text)),
    }
}

fn reply_lesen(event: &Value) -> Option<ReplyRef> {
    let reply = event.get("reply")?;
    let message_id = str_field(reply, &["parent_message_id"])?;
    let sender_id = str_field(reply, &["parent_user_id"]).unwrap_or_default();
    let sender_login = str_field(reply, &["parent_user_login"]).unwrap_or_default();
    let sender_display =
        str_field(reply, &["parent_user_name"]).unwrap_or_else(|| sender_login.clone());
    Some(ReplyRef {
        message_id,
        sender_id,
        sender_login,
        sender_display,
        text: str_field(reply, &["parent_message_body"]).unwrap_or_default(),
    })
}

/// Uebersetzt eine Sub/Resub/Gift-`channel.chat.notification`.
pub fn chat_subscription_zu_event(
    kind: ChatNotificationKind,
    event: &Value,
    message_id: Option<&str>,
    jetzt: DateTime<Utc>,
) -> Option<PlatformEvent> {
    let channel_id = str_field(event, &["broadcaster_user_id"])?;
    let kennzeichen = kennzeichen(message_id, jetzt);
    let actor = chatter_actor(event);

    let bauen = |art: &str| -> ActivityMeta {
        let meta = ActivityMeta::derived(
            Platform::Twitch,
            channel_id.clone(),
            jetzt,
            art,
            &kennzeichen,
        );
        match actor.clone() {
            Some(actor) => meta.with_actor(actor),
            None => meta,
        }
    };

    let ereignis = match kind {
        ChatNotificationKind::Sub => {
            let sub = event.get("sub")?;
            ActivityEvent::Subscribe {
                meta: bauen("subscribe"),
                tier: tier_of(sub),
                is_gift: false,
            }
        }
        ChatNotificationKind::Resub => {
            let resub = event.get("resub")?;
            ActivityEvent::Resub {
                meta: bauen("resub"),
                months: u32_field(resub, &["cumulative_months"]).unwrap_or(1),
                streak: u32_field(resub, &["streak_months"]),
                message: notification_text(event),
            }
        }
        ChatNotificationKind::SubGift => {
            let gift = event.get("sub_gift")?;
            ActivityEvent::SubGift {
                meta: bauen("sub_gift"),
                count: 1,
                tier: tier_of(gift),
            }
        }
        ChatNotificationKind::CommunitySubGift => {
            let gift = event.get("community_sub_gift")?;
            ActivityEvent::SubGift {
                meta: bauen("sub_gift"),
                count: u32_field(gift, &["total"]).unwrap_or(1),
                tier: tier_of(gift),
            }
        }
        // Raid und Unraid laufen ueber eigene Hooks, hier ist nichts zu tun.
        ChatNotificationKind::Raid | ChatNotificationKind::Unraid => return None,
    };
    Some(PlatformEvent::Activity(ereignis))
}

/// Uebersetzt eine `channel.chat.notification` mit `notice_type=raid`.
pub fn chat_raid_zu_event(
    event: &Value,
    message_id: Option<&str>,
    jetzt: DateTime<Utc>,
) -> Option<PlatformEvent> {
    let channel_id = str_field(event, &["broadcaster_user_id"])?;
    let raid = event.get("raid")?;
    let from_id = str_field(raid, &["user_id"])?;
    let from_login = str_field(raid, &["user_login"]).unwrap_or_else(|| from_id.clone());
    let from_display = str_field(raid, &["user_name"]).unwrap_or_else(|| from_login.clone());
    let meta = ActivityMeta::derived(
        Platform::Twitch,
        channel_id,
        jetzt,
        "raid",
        &kennzeichen(message_id, jetzt),
    )
    .with_actor(Actor::new(from_id, from_login.clone(), from_display));
    Some(PlatformEvent::Activity(ActivityEvent::Raid {
        meta,
        from: from_login,
        viewers: u32_field(raid, &["viewer_count"]).unwrap_or(0),
    }))
}

/// Uebersetzt ein `channel.raid`-Event.
///
/// Nur der eingehende Raid ist ein Dock-Ereignis. Ein ausgehender Raid gehoert
/// dem Zielkanal, nicht uns; er wuerde sonst im falschen Dock auftauchen.
pub fn channel_raid_zu_event(
    event: &Value,
    message_id: Option<&str>,
    jetzt: DateTime<Utc>,
) -> Option<PlatformEvent> {
    let channel_id = str_field(event, &["to_broadcaster_user_id"])?;
    let from_id = str_field(event, &["from_broadcaster_user_id"])?;
    let from_login =
        str_field(event, &["from_broadcaster_user_login"]).unwrap_or_else(|| from_id.clone());
    let from_display =
        str_field(event, &["from_broadcaster_user_name"]).unwrap_or_else(|| from_login.clone());
    let meta = ActivityMeta::derived(
        Platform::Twitch,
        channel_id,
        jetzt,
        "raid",
        &kennzeichen(message_id, jetzt),
    )
    .with_actor(Actor::new(from_id, from_login.clone(), from_display));
    Some(PlatformEvent::Activity(ActivityEvent::Raid {
        meta,
        from: from_login,
        viewers: u32_field(event, &["viewers", "viewer_count"]).unwrap_or(0),
    }))
}

/// Baut das Go-Live-Ereignis.
///
/// Kennzeichen ist die `stream_id`, wenn sie vorliegt: sie bleibt ueber eine
/// Wiederzustellung hinweg gleich und entdoppelt damit sauber.
pub fn stream_online_zu_event(
    twitch_user_id: &str,
    stream_id: Option<&str>,
    jetzt: DateTime<Utc>,
) -> Option<PlatformEvent> {
    let channel_id = nichtleer(twitch_user_id)?;
    Some(PlatformEvent::Activity(ActivityEvent::StreamOnline {
        meta: ActivityMeta::derived(
            Platform::Twitch,
            channel_id,
            jetzt,
            "stream_online",
            &kennzeichen(stream_id, jetzt),
        ),
    }))
}

/// Baut das Offline-Ereignis.
pub fn stream_offline_zu_event(
    twitch_user_id: &str,
    jetzt: DateTime<Utc>,
) -> Option<PlatformEvent> {
    let channel_id = nichtleer(twitch_user_id)?;
    Some(PlatformEvent::Activity(ActivityEvent::StreamOffline {
        meta: ActivityMeta::derived(
            Platform::Twitch,
            channel_id,
            jetzt,
            "stream_offline",
            &kennzeichen(None, jetzt),
        ),
    }))
}

fn nichtleer(wert: &str) -> Option<String> {
    let getrimmt = wert.trim();
    (!getrimmt.is_empty()).then(|| getrimmt.to_string())
}

// ---------------------------------------------------------------------------
// EventSubHooks-Wrapper
// ---------------------------------------------------------------------------

/// Haengt den Dock-Bus vor eine bestehende Hook-Kette.
pub fn wrap_eventsub_hooks(
    inner: Arc<dyn EventSubHooks>,
    sink: Arc<dyn ObsDockSink>,
) -> Arc<dyn EventSubHooks> {
    Arc::new(ObsDockHooks { inner, sink })
}

/// Schreibt jedes dock-taugliche Ereignis in den Bus und delegiert danach.
struct ObsDockHooks {
    inner: Arc<dyn EventSubHooks>,
    sink: Arc<dyn ObsDockSink>,
}

impl ObsDockHooks {
    /// Schreibt ein Ereignis; ein Fehler wird geloggt und sonst ignoriert.
    async fn melden(&self, ereignis: Option<PlatformEvent>) {
        let Some(ereignis) = ereignis else {
            return;
        };
        if let Err(error) = self.sink.write(&ereignis).await {
            tracing::warn!(
                %error,
                typ = ereignis.typ(),
                channel_id = ereignis.channel_id(),
                "obs_docks: Ereignis nicht in den Bus geschrieben"
            );
        }
    }
}

#[async_trait]
impl EventSubHooks for ObsDockHooks {
    async fn on_channel_raid(&self, event: &Value, message_id: Option<&str>) {
        self.melden(channel_raid_zu_event(event, message_id, Utc::now()))
            .await;
        self.inner.on_channel_raid(event, message_id).await;
    }

    async fn on_channel_moderate(&self, broadcaster_id: &str, login: &str, event: &Value) {
        self.inner
            .on_channel_moderate(broadcaster_id, login, event)
            .await;
    }

    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        self.melden(stream_online_zu_event(twitch_user_id, None, Utc::now()))
            .await;
        self.inner.on_stream_went_live(twitch_user_id, login).await;
    }

    /// Ausgeschrieben statt ueber die Default-Implementierung: die wuerde auf
    /// [`Self::on_stream_went_live`] zurueckfallen und dabei die `stream_id`
    /// verlieren, die der innere Hook (`ChatHooks`) auswertet.
    async fn on_stream_went_live_with_stream_id(
        &self,
        twitch_user_id: &str,
        login: &str,
        stream_id: Option<&str>,
    ) {
        self.melden(stream_online_zu_event(
            twitch_user_id,
            stream_id,
            Utc::now(),
        ))
        .await;
        self.inner
            .on_stream_went_live_with_stream_id(twitch_user_id, login, stream_id)
            .await;
    }

    async fn on_score_refresh(
        &self,
        twitch_user_id: &str,
        login: Option<&str>,
        trigger: &'static str,
    ) {
        self.inner
            .on_score_refresh(twitch_user_id, login, trigger)
            .await;
    }

    async fn on_stream_offline_engagement(&self, twitch_user_id: &str, login: Option<&str>) {
        self.inner
            .on_stream_offline_engagement(twitch_user_id, login)
            .await;
    }

    async fn on_stream_offline_global_ban(&self, twitch_user_id: &str, login: Option<&str>) {
        self.inner
            .on_stream_offline_global_ban(twitch_user_id, login)
            .await;
    }

    async fn on_stream_offline(&self, twitch_user_id: &str, login: Option<&str>) {
        self.melden(stream_offline_zu_event(twitch_user_id, Utc::now()))
            .await;
        self.inner.on_stream_offline(twitch_user_id, login).await;
    }

    async fn on_chat_message(&self, event: &Value, message_id: Option<&str>) {
        self.melden(chat_message_zu_event(event, Utc::now())).await;
        self.inner.on_chat_message(event, message_id).await;
    }

    async fn on_chat_subscription_notification(
        &self,
        kind: ChatNotificationKind,
        event: &Value,
        message_id: Option<&str>,
    ) {
        self.melden(chat_subscription_zu_event(
            kind,
            event,
            message_id,
            Utc::now(),
        ))
        .await;
        self.inner
            .on_chat_subscription_notification(kind, event, message_id)
            .await;
    }

    async fn on_chat_raid_notification(&self, event: &Value, message_id: Option<&str>) {
        self.melden(chat_raid_zu_event(event, message_id, Utc::now()))
            .await;
        self.inner
            .on_chat_raid_notification(event, message_id)
            .await;
    }

    async fn on_chat_unraid_notification(&self, event: &Value, message_id: Option<&str>) {
        self.inner
            .on_chat_unraid_notification(event, message_id)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;

    /// Zaehlt Schreibaufrufe und haelt die geschriebenen Ereignisse fest.
    #[derive(Default)]
    struct MerkendeSink {
        geschrieben: Mutex<Vec<PlatformEvent>>,
        /// `true` laesst jeden Schreibversuch scheitern.
        faellt_aus: bool,
    }

    #[async_trait]
    impl ObsDockSink for MerkendeSink {
        async fn write(&self, event: &PlatformEvent) -> Result<i64, ObsDockError> {
            self.geschrieben.lock().unwrap().push(event.clone());
            if self.faellt_aus {
                return Err(ObsDockError::Db(sqlx::Error::PoolClosed));
            }
            Ok(self.geschrieben.lock().unwrap().len() as i64)
        }
    }

    /// Innerer Hook: haelt fest, womit er aufgerufen wurde.
    #[derive(Default)]
    struct MerkendeHooks {
        chat_aufrufe: AtomicU64,
        chat_argumente: Mutex<Vec<(Value, Option<String>)>>,
        raids: AtomicU64,
        offline: AtomicU64,
        live_mit_stream_id: Mutex<Vec<(String, String, Option<String>)>>,
        unraid: AtomicU64,
    }

    #[async_trait]
    impl EventSubHooks for MerkendeHooks {
        async fn on_chat_message(&self, event: &Value, message_id: Option<&str>) {
            self.chat_aufrufe.fetch_add(1, Ordering::SeqCst);
            self.chat_argumente
                .lock()
                .unwrap()
                .push((event.clone(), message_id.map(str::to_string)));
        }
        async fn on_channel_raid(&self, _event: &Value, _message_id: Option<&str>) {
            self.raids.fetch_add(1, Ordering::SeqCst);
        }
        async fn on_stream_offline(&self, _twitch_user_id: &str, _login: Option<&str>) {
            self.offline.fetch_add(1, Ordering::SeqCst);
        }
        async fn on_stream_went_live_with_stream_id(
            &self,
            twitch_user_id: &str,
            login: &str,
            stream_id: Option<&str>,
        ) {
            self.live_mit_stream_id.lock().unwrap().push((
                twitch_user_id.to_string(),
                login.to_string(),
                stream_id.map(str::to_string),
            ));
        }
        async fn on_chat_unraid_notification(&self, _event: &Value, _message_id: Option<&str>) {
            self.unraid.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Eine vollstaendige `channel.chat.message`-Nutzlast, wie Twitch sie
    /// liefert (Feldnamen nach EventSub v1).
    fn chat_nutzlast() -> Value {
        json!({
            "broadcaster_user_id": "12345",
            "broadcaster_user_login": "earlysalty",
            "broadcaster_user_name": "EarlySalty",
            "chatter_user_id": "777",
            "chatter_user_login": "zuschauer",
            "chatter_user_name": "Zuschauer",
            "message_id": "abc-1",
            "message_type": "text",
            "color": "#1E90FF",
            "badges": [
                { "set_id": "subscriber", "id": "12", "info": "14" },
                { "set_id": "moderator", "id": "1", "info": "" }
            ],
            "message": {
                "text": "moin @earlysalty Kappa",
                "fragments": [
                    { "type": "text", "text": "moin " },
                    {
                        "type": "mention",
                        "text": "@earlysalty",
                        "mention": {
                            "user_id": "12345",
                            "user_login": "earlysalty",
                            "user_name": "EarlySalty"
                        }
                    },
                    { "type": "text", "text": " " },
                    {
                        "type": "emote",
                        "text": "Kappa",
                        "emote": { "id": "25", "emote_set_id": "0", "format": ["static"] }
                    }
                ]
            }
        })
    }

    fn zeitpunkt() -> DateTime<Utc> {
        "2026-08-23T20:15:00Z".parse().unwrap()
    }

    /// Beweisziel 1: ein simulierter `channel.chat.message`-Durchlauf erzeugt
    /// genau einen Schreibaufruf, und der innere Hook sieht unveraendert
    /// dieselben Argumente.
    ///
    /// Aufgerufen wird der Wrapper genau so, wie der Dispatcher ihn aufruft
    /// (`tb-monitoring/src/dispatch.rs:785`:
    /// `self.hooks.on_chat_message(&context.event, message_id)`). Der
    /// Dispatcher selbst braucht GuardStore, Inbox, Telemetrie und
    /// StreamerLoginStore und damit eine echte Postgres-Verbindung; ihn hier
    /// mitzustarten haette den Test an `TB_TEST_DATABASE_URL` gehaengt, ohne
    /// ueber den Wrapper mehr auszusagen.
    #[tokio::test]
    async fn chat_nachricht_erzeugt_genau_eine_zeile_und_delegiert_unveraendert() {
        let sink = Arc::new(MerkendeSink::default());
        let inner = Arc::new(MerkendeHooks::default());
        let hooks = wrap_eventsub_hooks(inner.clone(), sink.clone());

        let event = chat_nutzlast();
        hooks.on_chat_message(&event, Some("msg-1")).await;

        assert_eq!(
            sink.geschrieben.lock().unwrap().len(),
            1,
            "genau ein Schreibaufruf je Chatnachricht"
        );
        assert_eq!(inner.chat_aufrufe.load(Ordering::SeqCst), 1);
        let argumente = inner.chat_argumente.lock().unwrap();
        assert_eq!(argumente[0].0, event, "Event unveraendert durchgereicht");
        assert_eq!(argumente[0].1.as_deref(), Some("msg-1"));
    }

    /// Beweisziel 2: der Schreibpfad faellt aus, der innere Hook laeuft
    /// trotzdem.
    #[tokio::test]
    async fn fehler_im_schreibpfad_haelt_den_inneren_hook_nicht_auf() {
        let sink = Arc::new(MerkendeSink {
            geschrieben: Mutex::new(Vec::new()),
            faellt_aus: true,
        });
        let inner = Arc::new(MerkendeHooks::default());
        let hooks = wrap_eventsub_hooks(inner.clone(), sink.clone());

        let event = chat_nutzlast();
        hooks.on_chat_message(&event, Some("msg-1")).await;
        hooks
            .on_channel_raid(
                &json!({
                    "from_broadcaster_user_id": "999",
                    "from_broadcaster_user_login": "raider",
                    "to_broadcaster_user_id": "12345",
                    "viewers": 42
                }),
                Some("msg-2"),
            )
            .await;
        hooks.on_stream_offline("12345", Some("earlysalty")).await;

        assert_eq!(sink.geschrieben.lock().unwrap().len(), 3);
        assert_eq!(inner.chat_aufrufe.load(Ordering::SeqCst), 1);
        assert_eq!(inner.raids.load(Ordering::SeqCst), 1);
        assert_eq!(inner.offline.load(Ordering::SeqCst), 1);
    }

    /// Beweisziel 3: das erzeugte `payload`-JSON haelt sich an das eingefrorene
    /// Drahtformat (aeusserer Tag `typ`, innerer Tag `art`).
    #[tokio::test]
    async fn payload_json_entspricht_dem_eingefrorenen_drahtformat() {
        let sink = Arc::new(MerkendeSink::default());
        let hooks = wrap_eventsub_hooks(Arc::new(MerkendeHooks::default()), sink.clone());

        hooks.on_chat_message(&chat_nutzlast(), Some("msg-1")).await;

        let geschrieben = sink.geschrieben.lock().unwrap();
        let payload = serde_json::to_value(&geschrieben[0]).unwrap();

        assert_eq!(payload["typ"], "chat");
        assert_eq!(payload["platform"], "twitch");
        assert_eq!(payload["channel_id"], "12345");
        assert_eq!(payload["channel_login"], "earlysalty");
        assert_eq!(payload["message_id"], "abc-1");
        assert_eq!(payload["sender_login"], "zuschauer");
        assert_eq!(payload["sender_display"], "Zuschauer");
        assert_eq!(payload["color"], "#1E90FF");
        assert_eq!(payload["is_action"], false);

        assert_eq!(payload["badges"][0]["set_id"], "subscriber");
        assert_eq!(payload["badges"][0]["info"], "14");

        let fragmente = payload["fragments"].as_array().unwrap();
        assert_eq!(fragmente.len(), 4);
        assert_eq!(fragmente[0]["art"], "text");
        assert_eq!(fragmente[0]["text"], "moin ");
        assert_eq!(fragmente[1]["art"], "mention");
        assert_eq!(fragmente[1]["user_login"], "earlysalty");
        assert_eq!(fragmente[3]["art"], "emote");
        assert_eq!(fragmente[3]["emote_id"], "25");
        assert_eq!(
            fragmente[3]["url_template"],
            "https://static-cdn.jtvnw.net/emoticons/v2/25/{{format}}/{{theme_mode}}/{{scale}}"
        );

        // Rueckweg: das Dock muss dasselbe wieder einlesen koennen.
        let zurueck: PlatformEvent = serde_json::from_value(payload).unwrap();
        assert_eq!(&zurueck, &geschrieben[0]);
    }

    #[tokio::test]
    async fn go_live_reicht_die_stream_id_unveraendert_weiter() {
        let sink = Arc::new(MerkendeSink::default());
        let inner = Arc::new(MerkendeHooks::default());
        let hooks = wrap_eventsub_hooks(inner.clone(), sink.clone());

        hooks
            .on_stream_went_live_with_stream_id("12345", "earlysalty", Some("stream-7"))
            .await;

        let weitergereicht = inner.live_mit_stream_id.lock().unwrap();
        assert_eq!(weitergereicht.len(), 1);
        assert_eq!(
            weitergereicht[0],
            (
                "12345".to_string(),
                "earlysalty".to_string(),
                Some("stream-7".to_string())
            ),
            "die stream_id darf beim Durchreichen nicht verloren gehen"
        );

        let geschrieben = sink.geschrieben.lock().unwrap();
        assert_eq!(geschrieben.len(), 1);
        assert_eq!(
            geschrieben[0].dedupe_key().as_deref(),
            Some("twitch:12345:stream_online:stream-7")
        );
    }

    #[tokio::test]
    async fn unraid_schreibt_nichts_und_delegiert_trotzdem() {
        let sink = Arc::new(MerkendeSink::default());
        let inner = Arc::new(MerkendeHooks::default());
        let hooks = wrap_eventsub_hooks(inner.clone(), sink.clone());

        hooks
            .on_chat_unraid_notification(&json!({ "broadcaster_user_id": "12345" }), Some("m-1"))
            .await;

        assert!(sink.geschrieben.lock().unwrap().is_empty());
        assert_eq!(inner.unraid.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn eingehender_raid_gehoert_dem_zielkanal() {
        let event = json!({
            "from_broadcaster_user_id": "999",
            "from_broadcaster_user_login": "raider",
            "from_broadcaster_user_name": "Raider",
            "to_broadcaster_user_id": "12345",
            "to_broadcaster_user_login": "earlysalty",
            "viewers": 42
        });
        let ereignis = channel_raid_zu_event(&event, Some("m-9"), zeitpunkt()).unwrap();
        assert_eq!(ereignis.channel_id(), "12345");
        let payload = serde_json::to_value(&ereignis).unwrap();
        assert_eq!(payload["typ"], "activity");
        assert_eq!(payload["art"], "raid");
        assert_eq!(payload["from"], "raider");
        assert_eq!(payload["viewers"], 42);
        assert_eq!(payload["actor"]["login"], "raider");
        assert_eq!(payload["dedupe_key"], "twitch:12345:raid:m-9");
    }

    #[test]
    fn resub_uebernimmt_monate_streak_und_nachricht() {
        let event = json!({
            "broadcaster_user_id": "12345",
            "chatter_user_id": "777",
            "chatter_user_login": "zuschauer",
            "chatter_user_name": "Zuschauer",
            "notice_type": "resub",
            "message": { "text": "gerne weiter" },
            "resub": { "cumulative_months": 14, "streak_months": 3, "sub_tier": "2000" }
        });
        let ereignis = chat_subscription_zu_event(
            ChatNotificationKind::Resub,
            &event,
            Some("m-3"),
            zeitpunkt(),
        )
        .unwrap();
        let payload = serde_json::to_value(&ereignis).unwrap();
        assert_eq!(payload["typ"], "activity");
        assert_eq!(payload["art"], "resub");
        assert_eq!(payload["months"], 14);
        assert_eq!(payload["streak"], 3);
        assert_eq!(payload["message"], "gerne weiter");
        assert_eq!(payload["actor"]["login"], "zuschauer");
    }

    #[test]
    fn community_gift_zaehlt_die_geschenke() {
        let event = json!({
            "broadcaster_user_id": "12345",
            "chatter_user_id": "777",
            "chatter_user_login": "spender",
            "notice_type": "community_sub_gift",
            "community_sub_gift": { "total": 5, "sub_tier": "1000" }
        });
        let ereignis = chat_subscription_zu_event(
            ChatNotificationKind::CommunitySubGift,
            &event,
            Some("m-4"),
            zeitpunkt(),
        )
        .unwrap();
        let payload = serde_json::to_value(&ereignis).unwrap();
        assert_eq!(payload["art"], "sub_gift");
        assert_eq!(payload["count"], 5);
    }

    #[test]
    fn prime_abo_behaelt_seine_stufe() {
        let event = json!({
            "broadcaster_user_id": "12345",
            "chatter_user_id": "777",
            "chatter_user_login": "zuschauer",
            "notice_type": "sub",
            "sub": { "sub_tier": "1000", "is_prime": true, "duration_months": 1 }
        });
        let ereignis =
            chat_subscription_zu_event(ChatNotificationKind::Sub, &event, Some("m-5"), zeitpunkt())
                .unwrap();
        let payload = serde_json::to_value(&ereignis).unwrap();
        assert_eq!(payload["tier"], "Prime");
        assert_eq!(payload["is_gift"], false);
    }

    #[test]
    fn nachricht_ohne_fragmente_behaelt_den_text() {
        let event = json!({
            "broadcaster_user_id": "12345",
            "broadcaster_user_login": "earlysalty",
            "chatter_user_id": "777",
            "chatter_user_login": "zuschauer",
            "message_id": "abc-2",
            "message": { "text": "nur text" }
        });
        let ereignis = chat_message_zu_event(&event, zeitpunkt()).unwrap();
        let payload = serde_json::to_value(&ereignis).unwrap();
        assert_eq!(payload["fragments"][0]["art"], "text");
        assert_eq!(payload["fragments"][0]["text"], "nur text");
    }

    #[test]
    fn nachricht_ohne_pflichtfelder_wird_verworfen() {
        assert!(chat_message_zu_event(&json!({ "message_id": "x" }), zeitpunkt()).is_none());
        assert!(channel_raid_zu_event(&json!({}), None, zeitpunkt()).is_none());
        assert!(stream_online_zu_event("  ", None, zeitpunkt()).is_none());
        assert!(stream_offline_zu_event("", zeitpunkt()).is_none());
    }

    #[test]
    fn notify_nutzlast_traegt_genau_zwei_felder() {
        assert_eq!(
            notify_nutzlast("12345", 42),
            r#"{"channel_id":"12345","id":42}"#
        );
    }

    #[test]
    fn config_ist_ohne_datei_aus() {
        let pfad = std::env::temp_dir().join("obs-dock-gibt-es-nicht-12345.json");
        assert_eq!(
            ObsDocksConfig::aus_datei(&pfad),
            ObsDocksConfig { enabled: false }
        );
    }

    #[test]
    fn config_liest_den_schalter_aus_der_datei() {
        let pfad =
            std::env::temp_dir().join(format!("obs-dock-config-{}.json", std::process::id()));
        std::fs::write(
            &pfad,
            r#"{ "irgendwas_anderes": { "a": 1 }, "obs_docks": { "enabled": true } }"#,
        )
        .unwrap();
        let gelesen = ObsDocksConfig::aus_datei(&pfad);
        let _ = std::fs::remove_file(&pfad);
        assert_eq!(gelesen, ObsDocksConfig { enabled: true });
    }

    #[test]
    fn kaputte_config_laesst_den_bus_aus() {
        let pfad =
            std::env::temp_dir().join(format!("obs-dock-kaputt-{}.json", std::process::id()));
        std::fs::write(&pfad, "{ das ist kein json").unwrap();
        let gelesen = ObsDocksConfig::aus_datei(&pfad);
        let _ = std::fs::remove_file(&pfad);
        assert_eq!(gelesen, ObsDocksConfig { enabled: false });
    }

    // -----------------------------------------------------------------------
    // Datenbankgestuetzt: laeuft nur mit TB_TEST_DATABASE_URL, sonst SKIP.
    // Beweist das SQL selbst (Migration, genau eine Zeile, NOTIFY, Retention).
    // -----------------------------------------------------------------------

    /// DDL genau aus der Migration, damit Test und Produktion nicht auseinander
    /// laufen koennen.
    const MIGRATION: &str = include_str!("../../../migrations/20260824090000_obs_dock_events.sql");

    async fn test_pool() -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&dsn)
            .await
            .expect("Verbindung zur Test-Datenbank");
        sqlx::query("DROP TABLE IF EXISTS obs_dock_events")
            .execute(&pool)
            .await
            .unwrap();
        for anweisung in migration_anweisungen(MIGRATION) {
            sqlx::query(&anweisung).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    /// Zerlegt die Migration in ausfuehrbare Anweisungen.
    ///
    /// Erst die Kommentarzeilen entfernen, dann an `;` trennen: sonst
    /// verschluckt der Kommentarkopf der Datei die erste Anweisung mit, weil
    /// das erste Stueck dann mit `--` beginnt.
    ///
    /// Die Zerlegung ist absichtlich stumpf und kennt keine Zeichenketten. Der
    /// Test unten haelt die Anzahl der Anweisungen fest und faellt um, sobald
    /// jemand ein `;` in einen `COMMENT`-Text schreibt.
    fn migration_anweisungen(sql: &str) -> Vec<String> {
        let ohne_kommentar = sql
            .lines()
            .filter(|zeile| !zeile.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        ohne_kommentar
            .split(';')
            .map(str::trim)
            .filter(|teil| !teil.is_empty())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn migration_zerlegt_sich_in_ihre_anweisungen() {
        let anweisungen = migration_anweisungen(MIGRATION);
        assert_eq!(
            anweisungen.len(),
            5,
            "Tabelle, Index und drei COMMENT-Anweisungen: {anweisungen:#?}"
        );
        assert!(anweisungen[0].starts_with("CREATE TABLE IF NOT EXISTS obs_dock_events"));
        assert!(anweisungen[1].contains("obs_dock_events_channel_id_id_idx"));
    }

    #[tokio::test]
    async fn db_schreibpfad_legt_genau_eine_zeile_an() {
        let Some(pool) = test_pool().await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let sink = PgObsDockSink::new(pool.clone());
        let ereignis = chat_message_zu_event(&chat_nutzlast(), zeitpunkt()).unwrap();
        let id = sink.write(&ereignis).await.unwrap();

        let (anzahl,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM obs_dock_events WHERE channel_id = $1")
                .bind("12345")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(anzahl, 1);

        let (payload,): (Value,) =
            sqlx::query_as("SELECT payload FROM obs_dock_events WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(payload["typ"], "chat");

        let geloescht =
            cleanup_obs_dock_events_before(&pool, Utc::now() + chrono::Duration::minutes(1))
                .await
                .unwrap();
        assert_eq!(geloescht, 1);
    }
}
