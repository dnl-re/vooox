use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{protocol::WebSocketConfig, Message},
};

const WS_MAX_BYTES: usize = 100 * 1024 * 1024; // 100 MB — large recordings + base64 overhead

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Ok { model: String, language: String, device: String },
    Models { list: Vec<String> },
    ConfigOk,
    Segment { text: String, start: f32, end: f32 },
    Done { full_text: String, language: String, duration_ms: u64 },
    Bye,
    Error { msg: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Health,
    Models,
    Config { model: String, language: String },
    Transcribe { audio_b64: String },
    Shutdown,
}

pub struct WhisperClient {
    port: u16,
}

impl WhisperClient {
    pub fn new(port: u16) -> Self {
        WhisperClient { port }
    }

    async fn connect(
        &self,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        String,
    > {
        let url = format!("ws://127.0.0.1:{}", self.port);
        let mut config = WebSocketConfig::default();
        config.max_message_size = Some(WS_MAX_BYTES);
        config.max_frame_size = Some(WS_MAX_BYTES);
        connect_async_with_config(&url, Some(config), false)
            .await
            .map(|(ws, _)| ws)
            .map_err(|e| format!("ws connect: {e}"))
    }

    pub async fn health(&self) -> Result<ServerMsg, String> {
        let mut ws = self.connect().await?;
        send_msg(&mut ws, &ClientMsg::Health).await?;
        recv_msg(&mut ws).await
    }

    pub async fn models(&self) -> Result<Vec<String>, String> {
        let mut ws = self.connect().await?;
        send_msg(&mut ws, &ClientMsg::Models).await?;
        match recv_msg(&mut ws).await? {
            ServerMsg::Models { list } => Ok(list),
            other => Err(format!("unexpected: {other:?}")),
        }
    }

    pub async fn set_config(&self, model: &str, language: &str) -> Result<(), String> {
        let mut ws = self.connect().await?;
        send_msg(
            &mut ws,
            &ClientMsg::Config {
                model: model.into(),
                language: language.into(),
            },
        )
        .await?;
        match recv_msg(&mut ws).await? {
            ServerMsg::ConfigOk => Ok(()),
            other => Err(format!("unexpected: {other:?}")),
        }
    }

    /// Transcribe WAV bytes, streaming segments via callback.
    /// Returns the full concatenated text after all segments arrive.
    pub async fn transcribe<F>(
        &self,
        wav_bytes: &[u8],
        mut on_segment: F,
    ) -> Result<String, String>
    where
        F: FnMut(String),
    {
        let audio_b64 = base64::engine::general_purpose::STANDARD.encode(wav_bytes);
        let mut ws = self.connect().await?;
        send_msg(&mut ws, &ClientMsg::Transcribe { audio_b64 }).await?;

        let mut full_text = String::new();
        loop {
            match recv_msg(&mut ws).await? {
                ServerMsg::Segment { text, .. } => {
                    on_segment(text.clone());
                    if !full_text.is_empty() {
                        full_text.push(' ');
                    }
                    full_text.push_str(&text);
                }
                ServerMsg::Done { full_text: ft, .. } => {
                    if full_text.is_empty() {
                        full_text = ft;
                    }
                    break;
                }
                ServerMsg::Error { msg } => return Err(msg),
                other => return Err(format!("unexpected: {other:?}")),
            }
        }
        Ok(full_text)
    }
}

async fn send_msg<S>(ws: &mut S, msg: &ClientMsg) -> Result<(), String>
where
    S: SinkExt<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let json = serde_json::to_string(msg).map_err(|e| e.to_string())?;
    ws.send(Message::Text(json.into()))
        .await
        .map_err(|e| format!("ws send: {e}"))
}

async fn recv_msg<S>(ws: &mut S) -> Result<ServerMsg, String>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    match ws.next().await {
        Some(Ok(Message::Text(text))) => {
            serde_json::from_str(&text).map_err(|e| format!("parse server msg: {e}\nraw: {text}"))
        }
        Some(Ok(other)) => Err(format!("unexpected ws frame: {other:?}")),
        Some(Err(e)) => Err(format!("ws recv: {e}")),
        None => Err("connection closed".into()),
    }
}

// ── wait for sidecar to be ready ──────────────────────────────────────────

pub async fn wait_for_ready(port: u16, timeout_secs: u64) -> Result<(), String> {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let client = WhisperClient::new(port);
    loop {
        match client.health().await {
            Ok(_) => return Ok(()),
            Err(_) => {
                if Instant::now() >= deadline {
                    return Err(format!("sidecar not ready after {timeout_secs}s"));
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_msg_serializes() {
        let msg = ClientMsg::Health;
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("\"health\""));
    }

    #[test]
    fn server_segment_deserializes() {
        let raw = r#"{"type":"segment","text":"Hallo","start":0.0,"end":1.2}"#;
        let msg: ServerMsg = serde_json::from_str(raw).unwrap();
        match msg {
            ServerMsg::Segment { text, .. } => assert_eq!(text, "Hallo"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn server_done_deserializes() {
        let raw = r#"{"type":"done","full_text":"Hallo Welt","language":"de","duration_ms":500}"#;
        let msg: ServerMsg = serde_json::from_str(raw).unwrap();
        match msg {
            ServerMsg::Done { full_text, .. } => assert_eq!(full_text, "Hallo Welt"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn base64_audio_roundtrip() {
        let data = b"RIFF fake wav data";
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        let decoded = base64::engine::general_purpose::STANDARD.decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }
}
