// @agent-context: ConversationMemory — the harness's owned copy of the message
// history. Tracks user / assistant / tool records with FULL tool metadata so
// the UI can render the persisted history as ToolCards (not text rows) and
// future turns get the same context the agent had in this turn.

use crate::harness::tools::ToolResult;
use rig::message::{AssistantContent, Message, UserContent};
use rig::OneOrMany;
use serde::{Deserialize, Serialize};

/// Hard per-entry cap on stored tool output (chars of serialized envelope).
/// An adversarially huge tool result (a model asking `web_fetch` for a 2 MB
/// page, a marketplace dump, …) must not balloon the session file or the
/// compaction corpus: anything over this is truncated with a marker BEFORE it
/// is stored. Deliberately far above `tool_result_spill_threshold_chars`
/// (default 8000) so spill-to-disk still handles the normal big-result case —
/// this cap is the last line of defense, not the tuning knob.
pub const MAX_TOOL_ENTRY_CHARS: usize = 60_000;

/// Marker appended to any truncated stored tool payload.
const TRUNCATION_MARKER: &str = "…[tool output truncated: exceeded per-entry storage cap]";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMemory {
    pub session_id: String,
    pub messages: Vec<MessageRecord>,
    pub running_summary: Option<String>,
    pub spilled_blocks: Vec<SpillRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRecord {
    /// Stable id (`u-…`, `a-…`, `t-…`).
    pub id: String,
    /// "user" | "assistant" | "tool"
    pub role: String,
    /// Plain text for user / assistant. For tool records this holds the
    /// serialized ToolResult JSON the model received (kept for compaction
    /// token estimation).
    pub text: String,
    /// Set by compaction once an old tool record's body has been replaced
    /// with a `[cleared]` stub.
    #[serde(default)]
    pub cleared: bool,
    /// Path to the on-disk spill file when this record's body has been
    /// moved out of the in-memory thread.
    #[serde(default)]
    pub spilled_path: Option<String>,
    pub created_at: u64,

    // -- tool-record-only fields --
    /// The tool name (`search_skills`, `web_search`, …) when role == "tool".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The raw arguments JSON the model emitted for this tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_json: Option<String>,
    /// Stable id provided by rig to tie this record back to the streaming
    /// `toolCallStart` event, so the UI can dedupe pending + persisted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Full ToolResult envelope. Optional because errored tools have no envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<ToolResult>,
    /// Error message when the tool dispatch failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpillRef {
    pub block_id: String,
    pub path: String,
    pub byte_count: u64,
}

impl ConversationMemory {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            messages: Vec::new(),
            running_summary: None,
            spilled_blocks: Vec::new(),
        }
    }

    pub fn push_user(&mut self, text: impl Into<String>) {
        self.messages.push(MessageRecord {
            id: new_id("u"),
            role: "user".into(),
            text: text.into(),
            cleared: false,
            spilled_path: None,
            created_at: now_secs(),
            name: None,
            args_json: None,
            tool_call_id: None,
            tool_result: None,
            tool_error: None,
        });
    }

    pub fn push_assistant(&mut self, text: impl Into<String>) {
        self.messages.push(MessageRecord {
            id: new_id("a"),
            role: "assistant".into(),
            text: text.into(),
            cleared: false,
            spilled_path: None,
            created_at: now_secs(),
            name: None,
            args_json: None,
            tool_call_id: None,
            tool_result: None,
            tool_error: None,
        });
    }

    /// Append a successful tool call + result to the history. `text` is the
    /// JSON the model sees (the serialized ToolResult), `result` is the full
    /// envelope the UI renders. Both views are bounded by
    /// `MAX_TOOL_ENTRY_CHARS` — see `cap_tool_result`.
    pub fn push_tool_success(
        &mut self,
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        args_json: impl Into<String>,
        result: ToolResult,
    ) {
        let result = cap_tool_result(result);
        let text = serde_json::to_string(&result).unwrap_or_else(|_| "{}".into());
        self.messages.push(MessageRecord {
            id: new_id("t"),
            role: "tool".into(),
            text,
            cleared: false,
            spilled_path: None,
            created_at: now_secs(),
            name: Some(name.into()),
            args_json: Some(args_json.into()),
            tool_call_id: Some(tool_call_id.into()),
            tool_result: Some(result),
            tool_error: None,
        });
    }

    /// Append a failed tool call. Keeps the error visible in the persisted
    /// session so the user can see what went wrong. Error text is bounded —
    /// some failures echo entire response bodies.
    pub fn push_tool_error(
        &mut self,
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        args_json: impl Into<String>,
        error: impl Into<String>,
    ) {
        const MAX_ERROR_CHARS: usize = 4_000;
        let mut error_str = error.into();
        if error_str.chars().count() > MAX_ERROR_CHARS {
            error_str = error_str.chars().take(MAX_ERROR_CHARS).collect();
            error_str.push_str(TRUNCATION_MARKER);
        }
        let text = serde_json::json!({ "error": error_str.clone() }).to_string();
        self.messages.push(MessageRecord {
            id: new_id("t"),
            role: "tool".into(),
            text,
            cleared: false,
            spilled_path: None,
            created_at: now_secs(),
            name: Some(name.into()),
            args_json: Some(args_json.into()),
            tool_call_id: Some(tool_call_id.into()),
            tool_result: None,
            tool_error: Some(error_str),
        });
    }

    /// Token-estimation corpus. Cleared / spilled records contribute only
    /// their stub pointer text.
    pub fn corpus_for_tokens(&self) -> String {
        let mut buf = String::new();
        if let Some(s) = self.running_summary.as_deref() {
            buf.push_str("[running_summary]\n");
            buf.push_str(s);
            buf.push('\n');
        }
        for m in &self.messages {
            buf.push_str(&m.role);
            buf.push_str(": ");
            buf.push_str(&m.text);
            buf.push('\n');
        }
        buf
    }

    /// Build the rig history snapshot for the next agent turn.
    ///
    /// **Tool records are intentionally omitted.** OpenAI's chat-completions
    /// API requires that any message with `role: "tool"` is preceded by an
    /// `assistant` message carrying the corresponding `tool_calls` block. We
    /// persist tool results as bare records (so the UI can render ToolCards
    /// across reloads), but we never persist the matching assistant tool_calls
    /// block, so replaying them produces:
    ///
    ///   `Invalid parameter: messages with role 'tool' must be a response to
    ///    a preceeding message with 'tool_calls'.` (Azure HTTP 400)
    ///
    /// Skipping the tool records keeps the round-trip clean across providers
    /// (Anthropic, Azure OpenAI, Ollama). The final assistant text from each
    /// prior turn already incorporates the tool output, so the model retains
    /// the answer — and it can re-call tools on the new turn if it needs the
    /// raw payload again. The persisted records are still kept on disk for
    /// audit / UI rendering, just not pushed into the next prompt.
    pub fn to_rig_history(&self) -> Vec<Message> {
        let mut out: Vec<Message> = Vec::with_capacity(self.messages.len() + 1);
        if let Some(s) = self.running_summary.as_deref() {
            out.push(Message::User {
                content: OneOrMany::one(UserContent::text(format!(
                    "Prior conversation summary:\n{s}"
                ))),
            });
        }
        for m in &self.messages {
            match m.role.as_str() {
                "user" => out.push(Message::User {
                    content: OneOrMany::one(UserContent::text(m.text.clone())),
                }),
                "assistant" => out.push(Message::Assistant {
                    id: None,
                    content: OneOrMany::one(AssistantContent::text(m.text.clone())),
                }),
                // "tool" records skipped — see doc comment above.
                _ => {}
            }
        }
        out
    }

    pub fn record_count(&self) -> usize {
        self.messages.len()
    }
}

