// @agent-context: ReAct agent harness — top-level module.
//
// Layout (per .planning/harness-spec.md §1):
//
//   harness/
//   ├── mod.rs
//   ├── builtin.rs          // built-in local model: download + llama-server sidecar
//   ├── config.rs           // HarnessSettings + ProviderConfig
//   ├── context/            // context-window resolution + turn/tool budgets
//   ├── model_directory.rs  // ModelProfile + fetch+merge+cache from models.dev + ai-model-directory
//   ├── providers.rs        // resolve(ProviderConfig) -> rig CompletionModel
//   ├── tokens.rs           // TokenizerKind + Counter
//   ├── memory/             // ConversationMemory + compaction pipeline (inline, not hook)
//   ├── hooks.rs            // TelemetryHook + LoggingHook impl rig PromptHook + ToolCallHook
//   ├── tools/              // ToolResult envelope + 10 concrete tools
//   ├── orchestrator.rs     // assembles the main rig Agent
//   ├── agent.rs            // run_turn — the ReAct loop, calls compaction + hooks
//   ├── intent.rs           // item-count regex parser
//   ├── error.rs            // HarnessErrorKind taxonomy for the IPC boundary
//   ├── events.rs           // HarnessEvent enum
//   ├── telemetry.rs        // JSONL writer to %APPDATA%/skill-deck/telemetry/
//   ├── cancel.rs           // CancelRegistry
//   ├── session.rs          // HarnessSession persistence
//   └── mock.rs             // MockProvider for fast eval cycles

#![allow(dead_code)] // skeleton — every module is a placeholder until its phase fills it

pub mod agent;
pub mod builtin;
pub mod cancel;
pub mod config;
pub mod context;
pub mod error;
pub mod events;
pub mod hooks;
pub mod intent;
pub mod memory;
pub mod mock;
pub mod model_directory;
pub mod orchestrator;
pub mod providers;
pub mod session;
pub mod source_identity;
pub mod step_policy;
pub mod telemetry;
pub mod tokens;
pub mod tools;
