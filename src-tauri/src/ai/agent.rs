// @agent-context: Tool-calling agent loop.
//
// FLOW per turn:
//   1. Append user message to session.
//   2. Build ChatRequest = system prompt + history + tools.
//   3. Stream from provider, collect content_delta into assistant_text and
//      tool_call deltas into assembled_tool_calls.
//   4. When stream finishes:
//        - if tool_calls present: dispatch each, append tool messages,
//          re-stream with updated history (up to MAX_ITERATIONS).
//        - else: append assistant message and return.
//   5. Persist session.

use super::provider::{
    ChatChunk, ChatMessage, ChatRequest, ChatRole, ChunkSink, LlmProvider, ToolCall,
};
use super::session::{self, AiSession, AiSessionMessage};
use super::tools::{all_tools, dispatch_tool};
use crate::models::Skill;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const MAX_TOOL_ITERATIONS: usize = 6;
const DEFAULT_SYSTEM_PROMPT_BASE: &str = r#"You are Skill Deck's resident AI engineer. You help the user discover, understand, and combine the AI "skills" installed on their machine (markdown files that drive coding agents like Claude Code, Codex, Cursor, etc.), and you translate plain intent into ready-to-paste prompts for the user's coding agent of choice.

Behaviour by mode (decide which mode applies from the user's last turn):
- chat: normal Q&A — concise, no theatrics.
- skill-query: user asked about a specific skill or class of skills — call search_skills / list_skills / get_skill_detail before answering.
- brainstorm: user is exploring options — offer 2–4 concrete directions with trade-offs.
- prompt-build: user wants a prompt for their coding agent — call search_skills (if needed) to pick the right skills, then call render_prompt_for_coding_agent and surface the produced prompt verbatim in a fenced code block.

Rules:
- Prefer tool calls over speculation when the answer depends on what skills the user has installed.
- Don't invent skill ids — use the ones the tools return.
- Keep responses tight; the user reads diffs and prefers terse output.
- When you produce a final prompt for a coding agent, render it as a fenced code block labelled `prompt` and add a one-line note above explaining which skills it pulls in."#;

/// Inbound request from the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurnRequest {
    /// Session id. If empty, a new session is created.
    pub session_id: Option<String>,
    /// User text for this turn.
    pub user_text: String,
    /// Provider config id to use.
    pub provider_id: String,
    /// Model id to use (must belong to the provider).
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurnResult {
    pub session_id: String,
    pub assistant_text: String,
    pub tool_calls_made: usize,
}

/// Streaming event types emitted to the frontend during a turn.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AgentEvent {
    SessionStarted { session_id: String },
    ContentDelta { session_id: String, text: String },
    ToolCallStart { session_id: String, call_id: String, name: String, arguments_json: String },
    ToolCallResult { session_id: String, call_id: String, name: String, label: String, data: serde_json::Value },
    ToolCallError { session_id: String, call_id: String, name: String, message: String },
    Finish { session_id: String, reason: String },
    Error { session_id: String, message: String },
}

/// Emitter callback the caller (Tauri command) supplies — typically
/// `app_handle.emit("ai-agent-event", event)`.
pub type EventEmitter = Arc<dyn Fn(AgentEvent) + Send + Sync>;

