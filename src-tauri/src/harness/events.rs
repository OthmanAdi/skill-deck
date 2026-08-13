// @agent-context: Streamed harness events.
// Emitted via Tauri `app.emit("harness-event", ...)` (frontend) and as JSONL
// lines to telemetry path (CLI). One source of truth for both UIs.
//
// `rename_all_fields` matters: without it serde camelCases only the VARIANT
// names, and fields went over the wire as `session_id` / `call_id` while the
// frontend types (src/lib/types/harness.ts) declare `sessionId` / `callId`.
// That silent mismatch broke tool-card correlation and envelope keying.
// All IPC types are camelCase per house rules.

use crate::harness::error::HarnessErrorKind;
use crate::harness::tools::ToolResult;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HarnessEvent {
    SessionStarted {
        session_id: String,
    },
    PreModel {
        session_id: String,
        model: String,
        message_count: usize,
        est_tokens: u32,
    },
    ContentDelta {
        session_id: String,
        text: String,
    },
    ToolCallStart {
        session_id: String,
        call_id: String,
        name: String,
        arguments_json: String,
    },
    ToolCallResult {
        session_id: String,
        call_id: String,
        name: String,
        result: ToolResult,
    },
    ToolCallError {
        session_id: String,
        call_id: String,
        name: String,
        message: String,
    },
    ToolCallVetoed {
        session_id: String,
        call_id: String,
        name: String,
        reason: String,
    },
    Compaction {
        session_id: String,
        strategy: String,
        freed_tokens: u32,
        before: u32,
        after: u32,
    },
    Spilled {
        session_id: String,
        block_id: String,
        byte_count: u64,
        path: String,
    },
    PostModel {
        session_id: String,
        model: String,
        finish_reason: String,
        in_tokens: u32,
        out_tokens: u32,
        cost_usd: Option<f64>,
    },
    FinalEnvelope {
        session_id: String,
        suggested_next_prompts: Vec<String>,
        sources: Vec<EnvelopeSource>,
        skills_referenced: Vec<String>,
    },
    Finish {
        session_id: String,
        reason: String,
        duration_ms: u64,
    },
    Error {
        session_id: String,
        message: String,
        /// Machine-readable category so the frontend can branch UI without
        /// string-matching `message`. Mirrors `commands/updates.rs` errorKind.
        error_kind: HarnessErrorKind,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvelopeSource {
    pub title: String,
    pub url: String,
    pub kind: String, // "web", "marketplace", "local-skill"
}
