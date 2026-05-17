// @agent-context: Shared types every LlmProvider speaks.
//
// We normalize on the OpenAI chat-completions message shape because both
// Ollama (/api/chat) and Azure Foundry (/openai/v1/chat/completions) take
// that shape natively. ChatChunk is the streaming unit yielded by the
// provider, with content deltas, tool-call deltas, finish reasons, or
// errors all multiplexed onto a single async stream.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable identifier for a configured provider instance.
pub type ProviderId = String;

/// Persisted, user-editable provider config.
///
/// `kind` selects the wire impl (`"ollama"` or `"azure-foundry"` today).
/// `endpoint` / `api_key` / `deployment` are optional and only relevant
/// to certain kinds — the trait impl pulls what it needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: ProviderId,
    pub kind: String,
    pub label: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub deployment: Option<String>,
    /// API version (Azure Foundry only). Defaults handled in impl.
    #[serde(default)]
    pub api_version: Option<String>,
    /// Default model when caller doesn't specify.
    #[serde(default)]
    pub default_model: Option<String>,
    /// Optional human note from settings UI ("home box", "work key", etc.).
    #[serde(default)]
    pub notes: Option<String>,
}

/// Health probe result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealth {
    pub ok: bool,
    pub detail: Option<String>,
}

/// A model exposed by a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub context_window: Option<u32>,
    #[serde(default)]
    pub supports_tools: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    /// When this message is an assistant message that issued tool calls.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// When this message is a tool response — id of the call it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// When this message is a tool response — name of the tool that ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON arguments string as produced by the model.
    pub arguments_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema for arguments.
    pub parameters: serde_json::Value,
}

/// One inbound request to a provider for a streaming chat completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

/// One streamed chunk from the provider, in normalized form.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatChunk {
    /// Incremental assistant text.
    ContentDelta { text: String },
    /// Incremental or complete tool call. `arguments_json` may arrive as
    /// fragments — the agent concatenates them keyed by `id`.
    ToolCallDelta {
        id: String,
        name: Option<String>,
        arguments_json_delta: String,
    },
    /// Stream ended cleanly.
    Finish { reason: String },
    /// Stream ended with an error message.
    Error { message: String },
}

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum ProviderError {
    #[error("network error: {0}")]
    Network(String),
    #[error("auth error: {0}")]
    Auth(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("not configured: {0}")]
    NotConfigured(String),
}

impl ProviderError {
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

/// Async sink the provider calls for every chunk in the stream.
pub type ChunkSink = Box<dyn Fn(ChatChunk) + Send + Sync>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn id(&self) -> &str;
    #[allow(dead_code)]
    fn kind(&self) -> &'static str;

    async fn health(&self) -> ProviderHealth;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;

    /// Stream a chat completion. Provider yields chunks via `on_chunk`.
    /// Returns once the upstream connection is closed (Finish or Error
    /// chunk emitted just before).
    async fn stream_chat(
        &self,
        req: ChatRequest,
        on_chunk: ChunkSink,
    ) -> Result<(), ProviderError>;
}
