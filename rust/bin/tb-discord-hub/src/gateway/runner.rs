//! Verbindungs-Loop: WebSocket, Heartbeat-Takt, Reconnect mit Backoff,
//! Weitergabe der Ereignisse an die Senke und Bearbeitung von `/setup`.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{Instant, Interval, MissedTickBehavior};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

use super::events::{map_dispatch, HubEvent, SetupInteraction};
use super::permissions::{
    compute_report, render_check_failed, render_community_notice, render_setup_report,
};
use super::protocol::{self, gateway_connect_url};
use super::rest::{ephemeral_deferred, ephemeral_message, DiscordRest};
use super::session::{Action, Session};
use super::GatewayConfig;
use crate::GuildEventSink;

/// Zeitlimit für den Verbindungsaufbau.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Obergrenze des Reconnect-Backoffs.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Zufallszahl in `[0, 1)` ohne zusätzliche Abhängigkeit.
pub(crate) fn jitter() -> f64 {
    let mut h = RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
    );
    (h.finish() >> 11) as f64 / (1u64 << 53) as f64
}

/// Exponentielles Backoff mit Jitter.
#[derive(Debug, Default)]
pub(crate) struct Backoff {
    attempt: u32,
}

impl Backoff {
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// Wartezeit vor dem nächsten Versuch: erster Versuch < 1 s, danach
    /// 2, 4, 8 … s bis höchstens 60 s (jeweils mit bis zu 25 % Jitter).
    pub fn next_delay(&mut self, rnd: f64) -> Duration {
        let attempt = self.attempt;
        self.attempt = self.attempt.saturating_add(1);
        if attempt == 0 {
            return Duration::from_secs_f64(rnd);
        }
        let base = 2f64
            .powi(attempt.min(6) as i32)
            .min(MAX_BACKOFF.as_secs_f64());
        Duration::from_secs_f64((base * (0.75 + 0.25 * rnd)).min(MAX_BACKOFF.as_secs_f64()))
    }
}

/// Ereignisse für den Senken-Worker (Reihenfolge bleibt erhalten).
enum SinkEvent {
    Added(u64, String),
    Removed(u64),
    MemberLeft(u64, u64),
}

/// Gemeinsamer Kontext für Dispatch- und Interaction-Bearbeitung.
pub(crate) struct Ctx {
    pub rest: DiscordRest,
    pub community_guild_id: u64,
    pub application_id: Option<u64>,
    pub bot_user_id: AtomicU64,
    commands_registered: AtomicBool,
}

impl Ctx {
    pub fn new(rest: DiscordRest, community_guild_id: u64, application_id: Option<u64>) -> Self {
        Self {
            rest,
            community_guild_id,
            application_id,
            bot_user_id: AtomicU64::new(0),
            commands_registered: AtomicBool::new(false),
        }
    }

    fn guild_label(&self, guild_id: u64) -> &'static str {
        if guild_id == self.community_guild_id {
            "Community-Server (Haupt-Bot)"
        } else {
            "fremder Server"
        }
    }
}

/// Ergebnis einer einzelnen WebSocket-Verbindung.
enum Outcome {
    Reconnect { resume: bool, reason: String },
    Fatal(String),
}

/// Haupt-Loop des Gateways. Kehrt nur bei fatalen Fehlern zurück.
pub(crate) async fn run(
    config: GatewayConfig,
    sink: Arc<dyn GuildEventSink>,
    api_base: &str,
) -> Result<(), String> {
    if config.bot_token.trim().is_empty() {
        return Err("kein Bot-Token konfiguriert".to_string());
    }
    let rest = DiscordRest::new(api_base, config.bot_token.clone());
    let ctx = Arc::new(Ctx::new(
        rest,
        config.community_guild_id,
        config.application_id,
    ));
    let (tx, rx) = mpsc::unbounded_channel();
    let worker = tokio::spawn(sink_worker(rx, sink));

    let result = Runner {
        session: Session::new(config.bot_token, protocol::INTENTS),
        ctx,
        sink_tx: tx,
        gateway_url: None,
    }
    .run_loop()
    .await;

    // Senke noch abarbeiten lassen, dann beenden.
    let _ = worker.await;
    result
}