/// Run one agent turn against `provider` using `skills` as the tool context.
pub async fn run_agent_turn(
    provider: Arc<dyn LlmProvider>,
    skills: Vec<Skill>,
    req: AgentTurnRequest,
    emit: EventEmitter,
) -> Result<AgentTurnResult, String> {
    let session_id = req
        .session_id
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let mut session = match session::load(&session_id) {
        Ok(s) => s,
        Err(_) => AiSession::new(
            session_id.clone(),
            derive_title(&req.user_text),
            provider.id().to_string(),
            req.model.clone(),
        ),
    };

    // Keep session.provider/model in sync with the active selection.
    session.provider_id = provider.id().to_string();
    session.model = req.model.clone();
    session.updated_at = session::now_secs();

    let user_msg = AiSessionMessage {
        id: Uuid::new_v4().to_string(),
        role: ChatRole::User,
        content: req.user_text.clone(),
        tool_calls: Vec::new(),
        tool_call_id: None,
        tool_name: None,
        created_at: session::now_secs(),
    };
    session.messages.push(user_msg);

    emit(AgentEvent::SessionStarted {
        session_id: session_id.clone(),
    });

    let system_prompt = build_system_prompt(&skills);
    let tools = all_tools();
    let mut tool_calls_made = 0usize;
    let mut assistant_text_final = String::new();

    for iteration in 0..MAX_TOOL_ITERATIONS {
        let mut messages: Vec<ChatMessage> = Vec::with_capacity(session.messages.len() + 1);
        messages.push(ChatMessage {
            role: ChatRole::System,
            content: system_prompt.clone(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_name: None,
        });
        for m in &session.messages {
            messages.push(m.to_chat());
        }

        let request = ChatRequest {
            model: req.model.clone(),
            messages,
            tools: tools.clone(),
            temperature: Some(0.4),
            max_tokens: None,
        };

        let collector = Arc::new(Mutex::new(StreamCollector::default()));
        let emit_for_stream = emit.clone();
        let session_id_for_stream = session_id.clone();
        let collector_for_stream = collector.clone();

        let sink: ChunkSink = Box::new(move |chunk: ChatChunk| {
            let mut state = match collector_for_stream.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            match chunk {
                ChatChunk::ContentDelta { text } => {
                    state.assistant_text.push_str(&text);
                    emit_for_stream(AgentEvent::ContentDelta {
                        session_id: session_id_for_stream.clone(),
                        text,
                    });
                }
                ChatChunk::ToolCallDelta {
                    id,
                    name,
                    arguments_json_delta,
                } => {
                    state.append_tool_delta(id, name, arguments_json_delta);
                }
                ChatChunk::Finish { reason } => {
                    state.finish_reason = Some(reason);
                }
                ChatChunk::Error { message } => {
                    state.error = Some(message);
                }
            }
        });

        if let Err(err) = provider.stream_chat(request, sink).await {
            let msg = err.user_message();
            emit(AgentEvent::Error {
                session_id: session_id.clone(),
                message: msg.clone(),
            });
            // Best-effort save before bailing.
            let _ = session::save(&session);
            return Err(msg);
        }

        let state = collector.lock().map(|g| g.clone()).unwrap_or_default();
        if let Some(err_msg) = state.error.clone() {
            emit(AgentEvent::Error {
                session_id: session_id.clone(),
                message: err_msg.clone(),
            });
            let _ = session::save(&session);
            return Err(err_msg);
        }

        // Snapshot assembled tool calls into ChatMessage form for history.
        let tool_calls: Vec<ToolCall> = state
            .tool_calls
            .iter()
            .filter(|tc| !tc.name.is_empty())
            .map(|tc| ToolCall {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments_json: tc.arguments_json.clone(),
            })
            .collect();

        if tool_calls.is_empty() {
            // Final assistant reply.
            assistant_text_final = state.assistant_text.clone();
            let assistant = AiSessionMessage {
                id: Uuid::new_v4().to_string(),
                role: ChatRole::Assistant,
                content: state.assistant_text.clone(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                tool_name: None,
                created_at: session::now_secs(),
            };
            session.messages.push(assistant);
            session.updated_at = session::now_secs();
            let _ = session::save(&session);
            emit(AgentEvent::Finish {
                session_id: session_id.clone(),
                reason: state.finish_reason.unwrap_or_else(|| "stop".into()),
            });
            return Ok(AgentTurnResult {
                session_id,
                assistant_text: assistant_text_final,
                tool_calls_made,
            });
        }

        // Append assistant message with the tool calls so providers that
        // require strict tool/assistant pairing (Azure) can resolve refs.
        let assistant_with_calls = AiSessionMessage {
            id: Uuid::new_v4().to_string(),
            role: ChatRole::Assistant,
            content: state.assistant_text.clone(),
            tool_calls: tool_calls.clone(),
            tool_call_id: None,
            tool_name: None,
            created_at: session::now_secs(),
        };
        session.messages.push(assistant_with_calls);

        // Dispatch each tool call.
        for call in &tool_calls {
            tool_calls_made += 1;
            emit(AgentEvent::ToolCallStart {
                session_id: session_id.clone(),
                call_id: call.id.clone(),
                name: call.name.clone(),
                arguments_json: call.arguments_json.clone(),
            });
            let dispatched = dispatch_tool(&call.name, &call.arguments_json, &skills);
            match dispatched {
                Ok(result) => {
                    emit(AgentEvent::ToolCallResult {
                        session_id: session_id.clone(),
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        label: result.label.clone(),
                        data: result.data.clone(),
                    });
                    let tool_content = serde_json::to_string(&result.data).unwrap_or_default();
                    session.messages.push(AiSessionMessage {
                        id: Uuid::new_v4().to_string(),
                        role: ChatRole::Tool,
                        content: tool_content,
                        tool_calls: Vec::new(),
                        tool_call_id: Some(call.id.clone()),
                        tool_name: Some(call.name.clone()),
                        created_at: session::now_secs(),
                    });
                }
                Err(err_msg) => {
                    emit(AgentEvent::ToolCallError {
                        session_id: session_id.clone(),
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        message: err_msg.clone(),
                    });
                    session.messages.push(AiSessionMessage {
                        id: Uuid::new_v4().to_string(),
                        role: ChatRole::Tool,
                        content: serde_json::json!({ "error": err_msg }).to_string(),
                        tool_calls: Vec::new(),
                        tool_call_id: Some(call.id.clone()),
                        tool_name: Some(call.name.clone()),
                        created_at: session::now_secs(),
                    });
                }
            }
        }

        let _ = session::save(&session);

        // Safety cap.
        if iteration + 1 >= MAX_TOOL_ITERATIONS {
            let msg = format!(
                "max tool iterations ({}) reached without final answer",
                MAX_TOOL_ITERATIONS
            );
            emit(AgentEvent::Error {
                session_id: session_id.clone(),
                message: msg.clone(),
            });
            return Err(msg);
        }
    }

    Ok(AgentTurnResult {
        session_id,
        assistant_text: assistant_text_final,
        tool_calls_made,
    })
}

#[derive(Default, Clone)]
struct StreamCollector {
    assistant_text: String,
    tool_calls: Vec<AssembledToolCall>,
    finish_reason: Option<String>,
    error: Option<String>,
}

#[derive(Default, Clone)]
struct AssembledToolCall {
    id: String,
    name: String,
    arguments_json: String,
}

impl StreamCollector {
    fn append_tool_delta(&mut self, id: String, name: Option<String>, args_delta: String) {
        if let Some(existing) = self.tool_calls.iter_mut().find(|c| c.id == id) {
            if let Some(n) = name {
                if existing.name.is_empty() {
                    existing.name = n;
                }
            }
            existing.arguments_json.push_str(&args_delta);
            return;
        }
        self.tool_calls.push(AssembledToolCall {
            id,
            name: name.unwrap_or_default(),
            arguments_json: args_delta,
        });
    }
}

fn derive_title(user_text: &str) -> String {
    let trimmed = user_text.trim();
    if trimmed.is_empty() {
        return "Untitled chat".into();
    }
    let truncated: String = trimmed.chars().take(60).collect();
    if trimmed.chars().count() > 60 {
        format!("{}…", truncated)
    } else {
        truncated
    }
}

fn build_system_prompt(skills: &[Skill]) -> String {
    use std::fmt::Write;
    let mut summary = String::new();
    let _ = writeln!(
        &mut summary,
        "User currently has {} skill(s) installed across {} agent(s).",
        skills.len(),
        skills
            .iter()
            .map(|s| serde_json::to_string(&s.agent_id).unwrap_or_default())
            .collect::<std::collections::HashSet<_>>()
            .len()
    );
    // Compact top-N preview so the model can drop tool-calls when the
    // answer is obvious. Cap at 25 to keep token use low.
    for s in skills.iter().take(25) {
        let agent = serde_json::to_string(&s.agent_id)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let _ = writeln!(
            &mut summary,
            "- [{}] {} — {}",
            agent,
            s.name,
            s.description.chars().take(140).collect::<String>()
        );
    }
    format!(
        "{}\n\nSkill registry snapshot (preview — call list_skills/search_skills for full data):\n{}",
        DEFAULT_SYSTEM_PROMPT_BASE, summary
    )
}
