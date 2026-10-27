use std::{collections::VecDeque, time::Duration};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::{net::TcpStream, time::Instant};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

use super::ObserverError;

const MAX_QUEUED_EVENTS: usize = 256;

pub(super) struct Cdp {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    id: u64,
    session: Option<String>,
    events: VecDeque<Value>,
    step: Duration,
}

pub(super) struct Point {
    x: f64,
    y: f64,
}

impl Cdp {
    pub(super) async fn connect(url: &str, step: Duration) -> Result<Self, ObserverError> {
        let (socket, _) = tokio::time::timeout(step, tokio_tungstenite::connect_async(url))
            .await
            .map_err(|_| ObserverError::Timeout)?
            .map_err(|_| ObserverError::Unavailable)?;
        Ok(Self {
            socket,
            id: 0,
            session: None,
            events: VecDeque::new(),
            step,
        })
    }

    pub(super) async fn open_page(&mut self, url: &str) -> Result<String, ObserverError> {
        let target = self
            .browser("Target.createTarget", json!({ "url": url }))
            .await?["targetId"]
            .as_str()
            .ok_or(ObserverError::Ui)?
            .to_owned();
        self.attach_page(&target).await?;
        Ok(target)
    }

    pub(super) async fn attach_page(&mut self, target: &str) -> Result<(), ObserverError> {
        let session = self
            .browser(
                "Target.attachToTarget",
                json!({ "targetId": target, "flatten": true }),
            )
            .await?["sessionId"]
            .as_str()
            .ok_or(ObserverError::Ui)?
            .to_owned();
        self.session = Some(session);
        for domain in ["Page.enable", "Runtime.enable", "DOM.enable"] {
            self.page(domain, json!({})).await?;
        }
        Ok(())
    }

    pub(super) async fn close_page(&mut self, target: &str) {
        let _ = self
            .browser("Target.closeTarget", json!({ "targetId": target }))
            .await;
    }

    pub(super) async fn browser(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, ObserverError> {
        self.call(method, params, None).await
    }

    pub(super) async fn page(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, ObserverError> {
        let session = self.session.clone().ok_or(ObserverError::Ui)?;
        self.call(method, params, Some(session)).await
    }

    pub(super) async fn eval(&mut self, expression: &str) -> Result<Value, ObserverError> {
        let result = self
            .page(
                "Runtime.evaluate",
                json!({ "expression": expression, "awaitPromise": true, "returnByValue": true }),
            )
            .await?;
        if result.get("exceptionDetails").is_some() {
            return Err(ObserverError::Ui);
        }
        Ok(result["result"]["value"].clone())
    }

    pub(super) async fn click(&mut self, point: Point) -> Result<(), ObserverError> {
        for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
            self.page(
                "Input.dispatchMouseEvent",
                json!({ "type": kind, "x": point.x, "y": point.y, "button": "left", "clickCount": 1 }),
            )
            .await?;
        }
        Ok(())
    }

    pub(super) async fn press(
        &mut self,
        key: &str,
        code: &str,
        virtual_key: u32,
    ) -> Result<(), ObserverError> {
        for kind in ["keyDown", "keyUp"] {
            self.page(
                "Input.dispatchKeyEvent",
                json!({ "type": kind, "key": key, "code": code, "windowsVirtualKeyCode": virtual_key }),
            )
            .await?;
        }
        Ok(())
    }

    pub(super) async fn type_text(
        &mut self,
        text: &str,
        delay: Duration,
    ) -> Result<(), ObserverError> {
        for character in text.chars() {
            let key = character.to_string();
            self.page(
                "Input.dispatchKeyEvent",
                json!({ "type": "keyDown", "key": key, "text": key }),
            )
            .await?;
            self.page(
                "Input.dispatchKeyEvent",
                json!({ "type": "keyUp", "key": key }),
            )
            .await?;
            tokio::time::sleep(delay).await;
        }
        Ok(())
    }

    pub(super) async fn wait_event(
        &mut self,
        method: &str,
        timeout: Duration,
    ) -> Result<Value, ObserverError> {
        if let Some(index) = self
            .events
            .iter()
            .position(|event| event["method"] == method)
        {
            return self.events.remove(index).ok_or(ObserverError::Ui);
        }
        let deadline = Instant::now() + timeout;
        loop {
            let message = self.read(deadline).await?;
            if message["method"] == method {
                return Ok(message);
            }
            self.queue(message);
        }
    }

    async fn call(
        &mut self,
        method: &str,
        params: Value,
        session: Option<String>,
    ) -> Result<Value, ObserverError> {
        self.id += 1;
        let id = self.id;
        let mut request = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::Text(request.to_string().into()))
            .await
            .map_err(|_| ObserverError::Unavailable)?;
        let deadline = Instant::now() + self.step;
        loop {
            let message = self.read(deadline).await?;
            if message["id"].as_u64() == Some(id) {
                if message.get("error").is_some() {
                    return Err(ObserverError::Ui);
                }
                return Ok(message["result"].clone());
            }
            if message.get("method").is_some() {
                self.queue(message);
            }
        }
    }

    async fn read(&mut self, deadline: Instant) -> Result<Value, ObserverError> {
        loop {
            let frame = tokio::time::timeout_at(deadline, self.socket.next())
                .await
                .map_err(|_| ObserverError::Timeout)?
                .ok_or(ObserverError::Unavailable)?
                .map_err(|_| ObserverError::Unavailable)?;
            match frame {
                Message::Text(text) => {
                    return serde_json::from_str(text.as_str()).map_err(|_| ObserverError::Ui)
                }
                Message::Close(_) => return Err(ObserverError::Unavailable),
                _ => continue,
            }
        }
    }

    fn queue(&mut self, event: Value) {
        if self.events.len() >= MAX_QUEUED_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

impl Point {
    pub(super) fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            x: value.get("x")?.as_f64()?,
            y: value.get("y")?.as_f64()?,
        })
    }
}
