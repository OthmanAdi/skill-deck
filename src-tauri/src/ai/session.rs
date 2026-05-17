// @agent-context: Persisted AI chat session.
//
// One JSON file per session at
//   {config_dir}/skill-deck/ai-sessions/{session_id}.json
// matches the convention used for archive snapshots.

use super::provider::{ChatMessage, ChatRole, ToolCall};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionMessage {
    pub id: String,
    pub role: ChatRole,
    pub content: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    pub created_at: u64,
}

impl AiSessionMessage {
    pub fn to_chat(&self) -> ChatMessage {
        ChatMessage {
            role: self.role,
            content: self.content.clone(),
            tool_calls: self.tool_calls.clone(),
            tool_call_id: self.tool_call_id.clone(),
            tool_name: self.tool_name.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSession {
    pub id: String,
    pub title: String,
    pub provider_id: String,
    pub model: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub messages: Vec<AiSessionMessage>,
}

impl AiSession {
    pub fn new(id: String, title: String, provider_id: String, model: String) -> Self {
        let now = now_secs();
        Self {
            id,
            title,
            provider_id,
            model,
            created_at: now,
            updated_at: now,
            messages: Vec::new(),
        }
    }
}

pub fn sessions_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("skill-deck").join("ai-sessions")
}

pub fn session_path(id: &str) -> PathBuf {
    sessions_dir().join(format!("{}.json", sanitize_id(id)))
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn save(session: &AiSession) -> std::io::Result<()> {
    let dir = sessions_dir();
    fs::create_dir_all(&dir)?;
    let path = session_path(&session.id);
    let json = serde_json::to_string_pretty(session)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    fs::write(path, json)
}

pub fn load(id: &str) -> std::io::Result<AiSession> {
    let path = session_path(id);
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}

pub fn list() -> Vec<AiSession> {
    let dir = sessions_dir();
    let mut out = Vec::new();
    let read = match fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return out,
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(session) = serde_json::from_str::<AiSession>(&raw) {
                out.push(session);
            }
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    out
}

pub fn delete(id: &str) -> std::io::Result<()> {
    let path = session_path(id);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
