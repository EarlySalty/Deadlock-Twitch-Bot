//! Read-only command targets. A mention never changes the sender or channel:
//! permissions, command settings and cooldowns always belong to the original event.
use crate::{api::ChatApi, types::ChatMessageEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandTarget {
    pub user_id: String,
    pub login: String,
    pub name: String,
}

#[derive(Clone, Copy)]
pub(crate) enum DefaultTarget {
    Broadcaster,
    Chatter,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TargetError {
    Invalid,
    MissingId,
    NotFound(String),
    Unavailable,
}

impl TargetError {
    pub fn reply(&self) -> String {
        match self {
            Self::Invalid => "Bitte genau einen Twitch-Namen angeben, z. B. @username.".into(),
            Self::MissingId => {
                "Die Twitch-ID kann ich gerade nicht zuordnen. Versuch es später nochmal.".into()
            }
            Self::NotFound(login) => {
                format!("Den Twitch-Account @{login} habe ich nicht gefunden.")
            }
            Self::Unavailable => {
                "Den Twitch-Namen kann ich gerade nicht auflösen. Versuch es gleich nochmal.".into()
            }
        }
    }
}

pub(crate) fn parse_login(args: &str) -> Result<Option<String>, TargetError> {
    let args = args.trim();
    if args.is_empty() {
        return Ok(None);
    }
    // Accept punctuation commonly inserted after a tab-completed mention.
    let login = args
        .strip_prefix('@')
        .unwrap_or(args)
        .trim_end_matches([',', ':']);
    if login.is_empty()
        || login.len() > 25
        || !login
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err(TargetError::Invalid);
    }
    Ok(Some(login.to_ascii_lowercase()))
}

fn target(id: &str, login: &str, name: &str) -> Result<CommandTarget, TargetError> {
    if id.trim().is_empty() {
        return Err(TargetError::MissingId);
    }
    Ok(CommandTarget {
        user_id: id.trim().into(),
        login: login.to_ascii_lowercase(),
        name: if name.trim().is_empty() { login } else { name }.into(),
    })
}

fn from_event(
    event: &ChatMessageEvent,
    login: Option<&str>,
    default: DefaultTarget,
) -> Option<Result<CommandTarget, TargetError>> {
    let broadcaster = || {
        target(
            &event.broadcaster_user_id,
            &event.broadcaster_user_login,
            &event.broadcaster_user_name,
        )
    };
    let chatter = || {
        target(
            &event.chatter_user_id,
            &event.chatter_user_login,
            &event.chatter_user_name,
        )
    };
    let Some(login) = login else {
        return Some(match default {
            DefaultTarget::Broadcaster => broadcaster(),
            DefaultTarget::Chatter => chatter(),
        });
    };
    if login.eq_ignore_ascii_case(&event.broadcaster_user_login) {
        return Some(broadcaster());
    }
    if login.eq_ignore_ascii_case(&event.chatter_user_login) {
        return Some(chatter());
    }
    event
        .message
        .fragments
        .iter()
        .filter(|fragment| fragment.fragment_type == "mention")
        .filter_map(|fragment| fragment.mention.as_ref())
        .find(|mention| {
            mention.user_login.eq_ignore_ascii_case(login) && !mention.user_id.trim().is_empty()
        })
        .map(|mention| target(&mention.user_id, login, login))
}

pub(crate) async fn resolve(
    api: &dyn ChatApi,
    event: &ChatMessageEvent,
    args: &str,
    default: DefaultTarget,
) -> Result<CommandTarget, TargetError> {
    let login = parse_login(args)?;
    if let Some(result) = from_event(event, login.as_deref(), default) {
        return result;
    }
    let login = login.ok_or(TargetError::Invalid)?;
    // A bounded Helix lookup also supports IRC messages without mention fragments.
    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        api.resolve_user_id(&login),
    )
    .await
    {
        Ok(Ok(Some(id))) => target(&id, &login, &login),
        Ok(Ok(None)) => Err(TargetError::NotFound(login)),
        _ => Err(TargetError::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MentionRef, MessageFragment};

    #[test]
    fn parses_exactly_one_login() {
        for input in ["User_123", " @User_123 ", "@User_123,", "@User_123:"] {
            assert_eq!(parse_login(input), Ok(Some("user_123".into())));
        }
        assert_eq!(parse_login("  "), Ok(None));
        for input in [
            "@",
            "@@user",
            "@one @two",
            "@one\n@two",
            "user/name",
            "ümlaut",
            "https://twitch.tv/foo",
            "aaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert_eq!(parse_login(input), Err(TargetError::Invalid), "{input}");
        }
    }

    fn event() -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "100".into(),
            broadcaster_user_login: "channel".into(),
            broadcaster_user_name: "Channel".into(),
            chatter_user_id: "200".into(),
            chatter_user_login: "viewer".into(),
            ..Default::default()
        }
    }

    #[test]
    fn defaults_and_explicit_self_keep_stable_ids() {
        let event = event();
        assert_eq!(
            from_event(&event, None, DefaultTarget::Broadcaster)
                .unwrap()
                .unwrap()
                .user_id,
            "100"
        );
        assert_eq!(
            from_event(&event, None, DefaultTarget::Chatter)
                .unwrap()
                .unwrap()
                .user_id,
            "200"
        );
        assert_eq!(
            from_event(&event, Some("VIEWER"), DefaultTarget::Broadcaster)
                .unwrap()
                .unwrap()
                .name,
            "viewer"
        );
        assert_eq!(
            from_event(&event, Some("CHANNEL"), DefaultTarget::Chatter)
                .unwrap()
                .unwrap()
                .user_id,
            "100"
        );
    }

    #[test]
    fn mention_must_match_requested_login_not_first_mention() {
        let mut event = event();
        for (login, id) in [("unrelated", "300"), ("requested", "400")] {
            event.message.fragments.push(MessageFragment {
                fragment_type: "mention".into(),
                text: format!("@{login}"),
                mention: Some(MentionRef {
                    user_id: id.into(),
                    user_login: login.into(),
                }),
            });
        }
        let result = from_event(&event, Some("REQUESTED"), DefaultTarget::Chatter)
            .unwrap()
            .unwrap();
        assert_eq!(result.user_id, "400");
        assert_eq!(event.chatter_user_id, "200");
        assert!(from_event(&event, Some("unknown"), DefaultTarget::Chatter).is_none());
    }
}
