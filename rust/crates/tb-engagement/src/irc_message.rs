pub use tb_transport_twitch::irc_message::{parse_tags, parse_privmsg, ParsedPrivmsg};
use crate::types::IncomingMessage;

/// Baut die Engagement-Nachricht und filtert leere/eigene/Bot-Nachrichten.
pub fn build_incoming(parsed: &ParsedPrivmsg, self_login: &str) -> Option<IncomingMessage> {
    let login = parsed.login.trim().to_lowercase();
    let channel = parsed.channel.trim().to_lowercase();
    let text = parsed.text.trim().to_string();
    if login.is_empty()
        || channel.is_empty()
        || text.is_empty()
        || login == self_login
        || is_known_chat_bot(&login)
    {
        return None;
    }
    let room_id = parsed.tags.get("room-id")?.trim();
    let user_id = parsed.tags.get("user-id")?.trim();
    if room_id.is_empty() || user_id.is_empty() {
        return None;
    }
    Some(IncomingMessage {
        channel_login: channel,
        channel_user_id: room_id.to_string(),
        twitch_user_id: user_id.to_string(),
        twitch_login: login,
        content: text,
        message_id: parsed
            .tags
            .get("id")
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty()),
    })
}

fn is_known_chat_bot(login: &str) -> bool {
    tb_analytics::bekannte_bots::ist_ausgeschlossener_login(login)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privmsg_mit_vollstaendigen_tags() {
        let parsed = parse_privmsg(
            "@room-id=99;user-id=42;id=m1;tmi-sent-ts=1784138400123 \
             :viewer!viewer@viewer.tmi.twitch.tv PRIVMSG #nani :hallo welt",
        )
        .expect("PRIVMSG muss geparst werden");

        assert_eq!(parsed.login, "viewer");
        assert_eq!(parsed.channel, "nani");
        assert_eq!(parsed.text, "hallo welt");
        assert_eq!(parsed.tags.get("room-id").map(String::as_str), Some("99"));
        assert_eq!(parsed.tags.get("user-id").map(String::as_str), Some("42"));
        assert_eq!(parsed.tags.get("id").map(String::as_str), Some("m1"));
        assert_eq!(
            parsed.tags.get("tmi-sent-ts").map(String::as_str),
            Some("1784138400123")
        );
    }

    /// Twitch schickt die Kanal-ID als `room-id` an jeder Nachricht mit. Sie
    /// hier zu behalten ist der billigste Weg zur stabilen Identität: der
    /// Alternativweg wäre, den Kanal später aus seinem Namen zurückzurechnen —
    /// teurer und nur so gut wie die Namensauflösung.
    #[test]
    fn behaelt_die_kanal_id_aus_der_room_id() {
        let parsed = parse_privmsg(
            "@room-id=520300019;user-id=42;id=m1 \
             :viewer!viewer@viewer.tmi.twitch.tv PRIVMSG #coolysdl :hallo welt",
        )
        .expect("PRIVMSG muss geparst werden");
        let msg = build_incoming(&parsed, "botname").expect("Nachricht muss gebaut werden");

        assert_eq!(msg.channel_login, "coolysdl");
        assert_eq!(msg.channel_user_id, "520300019", "Kanal-ID aus room-id");
        assert_eq!(msg.twitch_user_id, "42", "user-id bleibt der Chatter");
    }

    /// Ohne Kanal-ID ist die Nachricht unbrauchbar — das war schon vorher so,
    /// bleibt aber wichtig: sonst käme eine leere ID in die Lesepfade.
    #[test]
    fn verwirft_nachricht_ohne_room_id() {
        let parsed =
            parse_privmsg("@user-id=42;id=m1 :viewer!v@v PRIVMSG #coolysdl :hallo").expect("parse");
        assert!(build_incoming(&parsed, "botname").is_none());
    }
}
