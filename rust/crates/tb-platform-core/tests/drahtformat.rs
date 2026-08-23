//! Abnahme des Drahtformats.
//!
//! Zwei Dinge werden hier festgenagelt:
//!
//! 1. Jede Variante von [`PlatformEvent`], jede Variante von [`ActivityEvent`]
//!    und jede Variante von [`Fragment`] ueberlebt serialisieren und wieder
//!    einlesen unveraendert.
//! 2. Der JSON-Tag heisst `typ` und die Namen der Varianten stehen fest. Aendert
//!    jemand daran etwas, faellt der Test um, denn ein aelteres Dock-Bundle
//!    liest sonst nichts mehr mit.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use tb_platform_core::{
    ActivityEvent, ActivityMeta, Actor, Badge, ChatMessage, Fragment, Platform, PlatformEvent,
    ReplyRef, StreamInfo,
};

fn zeitpunkt() -> DateTime<Utc> {
    "2026-08-23T20:15:00Z".parse().unwrap()
}

fn meta(art: &str, kennzeichen: &str) -> ActivityMeta {
    ActivityMeta::derived(Platform::Twitch, "12345", zeitpunkt(), art, kennzeichen)
}

fn meta_mit_actor(art: &str, kennzeichen: &str) -> ActivityMeta {
    meta(art, kennzeichen).with_actor(Actor::new("777", "zuschauer", "Zuschauer"))
}

/// Eine schlichte Chatnachricht ohne Sonderfaelle.
fn schlichte_nachricht() -> ChatMessage {
    ChatMessage {
        platform: Platform::Twitch,
        channel_id: "12345".into(),
        channel_login: "earlysalty".into(),
        message_id: "msg-1".into(),
        sender_id: "777".into(),
        sender_login: "zuschauer".into(),
        sender_display: "Zuschauer".into(),
        color: None,
        badges: Vec::new(),
        fragments: vec![Fragment::text("moin")],
        sent_at: zeitpunkt(),
        is_action: false,
        reply_to: None,
    }
}

/// Alle vier Fragmentarten in einer Nachricht, dazu Badges, Farbe und Reply.
fn volle_nachricht() -> ChatMessage {
    ChatMessage {
        platform: Platform::Kick,
        channel_id: "kanal-9".into(),
        channel_login: "earlysalty".into(),
        message_id: "msg-2".into(),
        sender_id: "778".into(),
        sender_login: "stammgast".into(),
        sender_display: "Stammgast".into(),
        color: Some("#D4AF37".into()),
        badges: vec![
            Badge::new("moderator", "1"),
            Badge {
                set_id: "subscriber".into(),
                id: "12".into(),
                info: Some("14".into()),
                image_url: Some("https://example.invalid/badge.png".into()),
            },
        ],
        fragments: vec![
            Fragment::text("moin "),
            Fragment::Emote {
                text: "Kappa".into(),
                emote_id: "25".into(),
                url_template: "https://example.invalid/emote/{{format}}".into(),
            },
            Fragment::Mention {
                text: "@earlysalty".into(),
                user_id: "12345".into(),
                user_login: "earlysalty".into(),
            },
            Fragment::Cheermote {
                text: "Cheer100".into(),
                prefix: "Cheer".into(),
                bits: 100,
                tier: 1,
                url_template: "https://example.invalid/cheer/{{scale}}".into(),
            },
        ],
        sent_at: zeitpunkt(),
        is_action: true,
        reply_to: Some(ReplyRef {
            message_id: "msg-0".into(),
            sender_id: "12345".into(),
            sender_login: "earlysalty".into(),
            sender_display: "EarlySalty".into(),
            text: "und?".into(),
        }),
    }
}

/// Alle neun Ereignisarten, jede einmal.
fn alle_aktivitaeten() -> Vec<ActivityEvent> {
    vec![
        ActivityEvent::Follow {
            meta: meta_mit_actor("follow", "m-1"),
        },
        ActivityEvent::Subscribe {
            meta: meta_mit_actor("subscribe", "m-2"),
            tier: "1000".into(),
            is_gift: false,
        },
        ActivityEvent::Resub {
            meta: meta_mit_actor("resub", "m-3"),
            months: 14,
            streak: Some(9),
            message: Some("weiter so".into()),
        },
        ActivityEvent::SubGift {
            meta: meta_mit_actor("sub_gift", "m-4"),
            count: 5,
            tier: "2000".into(),
        },
        ActivityEvent::Cheer {
            meta: meta_mit_actor("cheer", "m-5"),
            bits: 1000,
            message: None,
        },
        ActivityEvent::Raid {
            meta: meta_mit_actor("raid", "m-6"),
            from: "anderer_kanal".into(),
            viewers: 42,
        },
        ActivityEvent::StreamOnline {
            meta: meta("stream_online", "m-7"),
        },
        ActivityEvent::StreamOffline {
            meta: meta("stream_offline", "m-8"),
        },
        ActivityEvent::ChannelUpdate {
            meta: meta("channel_update", "m-9"),
            title: "Deadlock Ranked".into(),
            category: "Deadlock".into(),
        },
    ]
}

