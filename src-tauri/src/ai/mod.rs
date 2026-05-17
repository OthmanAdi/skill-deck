// @agent-context: AI module — pluggable LLM providers + skill-aware agent runtime.
//
// LAYERING:
// ┌────────────────────────────────────────────────────────┐
// │ commands/ai.rs   (Tauri IPC surface)                    │
// ├────────────────────────────────────────────────────────┤
// │ ai::agent         (tool-calling loop, intent routing)   │
// ├────────────────────────────────────────────────────────┤
// │ ai::session       (persisted conversation thread)       │
// ├────────────────────────────────────────────────────────┤
// │ ai::registry      (runtime provider lookup + config)    │
// ├────────────────────────────────────────────────────────┤
// │ ai::provider      (LlmProvider trait + shared types)    │
// ├────────────────────────────────────────────────────────┤
// │ ai::ollama  /  ai::azure_foundry  (wire impls)          │
// └────────────────────────────────────────────────────────┘
//
// The trait keeps wire encoding (Ollama ndjson vs Azure SSE) hidden so the
// agent loop and tool dispatcher work against one normalized stream.

pub mod agent;
pub mod azure_foundry;
pub mod ollama;
pub mod provider;
pub mod registry;
pub mod session;
pub mod tools;

pub use agent::{run_agent_turn, AgentTurnRequest, AgentTurnResult};
#[allow(unused_imports)]
pub use provider::{
    ChatChunk, ChatMessage, ChatRequest, ChatRole, LlmProvider, ModelInfo, ProviderConfig,
    ProviderError, ProviderHealth, ProviderId, ToolCall, ToolDefinition,
};
pub use registry::{build_provider, ProviderRegistrySnapshot};
pub use session::AiSession;
