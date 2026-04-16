use serde::{Deserialize, Serialize};

/// Universal message envelope used for all communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub command_id: String,
    pub session_id: String,
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    pub command: String,
    pub payload: serde_json::Value,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    Request,
    Response,
    Event,
}

// ── Request Payloads ──────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct SendInputPayload {
    pub input: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SendKeyPayload {
    pub key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReadFilePayload {
    pub path: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    2000
}

#[derive(Debug, Clone, Deserialize)]
pub struct WriteFilePayload {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListDirPayload {
    #[serde(default = "default_path")]
    pub path: String,
}

fn default_path() -> String {
    ".".to_string()
}

// ── Response Payloads ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ScreenshotResponse {
    pub screenshot: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CwdResponse {
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShellResponse {
    pub shell: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadFileResponse {
    pub content: String,
    pub encoding: String,
    pub total_lines: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct WriteFileResponse {
    pub success: bool,
    pub bytes_written: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListDirResponse {
    pub entries: Vec<DirEntry>,
}

// ── Event Payloads ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct HeartbeatPayload {
    pub uptime: u64,
    pub cwd: String,
    pub shell: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HelpEventPayload {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

// ── Helpers ───────────────────────────────────────────────────────

impl MessageEnvelope {
    /// Build a response envelope echoing the command_id and command from the request.
    pub fn response(
        request: &MessageEnvelope,
        session_id: &str,
        payload: serde_json::Value,
        error: Option<String>,
    ) -> Self {
        Self {
            command_id: request.command_id.clone(),
            session_id: session_id.to_string(),
            msg_type: MessageType::Response,
            command: request.command.clone(),
            payload,
            timestamp: now_iso8601(),
            error,
        }
    }

    /// Build an event envelope.
    pub fn event(
        session_id: &str,
        command: &str,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            command_id: generate_command_id(),
            session_id: session_id.to_string(),
            msg_type: MessageType::Event,
            command: command.to_string(),
            payload,
            timestamp: now_iso8601(),
            error: None,
        }
    }
}

/// Generate a simple unique command_id (hex string).
/// Since we don't have uuid in WASI easily, we use a counter.
use std::sync::atomic::{AtomicU64, Ordering};
static COMMAND_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn generate_command_id() -> String {
    let id = COMMAND_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{:016x}", id)
}

/// Returns current timestamp in ISO 8601 format.
/// In WASI we don't have full std::time, so we return a placeholder.
/// The external controller can override timestamps if needed.
pub fn now_iso8601() -> String {
    // WASI supports SystemTime
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Simple formatting: seconds since epoch as a string, not full ISO 8601
    // but good enough for protocol purposes
    format!("{}", secs)
}