async fn sink_worker(mut rx: mpsc::UnboundedReceiver<SinkEvent>, sink: Arc<dyn GuildEventSink>) {
    while let Some(ev) = rx.recv().await {
        match ev {
            SinkEvent::Added(id, name) => sink.guild_added(id, &name).await,
            SinkEvent::Removed(id) => sink.guild_removed(id).await,
            SinkEvent::MemberLeft(g, u) => sink.member_left(g, u).await,
        }
    }
}

struct Runner {
    session: Session,
    ctx: Arc<Ctx>,
    sink_tx: mpsc::UnboundedSender<SinkEvent>,
    gateway_url: Option<String>,
}

impl Runner {
    async fn run_loop(mut self) -> Result<(), String> {
        let mut backoff = Backoff::default();
        loop {
            let base_url = match self.session.resume_url() {
                Some(u) => u.to_string(),
                None => match self.fresh_gateway_url().await {
                    Ok(u) => u,
                    Err(FetchError::Fatal(msg)) => return Err(msg),
                    Err(FetchError::Transient(msg)) => {
                        let delay = backoff.next_delay(jitter());
                        tracing::warn!(error = %msg, ?delay, "Gateway-URL nicht abrufbar, neuer Versuch");
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                },
            };
            let url = gateway_connect_url(&base_url);
            self.session.on_connect();
            tracing::info!(%url, resume = self.session.can_resume(), "verbinde mit Discord-Gateway");
            match self.run_connection(&url).await {
                Outcome::Fatal(msg) => return Err(msg),
                Outcome::Reconnect { resume, reason } => {
                    if !resume {
                        self.session.invalidate();
                    }
                    if self.session.established() {
                        backoff.reset();
                    }
                    let mut delay = backoff.next_delay(jitter());
                    if !resume && reason.contains("Opcode 9") {
                        // Discord empfiehlt 1–5 s Wartezeit vor neuem IDENTIFY.
                        delay = delay.max(Duration::from_secs_f64(1.0 + 4.0 * jitter()));
                    }
                    tracing::warn!(%reason, resume, ?delay, "Gateway-Verbindung beendet, verbinde neu");
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    /// Holt die Gateway-URL per `GET /gateway/bot` und beachtet das
    /// Session-Start-Limit.
    async fn fresh_gateway_url(&mut self) -> Result<String, FetchError> {
        match self.ctx.rest.gateway_bot().await {
            Ok(gb) => {
                if let Some(limit) = &gb.session_start_limit {
                    if limit.remaining == 0 {
                        let wait = Duration::from_millis(limit.reset_after);
                        tracing::warn!(?wait, "Session-Start-Limit erschöpft, warte");
                        tokio::time::sleep(wait).await;
                    }
                }
                self.gateway_url = Some(gb.url.clone());
                Ok(gb.url)
            }
            Err(e) if e.is_unauthorized() => Err(FetchError::Fatal(format!(
                "Bot-Token ungültig (GET /gateway/bot: {e})"
            ))),
            Err(e) => match &self.gateway_url {
                Some(u) => {
                    tracing::warn!(error = %e, "GET /gateway/bot fehlgeschlagen, nutze letzte URL");
                    Ok(u.clone())
                }
                None => Err(FetchError::Transient(e.to_string())),
            },
        }
    }

    async fn run_connection(&mut self, url: &str) -> Outcome {
        let ws = match tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(url))
            .await
        {
            Ok(Ok((ws, _))) => ws,
            Ok(Err(e)) => {
                return Outcome::Reconnect {
                    resume: self.session.can_resume(),
                    reason: format!("Verbindungsaufbau fehlgeschlagen: {e}"),
                }
            }
            Err(_) => {
                return Outcome::Reconnect {
                    resume: self.session.can_resume(),
                    reason: "Zeitüberschreitung beim Verbindungsaufbau".to_string(),
                }
            }
        };
        let (mut write, mut read) = ws.split();
        let mut heartbeat: Option<Interval> = None;

        loop {
            let actions: Vec<Action> = tokio::select! {
                msg = read.next() => match msg {
                    Some(Ok(Message::Text(text))) => match protocol::parse_payload(&text) {
                        Ok(p) => self.session.on_payload(p),
                        Err(e) => {
                            tracing::warn!(error = %e, "Gateway-Payload verworfen");
                            Vec::new()
                        }
                    },
                    Some(Ok(Message::Close(frame))) => {
                        let code = frame.as_ref().map(|f| u16::from(f.code));
                        if let Some(f) = &frame {
                            tracing::info!(code = u16::from(f.code), reason = %f.reason, "Gateway schließt Verbindung");
                        }
                        vec![self.session.on_close(code)]
                    }
                    Some(Ok(Message::Binary(_))) => {
                        tracing::warn!("unerwarteter Binär-Frame vom Gateway ignoriert");
                        Vec::new()
                    }
                    Some(Ok(_)) => Vec::new(),
                    Some(Err(e)) => {
                        tracing::warn!(error = %e, "WebSocket-Fehler");
                        vec![self.session.on_close(None)]
                    }
                    None => vec![self.session.on_close(None)],
                },
                _ = tick(&mut heartbeat) => vec![self.session.on_heartbeat_tick()],
            };

            for action in actions {
                match action {
                    Action::Send(v) => {
                        if let Err(e) = write.send(Message::text(v.to_string())).await {
                            return Outcome::Reconnect {
                                resume: self.session.can_resume(),
                                reason: format!("Senden fehlgeschlagen: {e}"),
                            };
                        }
                    }
                    Action::StartHeartbeat { interval_ms } => {
                        let period = Duration::from_millis(interval_ms);
                        // Erster Heartbeat nach interval * jitter (Discord-Vorgabe).
                        let first = Instant::now() + period.mul_f64(jitter());
                        let mut iv = tokio::time::interval_at(first, period);
                        iv.set_missed_tick_behavior(MissedTickBehavior::Delay);
                        heartbeat = Some(iv);
                        tracing::debug!(interval_ms, "Heartbeat gestartet");
                    }
                    Action::Dispatch { name, data } => self.on_dispatch(&name, &data),
                    Action::Reconnect { resume, reason } => {
                        // Bei RESUME nicht mit 1000/1001 schließen, sonst verfällt die Sitzung.
                        let code = if resume { 4000 } else { 1000 };
                        let frame = CloseFrame {
                            code: CloseCode::from(code),
                            reason: "".into(),
                        };
                        let _ = tokio::time::timeout(
                            Duration::from_secs(2),
                            write.send(Message::Close(Some(frame))),
                        )
                        .await;
                        return Outcome::Reconnect { resume, reason };
                    }
                    Action::Fatal(msg) => return Outcome::Fatal(msg),
                }
            }
        }
    }

    fn on_dispatch(&mut self, name: &str, data: &serde_json::Value) {
        if name == "RESUMED" {
            tracing::info!("Gateway-Sitzung fortgesetzt");
        }
        let Some(event) = map_dispatch(name, data) else {
            return;
        };
        let ctx = &self.ctx;
        match event {
            HubEvent::Ready {
                bot_user_id,
                application_id,
                guild_count,
            } => {
                ctx.bot_user_id.store(bot_user_id, Ordering::Relaxed);
                tracing::info!(bot_user_id, ?application_id, guild_count, "Gateway READY");
                if let (Some(cfg), Some(got)) = (ctx.application_id, application_id) {
                    if cfg != got {
                        tracing::warn!(
                            konfiguriert = cfg,
                            gateway = got,
                            "Application-ID weicht von der des Bot-Tokens ab"
                        );
                    }
                }
                self.register_commands_once();
            }
            HubEvent::GuildAdded { guild_id, name } => {
                tracing::info!(guild_id, %name, art = ctx.guild_label(guild_id), "Guild verfügbar");
                let _ = self.sink_tx.send(SinkEvent::Added(guild_id, name));
            }
            HubEvent::GuildRemoved { guild_id } => {
                tracing::info!(
                    guild_id,
                    art = ctx.guild_label(guild_id),
                    "Hub aus Guild entfernt"
                );
                let _ = self.sink_tx.send(SinkEvent::Removed(guild_id));
            }
            HubEvent::MemberLeft { guild_id, user_id } => {
                if user_id == ctx.bot_user_id.load(Ordering::Relaxed) {
                    return;
                }
                tracing::debug!(
                    guild_id,
                    user_id,
                    art = ctx.guild_label(guild_id),
                    "Mitglied hat Guild verlassen"
                );
                let _ = self.sink_tx.send(SinkEvent::MemberLeft(guild_id, user_id));
            }
            HubEvent::SetupCommand(inter) => {
                let ctx = Arc::clone(&self.ctx);
                tokio::spawn(async move { handle_setup(&ctx, inter).await });
            }
        }
    }

    /// Registriert `/setup` einmal pro Prozess (Bulk-Overwrite ist idempotent).
    fn register_commands_once(&self) {
        let Some(app_id) = self.ctx.application_id else {
            tracing::info!("keine Application-ID konfiguriert, /setup wird nicht registriert");
            return;
        };
        if self.ctx.commands_registered.swap(true, Ordering::SeqCst) {
            return;
        }
        let ctx = Arc::clone(&self.ctx);
        tokio::spawn(async move {
            match ctx.rest.register_commands(app_id).await {
                Ok(()) => tracing::info!(app_id, "Slash-Befehl /setup global registriert"),
                Err(e) => {
                    ctx.commands_registered.store(false, Ordering::SeqCst);
                    tracing::error!(error = %e, "Registrierung von /setup fehlgeschlagen");
                }
            }
        });
    }
}

enum FetchError {
    Fatal(String),
    Transient(String),
}

/// Wartet auf den nächsten Heartbeat-Takt (oder ewig, solange keiner läuft).
async fn tick(heartbeat: &mut Option<Interval>) {
    match heartbeat {
        Some(iv) => {
            iv.tick().await;
        }
        None => std::future::pending::<()>().await,
    }
}

/// Bearbeitet `/setup`: Prüft die Rechte des Hubs und antwortet ephemeral.
pub(crate) async fn handle_setup(ctx: &Ctx, inter: SetupInteraction) {
    let rest = &ctx.rest;
    let Some(guild_id) = inter.guild_id else {
        let body = ephemeral_message("Please run `/setup` inside a server.");
        if let Err(e) = rest
            .interaction_callback(inter.interaction_id, &inter.token, &body)
            .await
        {
            tracing::warn!(error = %e, "Antwort auf /setup fehlgeschlagen");
        }
        return;
    };
    if guild_id == ctx.community_guild_id {
        tracing::info!(
            guild_id,
            "/setup auf dem Community-Server – Hinweis auf Haupt-Bot"
        );
        let body = ephemeral_message(&render_community_notice());
        if let Err(e) = rest
            .interaction_callback(inter.interaction_id, &inter.token, &body)
            .await
        {
            tracing::warn!(error = %e, "Antwort auf /setup fehlgeschlagen");
        }
        return;
    }

    // Aufschieben, damit die REST-Abfragen nicht an der 3-Sekunden-Grenze scheitern.
    if let Err(e) = rest
        .interaction_callback(inter.interaction_id, &inter.token, &ephemeral_deferred())
        .await
    {
        tracing::warn!(error = %e, guild_id, "Aufschieben von /setup fehlgeschlagen");
        return;
    }
    let bot_user_id = ctx.bot_user_id.load(Ordering::Relaxed);
    let content = match check_guild(ctx, guild_id, bot_user_id).await {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!(error = %e, guild_id, "Rechteprüfung für /setup fehlgeschlagen");
            render_check_failed(inter.app_permissions)
        }
    };
    tracing::info!(guild_id, "/setup beantwortet");
    if let Err(e) = rest
        .edit_original_response(inter.application_id, &inter.token, &content)
        .await
    {
        tracing::warn!(error = %e, guild_id, "Bearbeiten der /setup-Antwort fehlgeschlagen");
    }
}

async fn check_guild(ctx: &Ctx, guild_id: u64, bot_user_id: u64) -> Result<String, String> {
    if bot_user_id == 0 {
        return Err("Bot-User-ID noch unbekannt (kein READY)".to_string());
    }
    let guild = ctx.rest.guild(guild_id).await.map_err(|e| e.to_string())?;
    let member_roles = ctx
        .rest
        .member_role_ids(guild_id, bot_user_id)
        .await
        .map_err(|e| e.to_string())?;
    let report = compute_report(
        guild_id,
        guild.owner_id,
        bot_user_id,
        &guild.roles,
        &member_roles,
    );
    Ok(render_setup_report(&report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::sync::Mutex;
    use tokio::net::TcpListener;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn jitter_im_bereich() {
        for _ in 0..100 {
            let j = jitter();
            assert!((0.0..1.0).contains(&j));
        }
    }

    #[test]
    fn backoff_waechst_und_ist_begrenzt() {
        let mut b = Backoff::default();
        assert!(b.next_delay(0.5) < Duration::from_secs(1));
        assert_eq!(b.next_delay(1.0), Duration::from_secs(2));
        assert_eq!(b.next_delay(1.0), Duration::from_secs(4));
        for _ in 0..20 {
            assert!(b.next_delay(1.0) <= MAX_BACKOFF);
        }
        b.reset();
        assert!(b.next_delay(0.0) < Duration::from_secs(1));
    }

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<String>>);

    #[async_trait::async_trait]
    impl GuildEventSink for RecordingSink {
        async fn guild_added(&self, guild_id: u64, name: &str) {
            self.0
                .lock()
                .unwrap()
                .push(format!("added:{guild_id}:{name}"));
        }
        async fn guild_removed(&self, guild_id: u64) {
            self.0.lock().unwrap().push(format!("removed:{guild_id}"));
        }
        async fn member_left(&self, guild_id: u64, user_id: u64) {
            self.0
                .lock()
                .unwrap()
                .push(format!("left:{guild_id}:{user_id}"));
        }
    }

    type Ws = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

    async fn send(ws: &mut Ws, v: Value) {
        ws.send(Message::text(v.to_string())).await.unwrap();
    }

    /// Liest bis zum ersten Payload mit `op` (Heartbeats werden bestätigt).
    async fn expect_op(ws: &mut Ws, op: u64) -> Value {
        loop {
            let msg = ws.next().await.unwrap().unwrap();
            let Message::Text(t) = msg else { continue };
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["op"] == op {
                return v;
            }
            if v["op"] == 1 {
                send(ws, json!({"op": 11})).await;
            }
        }
    }

    async fn close(ws: &mut Ws, code: u16) {
        let _ = ws
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::from(code),
                reason: "".into(),
            })))
            .await;
        // Auf Abschluss des Close-Handshakes warten.
        while let Some(Ok(_)) = ws.next().await {}
    }

