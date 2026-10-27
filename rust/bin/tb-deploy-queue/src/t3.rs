use crate::config::T3Config;
use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

const PROTOCOL: &str = "2";
const ACTIVE_RUN_STATES: [&str; 5] = ["preparing", "queued", "starting", "running", "waiting"];
const FAILED_RUN_STATES: [&str; 4] = ["interrupted", "failed", "cancelled", "rolled_back"];
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Client {
    http: reqwest::Client,
    base: Url,
    bearer_file: std::path::PathBuf,
}

pub struct Thread {
    projection: Value,
}

pub enum Compaction {
    Waiting,
    Confirmed(String),
    Failed(String),
}

impl Client {
    pub fn new(config: &T3Config) -> Result<Self> {
        let base = Url::parse(&config.base_url).context("invalid T3 base URL")?;
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("cannot build HTTP client")?;
        Ok(Self {
            http,
            base,
            bearer_file: config.bearer_file.clone(),
        })
    }

    fn bearer(&self) -> Result<String> {
        let raw = std::fs::read_to_string(&self.bearer_file)
            .map_err(|error| anyhow!("cannot read the T3 bearer file: {}", error.kind()))?;
        let bearer = raw.trim().to_string();
        if bearer.is_empty() {
            bail!("the T3 bearer file is empty");
        }
        Ok(bearer)
    }

