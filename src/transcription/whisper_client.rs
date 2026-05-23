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
    Ready { model: String },
    Bye,
    Error { msg: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Health,
    Models,
    Config { model: String, language: String },
    EnsureModel { model: String },
    Transcribe { audio_b64: String },
    Shutdown,
}

pub struct WhisperClient {
    port: u16,
}

type WebSocket = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

impl WhisperClient {
    pub fn new(port: u16) -> Self {
        WhisperClient { port }
    }

    async fn open_websocket_connection(&self) -> Result<WebSocket, String> {
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
        let mut ws = self.open_websocket_connection().await?;
        send_msg(&mut ws, &ClientMsg::Health).await?;
        recv_msg(&mut ws).await
    }

    /// Triggers an explicit download of the named model. Blocks until HF
    /// finishes (which for `large-v3` can easily be 30+ minutes on a slow
    /// connection); the sidecar runs the download in a worker thread so the
    /// WS itself stays responsive.
    pub async fn ensure_model(&self, model: &str) -> Result<(), String> {
        let mut ws = self.open_websocket_connection().await?;
        send_msg(&mut ws, &ClientMsg::EnsureModel { model: model.into() }).await?;
        match recv_msg(&mut ws).await? {
            ServerMsg::Ready { .. } => Ok(()),
            ServerMsg::Error { msg } => Err(msg),
            other => Err(format!("unexpected: {other:?}")),
        }
    }

    pub async fn set_config(&self, model: &str, language: &str) -> Result<(), String> {
        let mut ws = self.open_websocket_connection().await?;
        send_msg(&mut ws, &ClientMsg::Config { model: model.into(), language: language.into() }).await?;
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
        on_segment: F,
    ) -> Result<String, String>
    where
        F: FnMut(String),
    {
        let audio_b64 = encode_audio_as_base64(wav_bytes);
        let mut ws = self.open_websocket_connection().await?;
        send_audio_to_server(&mut ws, audio_b64).await?;
        collect_transcription_segments(&mut ws, on_segment).await
    }
}

async fn send_audio_to_server(ws: &mut WebSocket, audio_b64: String) -> Result<(), String> {
    send_msg(ws, &ClientMsg::Transcribe { audio_b64 }).await
}

async fn collect_transcription_segments<F>(
    ws: &mut WebSocket,
    mut on_segment: F,
) -> Result<String, String>
where
    F: FnMut(String),
{
    let mut accumulated_text = String::new();
    loop {
        match recv_msg(ws).await? {
            ServerMsg::Segment { text, .. } => {
                on_segment(text.clone());
                append_segment_to_text(&mut accumulated_text, &text);
            }
            ServerMsg::Done { full_text, .. } => {
                return Ok(combine_segments_to_text(accumulated_text, full_text));
            }
            ServerMsg::Error { msg } => return Err(msg),
            other => return Err(format!("unexpected: {other:?}")),
        }
    }
}

fn append_segment_to_text(text: &mut String, segment: &str) {
    if !text.is_empty() {
        text.push(' ');
    }
    text.push_str(segment);
}

fn combine_segments_to_text(accumulated: String, server_full_text: String) -> String {
    if accumulated.is_empty() { server_full_text } else { accumulated }
}

fn encode_audio_as_base64(wav_bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(wav_bytes)
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
            Err(_) if has_timed_out(deadline) => {
                return Err(format!("sidecar not ready after {timeout_secs}s"));
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
        }
    }
}

fn has_timed_out(deadline: std::time::Instant) -> bool {
    std::time::Instant::now() >= deadline
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
        let encoded = encode_audio_as_base64(data);
        let decoded = base64::engine::general_purpose::STANDARD.decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }
}