/// Jede Momentaufnahme, live und offline.
fn alle_infos() -> Vec<StreamInfo> {
    vec![
        StreamInfo {
            platform: Platform::Twitch,
            channel_id: "12345".into(),
            title: "Deadlock Ranked".into(),
            category_id: Some("1922780024".into()),
            category_name: Some("Deadlock".into()),
            tags: vec!["Deutsch".into(), "Deadlock".into()],
            is_live: true,
            started_at: Some(zeitpunkt()),
            viewers: Some(128),
        },
        StreamInfo::offline(Platform::YouTube, "kanal-y", "Pause"),
    ]
}

/// Jedes Ereignis, das ueber die Leitung gehen kann.
fn alle_ereignisse() -> Vec<PlatformEvent> {
    let mut ereignisse = vec![
        PlatformEvent::Chat(schlichte_nachricht()),
        PlatformEvent::Chat(volle_nachricht()),
    ];
    ereignisse.extend(alle_aktivitaeten().into_iter().map(PlatformEvent::Activity));
    ereignisse.extend(alle_infos().into_iter().map(PlatformEvent::Info));
    ereignisse
}

#[test]
fn jede_ereignisvariante_ueberlebt_den_roundtrip() {
    for ereignis in alle_ereignisse() {
        let json = serde_json::to_string(&ereignis).expect("serialisieren");
        let zurueck: PlatformEvent = serde_json::from_str(&json).expect(&json);
        assert_eq!(ereignis, zurueck, "Roundtrip kaputt fuer {json}");
    }
}

#[test]
fn roundtrip_deckt_jede_aktivitaetsart_ab() {
    let erwartet: BTreeSet<&str> = [
        "follow",
        "subscribe",
        "resub",
        "sub_gift",
        "cheer",
        "raid",
        "stream_online",
        "stream_offline",
        "channel_update",
    ]
    .into_iter()
    .collect();

    let abgedeckt: BTreeSet<&str> = alle_aktivitaeten().iter().map(ActivityEvent::art).collect();
    assert_eq!(
        abgedeckt, erwartet,
        "neue oder umbenannte Aktivitaetsart ohne Roundtrip-Test"
    );

    // Jede Art einzeln, nicht nur als Teil der Sammelschleife.
    for ereignis in alle_aktivitaeten() {
        let json = serde_json::to_string(&ereignis).expect("serialisieren");
        let zurueck: ActivityEvent = serde_json::from_str(&json).expect(&json);
        assert_eq!(ereignis, zurueck, "Roundtrip kaputt fuer {json}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap()["art"],
            serde_json::json!(ereignis.art())
        );
    }
}

#[test]
fn roundtrip_deckt_jede_fragmentart_ab() {
    let erwartet: BTreeSet<&str> = ["text", "emote", "mention", "cheermote"]
        .into_iter()
        .collect();
    let abgedeckt: BTreeSet<&str> = volle_nachricht()
        .fragments
        .iter()
        .map(Fragment::art)
        .collect();
    assert_eq!(
        abgedeckt, erwartet,
        "neue oder umbenannte Fragmentart ohne Roundtrip-Test"
    );

    for fragment in volle_nachricht().fragments {
        let json = serde_json::to_string(&fragment).expect("serialisieren");
        let zurueck: Fragment = serde_json::from_str(&json).expect(&json);
        assert_eq!(fragment, zurueck, "Roundtrip kaputt fuer {json}");
    }
}

#[test]
fn der_tag_heisst_typ_und_die_variantennamen_stehen_fest() {
    let paare = [
        (PlatformEvent::Chat(schlichte_nachricht()), "chat"),
        (
            PlatformEvent::Activity(ActivityEvent::Follow {
                meta: meta("follow", "m-1"),
            }),
            "activity",
        ),
        (
            PlatformEvent::Info(StreamInfo::offline(Platform::Twitch, "12345", "Pause")),
            "info",
        ),
    ];

    for (ereignis, erwarteter_typ) in paare {
        let wert: serde_json::Value = serde_json::to_value(&ereignis).expect("serialisieren");
        assert_eq!(
            wert["typ"],
            serde_json::json!(erwarteter_typ),
            "Drahtformat-Tag verschoben"
        );
        assert_eq!(ereignis.typ(), erwarteter_typ);
    }
}