    pub async fn thread(&self, thread_id: &str) -> Result<Thread> {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|()| anyhow!("T3 base URL cannot carry a path"))?
            .extend(["api", "orchestration", "threads", thread_id]);
        let response = self
            .http
            .get(url)
            .bearer_auth(self.bearer()?)
            .header("x-t3-orchestration-protocol", PROTOCOL)
            .send()
            .await
            .context("T3 thread request failed")?;
        let status = response.status();
        if !status.is_success() {
            bail!("T3 thread {thread_id} answered {status}");
        }
        let body: Value = response
            .json()
            .await
            .context("T3 thread body is not JSON")?;
        let projection = body
            .get("projection")
            .cloned()
            .ok_or_else(|| anyhow!("T3 thread {thread_id} has no projection"))?;
        Ok(Thread { projection })
    }

    async fn socket_url(&self) -> Result<Url> {
        let mut ticket_url = self.base.clone();
        ticket_url.set_path("/api/auth/websocket-ticket");
        let response = self
            .http
            .post(ticket_url)
            .bearer_auth(self.bearer()?)
            .json(&json!({}))
            .send()
            .await
            .context("T3 websocket ticket request failed")?;
        let status = response.status();
        if !status.is_success() {
            bail!("T3 websocket ticket answered {status}");
        }
        let body: Value = response
            .json()
            .await
            .context("T3 ticket body is not JSON")?;
        let ticket = body
            .get("ticket")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("T3 ticket response has no ticket"))?;
        let mut url = self.base.clone();
        let scheme = if self.base.scheme() == "https" {
            "wss"
        } else {
            "ws"
        };
        url.set_scheme(scheme)
            .map_err(|()| anyhow!("cannot derive the T3 websocket URL"))?;
        url.set_path("/ws");
        url.query_pairs_mut()
            .clear()
            .append_pair("orchestrationProtocol", PROTOCOL)
            .append_pair("wsTicket", ticket);
        Ok(url)
    }

    async fn dispatch(&self, command: Value) -> Result<()> {
        let url = self.socket_url().await?;
        let (mut socket, _) = tokio::time::timeout(
            REQUEST_TIMEOUT,
            tokio_tungstenite::connect_async(url.as_str()),
        )
        .await
        .context("T3 websocket connect timed out")?
        .context("T3 websocket connect failed")?;
        let request = json!({
            "_tag": "Request",
            "id": "1",
            "tag": "orchestration.dispatchCommand",
            "payload": command,
            "headers": [],
        });
        socket
            .send(Message::Text(request.to_string().into()))
            .await
            .context("cannot send the T3 command")?;
        let outcome = tokio::time::timeout(REQUEST_TIMEOUT, read_exit(&mut socket))
            .await
            .context("T3 command reply timed out")?;
        let _ = socket.close(None).await;
        outcome
    }

    pub async fn send_message(&self, thread: &Thread, text: &str) -> Result<String> {
        let message_id = uuid::Uuid::new_v4().to_string();
        let command = json!({
            "type": "message.dispatch",
            "createdBy": "agent",
            "creationSource": "server",
            "commandId": uuid::Uuid::new_v4().to_string(),
            "threadId": thread.id()?,
            "messageId": message_id,
            "text": text,
            "attachments": [],
            "modelSelection": thread.model_selection()?,
            "dispatchMode": {"type": "queue_after_active"},
        });
        self.dispatch(command).await?;
        Ok(message_id)
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn read_exit(socket: &mut Socket) -> Result<()> {
    while let Some(frame) = socket.next().await {
        let text = match frame.context("T3 websocket read failed")? {
            Message::Text(text) => text.to_string(),
            Message::Close(_) => bail!("T3 closed the websocket before replying"),
            _ => continue,
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        match value.get("_tag").and_then(Value::as_str) {
            Some("Ping") => {
                socket
                    .send(Message::Text(json!({"_tag": "Pong"}).to_string().into()))
                    .await
                    .context("cannot answer the T3 ping")?;
            }
            Some("Exit") if value.get("requestId").and_then(Value::as_str) == Some("1") => {
                let exit = value.get("exit").cloned().unwrap_or(Value::Null);
                if exit.get("_tag").and_then(Value::as_str) == Some("Success") {
                    return Ok(());
                }
                let cause = exit.get("cause").map(Value::to_string).unwrap_or_default();
                bail!("T3 rejected the command: {}", truncate(&cause, 400));
            }
            _ => {}
        }
    }
    bail!("T3 websocket ended without a reply")
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

impl Thread {
    fn id(&self) -> Result<Value> {
        self.projection
            .pointer("/thread/id")
            .cloned()
            .ok_or_else(|| anyhow!("T3 thread projection has no id"))
    }

    fn model_selection(&self) -> Result<Value> {
        self.projection
            .pointer("/thread/modelSelection")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(|| anyhow!("T3 thread has no model selection"))
    }

    fn runs(&self) -> impl Iterator<Item = &Value> {
        self.projection
            .get("runs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
    }

    pub fn is_idle(&self) -> bool {
        !self.runs().any(|run| {
            run.get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| ACTIVE_RUN_STATES.contains(&status))
        })
    }

    pub fn compaction(&self, message_id: &str) -> Compaction {
        let Some(run) = self
            .runs()
            .find(|run| run.get("userMessageId").and_then(Value::as_str) == Some(message_id))
        else {
            return Compaction::Waiting;
        };
        let run_id = run.get("id").and_then(Value::as_str).unwrap_or_default();
        let status = run
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let item = self
            .projection
            .get("turnItems")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|item| {
                item.get("type").and_then(Value::as_str) == Some("compaction")
                    && item.get("runId").and_then(Value::as_str) == Some(run_id)
            });
        if let Some(item) = item {
            let item_status = item
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if item_status == "completed" {
                return Compaction::Confirmed(format!(
                    "item {} title \"{title}\" tokens {} -> {}",
                    item.get("id").and_then(Value::as_str).unwrap_or_default(),
                    item.get("beforeTokenCount")
                        .map(Value::to_string)
                        .unwrap_or_default(),
                    item.get("afterTokenCount")
                        .map(Value::to_string)
                        .unwrap_or_default(),
                ));
            }
            if item_status == "failed" {
                return Compaction::Failed(format!("compaction item failed, title \"{title}\""));
            }
        }
        if FAILED_RUN_STATES.contains(&status) {
            return Compaction::Failed(format!("compact run ended {status}"));
        }
        if status == "completed" && item.is_none() {
            return Compaction::Failed("compact run completed without a compaction item".into());
        }
        Compaction::Waiting
    }
}
