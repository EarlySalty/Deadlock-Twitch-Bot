use serde_json::Value;

use super::protocol::{self, op, CloseAction, GatewayPayload};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Send(Value),
    StartHeartbeat { interval_ms: u64 },
    Dispatch { name: String, data: Value },
    Reconnect { resume: bool, reason: String },
    Fatal(String),
}

#[derive(Clone)]
pub struct Session {
    token: String,
    intents: u64,
    session_id: Option<String>,
    resume_gateway_url: Option<String>,
    seq: Option<u64>,
    heartbeat_acked: bool,
    established: bool,
}

impl Session {
    pub fn new(token: impl Into<String>, intents: u64) -> Self {
        Self {
            token: token.into(),
            intents,
            session_id: None,
            resume_gateway_url: None,
            seq: None,
            heartbeat_acked: true,
            established: false,
        }
    }

    pub fn can_resume(&self) -> bool {
        self.session_id.is_some() && self.seq.is_some()
    }

    pub fn resume_url(&self) -> Option<&str> {
        if self.can_resume() {
            self.resume_gateway_url.as_deref()
        } else {
            None
        }
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn seq(&self) -> Option<u64> {
        self.seq
    }

    pub fn established(&self) -> bool {
        self.established
    }

    pub fn invalidate(&mut self) {
        self.session_id = None;
        self.resume_gateway_url = None;
        self.seq = None;
    }

    pub fn on_connect(&mut self) {
        self.heartbeat_acked = true;
        self.established = false;
    }

    pub fn on_payload(&mut self, p: GatewayPayload) -> Vec<Action> {
        match p.op {
            op::HELLO => {
                let Some(interval_ms) = protocol::hello_interval_ms(&p.d) else {
                    return vec![Action::Reconnect {
                        resume: self.can_resume(),
                        reason: "HELLO ohne heartbeat_interval".to_string(),
                    }];
                };
                self.heartbeat_acked = true;
                let login = match (&self.session_id, self.seq) {
                    (Some(sid), Some(seq)) => protocol::resume(&self.token, sid, seq),
                    _ => protocol::identify(&self.token, self.intents),
                };
                vec![Action::StartHeartbeat { interval_ms }, Action::Send(login)]
            }
            op::HEARTBEAT_ACK => {
                self.heartbeat_acked = true;
                Vec::new()
            }
            op::HEARTBEAT => vec![Action::Send(protocol::heartbeat(self.seq))],
            op::RECONNECT => vec![Action::Reconnect {
                resume: true,
                reason: "Gateway fordert Reconnect an (Opcode 7)".to_string(),
            }],
            op::INVALID_SESSION => {
                let resumable = p.d.as_bool().unwrap_or(false) && self.can_resume();
                if !resumable {
                    self.invalidate();
                }
                vec![Action::Reconnect {
                    resume: resumable,
                    reason: format!("Sitzung ungültig (Opcode 9, fortsetzbar: {resumable})"),
                }]
            }
            op::DISPATCH => {
                if let Some(s) = p.s {
                    self.seq = Some(s);
                }
                let name = p.t.unwrap_or_default();
                match name.as_str() {
                    "READY" => {
                        self.session_id =
                            p.d.get("session_id")
                                .and_then(Value::as_str)
                                .map(str::to_string);
                        self.resume_gateway_url =
                            p.d.get("resume_gateway_url")
                                .and_then(Value::as_str)
                                .map(str::to_string);
                        self.established = true;
                    }
                    "RESUMED" => self.established = true,
                    _ => {}
                }
                vec![Action::Dispatch { name, data: p.d }]
            }
            _ => Vec::new(),
        }
    }

    pub fn on_heartbeat_tick(&mut self) -> Action {
        if !self.heartbeat_acked {
            return Action::Reconnect {
                resume: true,
                reason: "kein Heartbeat-ACK erhalten".to_string(),
            };
        }
        self.heartbeat_acked = false;
        Action::Send(protocol::heartbeat(self.seq))
    }

    pub fn on_close(&mut self, code: Option<u16>) -> Action {
        match protocol::classify_close(code) {
            CloseAction::Fatal(msg) => Action::Fatal(msg),
            CloseAction::Reidentify => {
                self.invalidate();
                Action::Reconnect {
                    resume: false,
                    reason: format!("Verbindung geschlossen ({code:?}), neue Sitzung nötig"),
                }
            }
            CloseAction::Resume => Action::Reconnect {
                resume: self.can_resume(),
                reason: format!("Verbindung geschlossen ({code:?})"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn payload(v: Value) -> GatewayPayload {
        serde_json::from_value(v).unwrap()
    }

    fn ready(s: &mut Session) {
        s.on_payload(payload(json!({
            "op": 0, "s": 1, "t": "READY",
            "d": {"session_id": "abc", "resume_gateway_url": "wss://resume.example"}
        })));
    }

    #[test]
    fn hello_ohne_sitzung_identifiziert() {
        let mut s = Session::new("tok", 3);
        let a = s.on_payload(payload(json!({"op":10,"d":{"heartbeat_interval":1000}})));
        assert_eq!(a[0], Action::StartHeartbeat { interval_ms: 1000 });
        match &a[1] {
            Action::Send(v) => {
                assert_eq!(v["op"], 2);
                assert_eq!(v["d"]["intents"], 3);
            }
            other => panic!("unerwartet: {other:?}"),
        }
    }

    #[test]
    fn hello_mit_sitzung_setzt_fort() {
        let mut s = Session::new("tok", 3);
        ready(&mut s);
        s.on_payload(payload(json!({"op":0,"s":5,"t":"GUILD_CREATE","d":{}})));
        assert_eq!(s.resume_url(), Some("wss://resume.example"));
        s.on_connect();
        let a = s.on_payload(payload(json!({"op":10,"d":{"heartbeat_interval":1000}})));
        match &a[1] {
            Action::Send(v) => {
                assert_eq!(v["op"], 6);
                assert_eq!(v["d"]["session_id"], "abc");
                assert_eq!(v["d"]["seq"], 5);
            }
            other => panic!("unerwartet: {other:?}"),
        }
    }

    #[test]
    fn hello_ohne_intervall_reconnect() {
        let mut s = Session::new("tok", 3);
        let a = s.on_payload(payload(json!({"op":10,"d":{}})));
        assert!(matches!(a[0], Action::Reconnect { .. }));
    }

    #[test]
    fn dispatch_merkt_sequenz_und_ready() {
        let mut s = Session::new("tok", 3);
        assert!(!s.established());
        ready(&mut s);
        assert!(s.established());
        assert_eq!(s.session_id(), Some("abc"));
        assert_eq!(s.seq(), Some(1));
        let a = s.on_payload(payload(
            json!({"op":0,"s":9,"t":"GUILD_DELETE","d":{"id":"1"}}),
        ));
        assert_eq!(s.seq(), Some(9));
        assert!(matches!(&a[0], Action::Dispatch { name, .. } if name == "GUILD_DELETE"));
    }

    #[test]
    fn heartbeat_ohne_ack_ist_zombie() {
        let mut s = Session::new("tok", 3);
        assert_eq!(
            s.on_heartbeat_tick(),
            Action::Send(json!({"op":1,"d":null}))
        );
        assert!(matches!(
            s.on_heartbeat_tick(),
            Action::Reconnect { resume: true, .. }
        ));
    }

    #[test]
    fn heartbeat_mit_ack_laeuft_weiter() {
        let mut s = Session::new("tok", 3);
        ready(&mut s);
        assert_eq!(s.on_heartbeat_tick(), Action::Send(json!({"op":1,"d":1})));
        s.on_payload(payload(json!({"op":11})));
        assert_eq!(s.on_heartbeat_tick(), Action::Send(json!({"op":1,"d":1})));
    }

    #[test]
    fn server_fordert_heartbeat_an() {
        let mut s = Session::new("tok", 3);
        let a = s.on_payload(payload(json!({"op":1,"d":null})));
        assert_eq!(a, vec![Action::Send(json!({"op":1,"d":null}))]);
    }

    #[test]
    fn opcode_7_reconnect_mit_resume() {
        let mut s = Session::new("tok", 3);
        ready(&mut s);
        let a = s.on_payload(payload(json!({"op":7,"d":null})));
        assert!(matches!(a[0], Action::Reconnect { resume: true, .. }));
        assert!(s.can_resume());
    }

    #[test]
    fn opcode_9_nicht_fortsetzbar_verwirft_sitzung() {
        let mut s = Session::new("tok", 3);
        ready(&mut s);
        let a = s.on_payload(payload(json!({"op":9,"d":false})));
        assert!(matches!(a[0], Action::Reconnect { resume: false, .. }));
        assert!(!s.can_resume());
        assert_eq!(s.resume_url(), None);
    }

    #[test]
    fn opcode_9_fortsetzbar_behaelt_sitzung() {
        let mut s = Session::new("tok", 3);
        ready(&mut s);
        let a = s.on_payload(payload(json!({"op":9,"d":true})));
        assert!(matches!(a[0], Action::Reconnect { resume: true, .. }));
        assert!(s.can_resume());
    }

    #[test]
    fn close_codes() {
        let mut s = Session::new("tok", 3);
        ready(&mut s);
        assert!(matches!(s.on_close(Some(4014)), Action::Fatal(_)));
        assert!(matches!(
            s.on_close(Some(4000)),
            Action::Reconnect { resume: true, .. }
        ));
        assert!(matches!(
            s.on_close(Some(4009)),
            Action::Reconnect { resume: false, .. }
        ));
        assert!(!s.can_resume());
        assert!(matches!(
            s.on_close(Some(4000)),
            Action::Reconnect { resume: false, .. }
        ));
    }
}