/// Enforce `MAX_TOOL_ENTRY_CHARS` on a tool envelope before storage. The
/// `data` payload carries the bulk, so an oversized envelope keeps its
/// title/kind/snippet (what the UI header renders) and swaps `data` for a
/// bounded preview object with an explicit truncation marker.
fn cap_tool_result(result: ToolResult) -> ToolResult {
    let serialized_chars = serde_json::to_string(&result)
        .map(|s| s.chars().count())
        .unwrap_or(0);
    if serialized_chars <= MAX_TOOL_ENTRY_CHARS {
        return result;
    }
    let preview: String = serde_json::to_string(&result.data)
        .unwrap_or_default()
        .chars()
        .take(MAX_TOOL_ENTRY_CHARS / 2)
        .collect();
    ToolResult {
        data: serde_json::json!({
            "truncated": true,
            "originalChars": serialized_chars,
            "note": TRUNCATION_MARKER,
            "preview": preview,
        }),
        ..result
    }
}

fn new_id(prefix: &str) -> String {
    use uuid::Uuid;
    format!("{prefix}-{}", Uuid::new_v4())
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn giant_result(chars: usize) -> ToolResult {
        ToolResult {
            title: "web_fetch(https://example.com)".into(),
            kind: "web-fetch".into(),
            snippet: "snippet".into(),
            data: serde_json::json!({ "body": "x".repeat(chars) }),
            items_count: Some(1),
            source: Some("web".into()),
            elapsed_ms: 5,
        }
    }

    #[test]
    fn small_tool_results_are_stored_verbatim() {
        let mut m = ConversationMemory::new("s");
        m.push_tool_success("c1", "web_fetch", "{}", giant_result(100));
        let rec = &m.messages[0];
        let result = rec.tool_result.as_ref().expect("envelope kept");
        assert_eq!(result.data["body"].as_str().map(|s| s.len()), Some(100));
        assert!(rec.text.len() < MAX_TOOL_ENTRY_CHARS);
    }

    #[test]
    fn oversized_tool_results_are_truncated_with_marker() {
        let mut m = ConversationMemory::new("s");
        // ~200k chars of body — 3x over the cap.
        m.push_tool_success(
            "c1",
            "web_fetch",
            "{}",
            giant_result(MAX_TOOL_ENTRY_CHARS * 3),
        );
        let rec = &m.messages[0];
        // Stored model-view text is bounded.
        assert!(
            rec.text.chars().count() <= MAX_TOOL_ENTRY_CHARS + 1_000,
            "stored text must be capped, got {} chars",
            rec.text.chars().count()
        );
        let result = rec.tool_result.as_ref().expect("envelope kept");
        assert_eq!(result.data["truncated"], true);
        assert_eq!(result.data["note"].as_str(), Some(TRUNCATION_MARKER));
        assert!(result.data["originalChars"].as_u64().unwrap_or(0) > MAX_TOOL_ENTRY_CHARS as u64);
        // Header metadata the UI renders survives truncation.
        assert_eq!(result.title, "web_fetch(https://example.com)");
        assert_eq!(result.kind, "web-fetch");
    }

    #[test]
    fn tool_error_text_is_bounded() {
        let mut m = ConversationMemory::new("s");
        m.push_tool_error("c1", "web_fetch", "{}", "e".repeat(100_000));
        let rec = &m.messages[0];
        let err = rec.tool_error.as_ref().expect("error kept");
        assert!(err.chars().count() < 5_000, "error must be capped");
        assert!(err.ends_with(TRUNCATION_MARKER));
    }

    #[test]
    fn to_rig_history_skips_tool_records_and_prepends_summary() {
        let mut m = ConversationMemory::new("s");
        m.running_summary = Some("prior summary".into());
        m.push_user("q");
        m.push_tool_success("c1", "search_skills", "{}", giant_result(10));
        m.push_assistant("a");
        let history = m.to_rig_history();
        // summary + user + assistant; tool record omitted.
        assert_eq!(history.len(), 3);
    }
}
