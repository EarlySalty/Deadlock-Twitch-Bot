use serde::Deserialize;
use serde_json::{json, Value};

pub const GATEWAY_VERSION: u8 = 10;

pub const INTENT_GUILDS: u64 = 1 << 0;
pub const INTENT_GUILD_MEMBERS: u64 = 1 << 1;
pub const INTENTS: u64 = INTENT_GUILDS | INTENT_GUILD_MEMBERS;

pub mod op {
    pub const DISPATCH: u8 = 0;
    pub const HEARTBEAT: u8 = 1;
    pub const IDENTIFY: u8 = 2;
    pub const RESUME: u8 = 6;
    pub const RECONNECT: u8 = 7;
    pub const INVALID_SESSION: u8 = 9;
    pub const HELLO: u8 = 10;
    pub const HEARTBEAT_ACK: u8 = 11;
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GatewayPayload {
    pub op: u8,
    #[serde(default)]
    pub d: Value,
    #[serde(default)]
    pub s: Option<u64>,
    #[serde(default)]
    pub t: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("ungültiges Gateway-Payload: {0}")]
pub struct PayloadError(#[from] serde_json::Error);

pub fn parse_payload(text: &str) -> Result<GatewayPayload, PayloadError> {
    Ok(serde_json::from_str(text)?)
}

pub fn hello_interval_ms(d: &Value) -> Option<u64> {
    d.get("heartbeat_interval")
        .and_then(Value::as_u64)
        .filter(|v| *v > 0)
}

pub fn heartbeat(seq: Option<u64>) -> Value {
    json!({ "op": op::HEARTBEAT, "d": seq })
}

pub fn identify(token: &str, intents: u64) -> Value {
    json!({
        "op": op::IDENTIFY,
        "d": {
            "token": token,
            "intents": intents,
            "properties": {
                "os": std::env::consts::OS,
                "browser": "tb-discord-hub",
                "device": "tb-discord-hub"
            }
        }
    })
}

pub fn resume(token: &str, session_id: &str, seq: u64) -> Value {
    json!({
        "op": op::RESUME,
        "d": { "token": token, "session_id": session_id, "seq": seq }
    })
}

pub fn gateway_connect_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    format!("{base}/?v={GATEWAY_VERSION}&encoding=json")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseAction {
    Resume,
    Reidentify,
    Fatal(String),
}

pub fn classify_close(code: Option<u16>) -> CloseAction {
    match code {
        Some(4004) => CloseAction::Fatal(
            "Authentifizierung fehlgeschlagen (4004): Bot-Token ungültig".to_string(),
        ),
        Some(4010) => CloseAction::Fatal("ungültiger Shard (4010)".to_string()),
        Some(4011) => CloseAction::Fatal(
            "Sharding erforderlich (4011): zu viele Guilds für eine Verbindung".to_string(),
        ),
        Some(4012) => CloseAction::Fatal("ungültige API-Version (4012)".to_string()),
        Some(4013) => CloseAction::Fatal("ungültige Intents (4013)".to_string()),
        Some(4014) => CloseAction::Fatal(
            "nicht erlaubte Intents (4014): Server-Members-Intent im Discord Developer Portal \
             (Bot → Privileged Gateway Intents) aktivieren"
                .to_string(),
        ),
        Some(4007) | Some(4009) => CloseAction::Reidentify,
        Some(1000) | Some(1001) => CloseAction::Reidentify,
        _ => CloseAction::Resume,
    }
}

pub fn snowflake(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parst_hello() {
        let p = parse_payload(r#"{"op":10,"d":{"heartbeat_interval":41250},"s":null,"t":null}"#)
            .unwrap();
        assert_eq!(p.op, op::HELLO);
        assert_eq!(hello_interval_ms(&p.d), Some(41250));
        assert_eq!(p.s, None);
    }

    #[test]
    fn parst_dispatch_mit_sequenz() {
        let p = parse_payload(r#"{"op":0,"d":{"x":1},"s":42,"t":"GUILD_CREATE"}"#).unwrap();
        assert_eq!(p.s, Some(42));
        assert_eq!(p.t.as_deref(), Some("GUILD_CREATE"));
    }

    #[test]
    fn parst_ack_ohne_d() {
        let p = parse_payload(r#"{"op":11}"#).unwrap();
        assert_eq!(p.op, op::HEARTBEAT_ACK);
        assert!(p.d.is_null());
    }

    #[test]
    fn ungueltiges_json_ist_fehler() {
        assert!(parse_payload("kein json").is_err());
    }

    #[test]
    fn identify_enthaelt_intents_und_token() {
        let v = identify("abc", INTENTS);
        assert_eq!(v["op"], 2);
        assert_eq!(v["d"]["token"], "abc");
        assert_eq!(v["d"]["intents"], 3);
    }

    #[test]
    fn resume_und_heartbeat() {
        let v = resume("t", "sess", 7);
        assert_eq!(v["op"], 6);
        assert_eq!(v["d"]["session_id"], "sess");
        assert_eq!(v["d"]["seq"], 7);
        assert_eq!(heartbeat(None), json!({"op":1,"d":null}));
        assert_eq!(heartbeat(Some(3)), json!({"op":1,"d":3}));
    }

    #[test]
    fn connect_url() {
        assert_eq!(
            gateway_connect_url("wss://gateway.discord.gg"),
            "wss://gateway.discord.gg/?v=10&encoding=json"
        );
        assert_eq!(
            gateway_connect_url("wss://x.discord.gg/"),
            "wss://x.discord.gg/?v=10&encoding=json"
        );
    }

    #[test]
    fn fatale_close_codes() {
        for code in [4004, 4010, 4011, 4012, 4013, 4014] {
            assert!(
                matches!(classify_close(Some(code)), CloseAction::Fatal(_)),
                "{code} muss fatal sein"
            );
        }
        match classify_close(Some(4014)) {
            CloseAction::Fatal(msg) => assert!(msg.contains("Developer Portal")),
            _ => unreachable!(),
        }
    }

    #[test]
    fn behebbare_close_codes() {
        assert_eq!(classify_close(Some(4000)), CloseAction::Resume);
        assert_eq!(classify_close(Some(4008)), CloseAction::Resume);
        assert_eq!(classify_close(None), CloseAction::Resume);
        assert_eq!(classify_close(Some(4007)), CloseAction::Reidentify);
        assert_eq!(classify_close(Some(4009)), CloseAction::Reidentify);
    }

    #[test]
    fn snowflake_aus_string_und_zahl() {
        assert_eq!(snowflake(Some(&json!("123"))), Some(123));
        assert_eq!(snowflake(Some(&json!(5))), Some(5));
        assert_eq!(snowflake(Some(&json!("x"))), None);
        assert_eq!(snowflake(None), None);
    }
}