    async fn mock_gateway_bot(ws_url: &str, expected_calls: u64) -> MockServer {
        let api = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gateway/bot"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "url": ws_url, "shards": 1,
                "session_start_limit": {"total": 1000, "remaining": 999, "reset_after": 0, "max_concurrency": 1}
            })))
            .expect(expected_calls)
            .mount(&api)
            .await;
        api
    }

    fn config(app: Option<u64>) -> GatewayConfig {
        GatewayConfig {
            bot_token: "tok".into(),
            application_id: app,
            community_guild_id: 1,
        }
    }

    #[tokio::test]
    async fn hello_identify_dispatch_und_fataler_close() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_url = format!("ws://{}", listener.local_addr().unwrap());
        let api = mock_gateway_bot(&ws_url, 1).await;

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            send(
                &mut ws,
                json!({"op": 10, "d": {"heartbeat_interval": 45000}}),
            )
            .await;
            let ident = expect_op(&mut ws, 2).await;
            assert_eq!(ident["d"]["token"], "tok");
            assert_eq!(ident["d"]["intents"], 3);
            send(
                &mut ws,
                json!({"op":0,"s":1,"t":"READY","d":{
                "session_id":"s1","resume_gateway_url":"ws://127.0.0.1:1",
                "user":{"id":"50"},"application":{"id":"77"},"guilds":[]}}),
            )
            .await;
            send(
                &mut ws,
                json!({"op":0,"s":2,"t":"GUILD_CREATE","d":{"id":"1","name":"Community"}}),
            )
            .await;
            send(
                &mut ws,
                json!({"op":0,"s":3,"t":"GUILD_CREATE","d":{"id":"2","name":"Fremd"}}),
            )
            .await;
            send(
                &mut ws,
                json!({"op":0,"s":4,"t":"GUILD_CREATE","d":{"id":"3","unavailable":true}}),
            )
            .await;
            send(
                &mut ws,
                json!({"op":0,"s":5,"t":"GUILD_DELETE","d":{"id":"3","unavailable":true}}),
            )
            .await;
            send(&mut ws, json!({"op":0,"s":6,"t":"GUILD_MEMBER_REMOVE","d":{"guild_id":"2","user":{"id":"8"}}})).await;
            // Eigener Bot: wird nicht gemeldet.
            send(&mut ws, json!({"op":0,"s":7,"t":"GUILD_MEMBER_REMOVE","d":{"guild_id":"2","user":{"id":"50"}}})).await;
            send(
                &mut ws,
                json!({"op":0,"s":8,"t":"GUILD_DELETE","d":{"id":"2"}}),
            )
            .await;
            close(&mut ws, 4014).await;
        });

        let sink = Arc::new(RecordingSink::default());
        let res = tokio::time::timeout(
            Duration::from_secs(10),
            run(config(None), sink.clone(), &api.uri()),
        )
        .await
        .expect("Gateway muss bei 4014 enden");
        let err = res.unwrap_err();
        assert!(err.contains("4014"), "{err}");
        server.await.unwrap();
        assert_eq!(
            *sink.0.lock().unwrap(),
            vec![
                "added:1:Community".to_string(),
                "added:2:Fremd".to_string(),
                "left:2:8".to_string(),
                "removed:2".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn reconnect_opcode_7_setzt_sitzung_fort() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_url = format!("ws://{}", listener.local_addr().unwrap());
        // Nur ein Abruf: Die zweite Verbindung nutzt resume_gateway_url.
        let api = mock_gateway_bot(&ws_url, 1).await;
        let resume_url = ws_url.clone();

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            send(
                &mut ws,
                json!({"op": 10, "d": {"heartbeat_interval": 45000}}),
            )
            .await;
            expect_op(&mut ws, 2).await;
            send(
                &mut ws,
                json!({"op":0,"s":1,"t":"READY","d":{
                "session_id":"s1","resume_gateway_url":resume_url,
                "user":{"id":"50"},"guilds":[]}}),
            )
            .await;
            send(
                &mut ws,
                json!({"op":0,"s":2,"t":"GUILD_CREATE","d":{"id":"2","name":"Fremd"}}),
            )
            .await;
            send(&mut ws, json!({"op": 7, "d": null})).await;
            // Client schließt mit Nicht-1000-Code.
            let mut client_code = None;
            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Close(Some(f)) = msg {
                    client_code = Some(u16::from(f.code));
                }
            }
            assert_eq!(client_code, Some(4000));

            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            send(
                &mut ws,
                json!({"op": 10, "d": {"heartbeat_interval": 45000}}),
            )
            .await;
            let resume = expect_op(&mut ws, 6).await;
            assert_eq!(resume["d"]["session_id"], "s1");
            assert_eq!(resume["d"]["seq"], 2);
            assert_eq!(resume["d"]["token"], "tok");
            close(&mut ws, 4004).await;
        });

        let sink = Arc::new(RecordingSink::default());
        let err = tokio::time::timeout(
            Duration::from_secs(10),
            run(config(None), sink.clone(), &api.uri()),
        )
        .await
        .expect("Gateway muss bei 4004 enden")
        .unwrap_err();
        assert!(err.contains("4004"), "{err}");
        server.await.unwrap();
        assert_eq!(*sink.0.lock().unwrap(), vec!["added:2:Fremd".to_string()]);
    }

    #[tokio::test]
    async fn ready_registriert_setup_befehl() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_url = format!("ws://{}", listener.local_addr().unwrap());
        let api = mock_gateway_bot(&ws_url, 1).await;
        Mock::given(method("PUT"))
            .and(path("/applications/77/commands"))
            .and(body_partial_json(
                json!([{"name":"setup","default_member_permissions":"32"}]),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .expect(1)
            .mount(&api)
            .await;

        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            send(
                &mut ws,
                json!({"op": 10, "d": {"heartbeat_interval": 45000}}),
            )
            .await;
            expect_op(&mut ws, 2).await;
            send(
                &mut ws,
                json!({"op":0,"s":1,"t":"READY","d":{
                "session_id":"s1","resume_gateway_url":"ws://127.0.0.1:1",
                "user":{"id":"50"},"guilds":[]}}),
            )
            .await;
            // Zeit für die Registrierung lassen.
            tokio::time::sleep(Duration::from_millis(300)).await;
            close(&mut ws, 4004).await;
        });

        let sink = Arc::new(RecordingSink::default());
        let _ = tokio::time::timeout(
            Duration::from_secs(10),
            run(config(Some(77)), sink, &api.uri()),
        )
        .await
        .unwrap();
        server.await.unwrap();
        // `expect(1)` wird beim Drop des MockServers geprüft.
    }

    #[tokio::test]
    async fn leerer_token_bricht_ab() {
        let mut cfg = config(None);
        cfg.bot_token = " ".into();
        let err = run(
            cfg,
            Arc::new(RecordingSink::default()),
            "http://127.0.0.1:1",
        )
        .await
        .unwrap_err();
        assert!(err.contains("Bot-Token"));
    }

    #[tokio::test]
    async fn ungueltiger_token_bei_gateway_bot_ist_fatal() {
        let api = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/gateway/bot"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&api)
            .await;
        let err = run(config(None), Arc::new(RecordingSink::default()), &api.uri())
            .await
            .unwrap_err();
        assert!(err.contains("Bot-Token ungültig"));
    }

    fn inter(guild_id: Option<u64>) -> SetupInteraction {
        SetupInteraction {
            interaction_id: 11,
            token: "itok".into(),
            application_id: 77,
            guild_id,
            app_permissions: None,
        }
    }

    #[tokio::test]
    async fn setup_fremder_server_prueft_rechte() {
        let api = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/interactions/11/itok/callback"))
            .and(body_partial_json(json!({"type": 5, "data": {"flags": 64}})))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&api)
            .await;
        Mock::given(method("GET"))
            .and(path("/guilds/2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "2", "owner_id": "9",
                "roles": [
                    {"id":"2","name":"@everyone","permissions":"3072","position":0},
                    {"id":"10","name":"Deadlock Hub","permissions":"268451840","position":2,"managed":true},
                    {"id":"11","name":"Live","permissions":"0","position":1},
                    {"id":"12","name":"Admins","permissions":"8","position":5}
                ]
            })))
            .mount(&api)
            .await;
        Mock::given(method("GET"))
            .and(path("/guilds/2/members/50"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"roles": ["10"]})))
            .mount(&api)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/webhooks/77/itok/messages/@original"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&api)
            .await;

        let ctx = Ctx::new(DiscordRest::new(api.uri(), "tok"), 1, Some(77));
        ctx.bot_user_id.store(50, Ordering::Relaxed);
        handle_setup(&ctx, inter(Some(2))).await;

        let reqs = api.received_requests().await.unwrap();
        let patch = reqs
            .iter()
            .find(|r| r.method.as_str() == "PATCH")
            .expect("PATCH gesendet");
        let body: Value = serde_json::from_slice(&patch.body).unwrap();
        let content = body["content"].as_str().unwrap();
        assert!(content.contains("✅ View Channels"), "{content}");
        assert!(content.contains("✅ Manage Roles"), "{content}");
        assert!(content.contains("All required permissions"), "{content}");
        assert!(content.contains("`Admins`"), "{content}");
        assert!(content.contains("Roles it can manage: 1"), "{content}");
        // Callbacks ohne Bot-Token.
        let cb = reqs.iter().find(|r| r.method.as_str() == "POST").unwrap();
        assert!(!cb.headers.contains_key("authorization"));
    }

    #[tokio::test]
    async fn setup_community_server_nur_hinweis() {
        let api = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/interactions/11/itok/callback"))
            .and(body_partial_json(json!({"type": 4, "data": {"flags": 64}})))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&api)
            .await;
        let ctx = Ctx::new(DiscordRest::new(api.uri(), "tok"), 1, Some(77));
        handle_setup(&ctx, inter(Some(1))).await;
        let reqs = api.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&reqs[0].body).unwrap();
        assert!(body["data"]["content"]
            .as_str()
            .unwrap()
            .contains("main Deadlock bot"));
    }

    #[tokio::test]
    async fn setup_rueckfall_bei_rest_fehler() {
        let api = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/interactions/11/itok/callback"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&api)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&api)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/webhooks/77/itok/messages/@original"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&api)
            .await;
        let ctx = Ctx::new(DiscordRest::new(api.uri(), "tok"), 1, Some(77));
        ctx.bot_user_id.store(50, Ordering::Relaxed);
        let mut i = inter(Some(2));
        i.app_permissions = Some(1024);
        handle_setup(&ctx, i).await;
        let reqs = api.received_requests().await.unwrap();
        let patch = reqs.iter().find(|r| r.method.as_str() == "PATCH").unwrap();
        let body: Value = serde_json::from_slice(&patch.body).unwrap();
        assert!(body["content"].as_str().unwrap().contains("missing"));
    }
}
