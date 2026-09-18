//! IRCv3 message parser shared by the engagement adapter and anonymous collectors.
use std::collections::HashMap;

/// Eine geparste IRC-PRIVMSG mit IRCv3-Tags.
#[derive(Debug, Clone)]
pub struct ParsedPrivmsg {
    pub tags: HashMap<String, String>,
    pub login: String,
    pub channel: String,
    pub text: String,
}

/// Parst IRCv3-Tags (`key=value;key2=value2`).
pub fn parse_tags(raw: &str) -> HashMap<String, String> {
    raw.split(';')
        .filter_map(|kv| kv.split_once('='))
        .map(|(key, value)| (key.to_string(), unescape_tag(value)))
        .collect()
}

/// Zerlegt `@tags :nick!user@host PRIVMSG #channel :text`.
pub fn parse_privmsg(line: &str) -> Option<ParsedPrivmsg> {
    let (tags, rest) = if let Some(stripped) = line.strip_prefix('@') {
        let (tag_part, rest) = stripped.split_once(' ')?;
        (parse_tags(tag_part), rest)
    } else {
        (HashMap::new(), line)
    };
    let rest = rest.strip_prefix(':')?;
    let (prefix, after) = rest.split_once(' ')?;
    let login = prefix.split('!').next()?.to_string();
    let after = after.strip_prefix("PRIVMSG #")?;
    let (channel, text) = after.split_once(' ')?;
    Some(ParsedPrivmsg {
        tags,
        login,
        channel: channel.to_string(),
        text: text.strip_prefix(':')?.to_string(),
    })
}


fn unescape_tag(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' { result.push(ch); continue; }
        if let Some(escaped) = chars.next() {
            result.push(match escaped { ':' => ';', 's' => ' ', 'r' => '\r', 'n' => '\n', other => other });
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tags_are_unescaped_without_changing_message_content() {
        let msg = parse_privmsg(r"@display-name=A\sB;badges=mod/1 :x!x@x PRIVMSG #room :hello \s world").unwrap();
        assert_eq!(msg.tags["display-name"], "A B");
        assert_eq!(msg.text, r"hello \s world");
    }
}