#[test]
fn chat_json_ist_woertlich_eingefroren() {
    let json =
        serde_json::to_string(&PlatformEvent::Chat(schlichte_nachricht())).expect("serialisieren");
    assert_eq!(
        json,
        r#"{"typ":"chat","platform":"twitch","channel_id":"12345","channel_login":"earlysalty","message_id":"msg-1","sender_id":"777","sender_login":"zuschauer","sender_display":"Zuschauer","badges":[],"fragments":[{"art":"text","text":"moin"}],"sent_at":"2026-08-23T20:15:00Z","is_action":false}"#
    );
}

#[test]
fn aktivitaets_json_ist_woertlich_eingefroren() {
    let ereignis = PlatformEvent::Activity(ActivityEvent::Raid {
        meta: meta_mit_actor("raid", "m-6"),
        from: "anderer_kanal".into(),
        viewers: 42,
    });
    let json = serde_json::to_string(&ereignis).expect("serialisieren");
    assert_eq!(
        json,
        r#"{"typ":"activity","art":"raid","platform":"twitch","channel_id":"12345","occurred_at":"2026-08-23T20:15:00Z","dedupe_key":"twitch:12345:raid:m-6","actor":{"id":"777","login":"zuschauer","display":"Zuschauer"},"from":"anderer_kanal","viewers":42}"#
    );
}

#[test]
fn info_json_ist_woertlich_eingefroren() {
    let json = serde_json::to_string(&PlatformEvent::Info(StreamInfo::offline(
        Platform::YouTube,
        "kanal-y",
        "Pause",
    )))
    .expect("serialisieren");
    assert_eq!(
        json,
        r#"{"typ":"info","platform":"youtube","channel_id":"kanal-y","title":"Pause","tags":[],"is_live":false}"#
    );
}

#[test]
fn dedupe_schluessel_ist_stabil_und_unterscheidet_ereignisse() {
    // Gleiches Ereignis, zweimal gebaut: gleicher Schluessel.
    let einmal = ActivityEvent::Follow {
        meta: meta("follow", "m-1"),
    };
    let nochmal = ActivityEvent::Follow {
        meta: meta("follow", "m-1"),
    };
    assert_eq!(einmal.dedupe_key(), nochmal.dedupe_key());

    // Auch nach dem Weg ueber die Leitung: gleicher Schluessel.
    let json = serde_json::to_string(&einmal).expect("serialisieren");
    let zurueck: ActivityEvent = serde_json::from_str(&json).expect("einlesen");
    assert_eq!(einmal.dedupe_key(), zurueck.dedupe_key());

    // Zwei verschiedene Ereignisse: verschiedene Schluessel, paarweise geprueft.
    let mut schluessel = BTreeSet::new();
    let mut anzahl = 0;
    for ereignis in alle_aktivitaeten() {
        anzahl += 1;
        assert!(
            schluessel.insert(ereignis.dedupe_key().to_string()),
            "doppelter Dedupe-Schluessel bei {}",
            ereignis.art()
        );
    }
    assert_eq!(schluessel.len(), anzahl);

    // Auch Chatnachrichten reihen sich in dieselbe Systematik ein.
    assert_eq!(
        schlichte_nachricht().dedupe_key(),
        schlichte_nachricht().dedupe_key()
    );
    assert_ne!(
        schlichte_nachricht().dedupe_key(),
        volle_nachricht().dedupe_key()
    );
    assert!(!schluessel.contains(&schlichte_nachricht().dedupe_key()));
}

#[test]
fn ereignis_kennt_seinen_kanal_und_seine_plattform() {
    for ereignis in alle_ereignisse() {
        assert!(!ereignis.channel_id().is_empty());
        match &ereignis {
            PlatformEvent::Info(_) => assert!(ereignis.dedupe_key().is_none()),
            _ => assert!(ereignis.dedupe_key().is_some()),
        }
    }

    let info = PlatformEvent::Info(StreamInfo::offline(Platform::Kick, "kanal-k", "Pause"));
    assert_eq!(info.platform(), Platform::Kick);
    assert_eq!(info.channel_id(), "kanal-k");
}
