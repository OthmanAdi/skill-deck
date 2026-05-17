// @agent-context: Tool-calling agent loop with telemetry + logging.
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
//   5. Persist session + per-turn telemetry.

use super::provider::{
    ChatChunk, ChatMessage, ChatRequest, ChatRole, ChunkSink, LlmProvider, ToolCall,
};
use super::session::{self, AiSession, AiSessionMessage, AgentTurnTelemetry, ToolDispatchRecord};
use super::tools::{all_tools, dispatch_tool};
use crate::models::Skill;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const MAX_TOOL_ITERATIONS: usize = 8;
const DEFAULT_TEMPERATURE: f32 = 0.2;

const DEFAULT_SYSTEM_PROMPT_BASE: &str = r#"You are Skill Deck's resident AI engineer. Your job is to help the user discover, understand, and combine the AI "skills" installed on their local machine (markdown files that drive coding agents like Claude Code, Codex, Cursor, Gemini CLI, etc.), and to translate plain intent into ready-to-paste prompts for the user's coding agent of choice.

## Hard rules (non-negotiable)

1. You have NO independent knowledge of the user's installed skills. EVERY claim about what they have, do not have, by name / tag / date / scope / language / author MUST come from a tool call. If you have not called a tool yet for this turn, you are not allowed to answer factually about their skills.

2. NEVER reply with "you haven't installed any skills" or "I don't have access" or "I don't know what you have." Call tools instead. If a tool returns zero results, broaden the query (different keywords, more fields, wider time window) at least once before concluding the answer is zero.

3. Be thorough. Chain multiple tool calls when the question is investigative. Typical good chains:
   - "what skills do I have for X" → search_skills(X) → list_skills(tag=X) if low results → answer.
   - "what did I install today" → list_skills(installed_since_unix=<computed>) → get_skill_stats(group_by=agent_id) for context → answer.
   - "make me a prompt for X using Y and Z" → search_skills(Y) + search_skills(Z) → get_skill_detail for any you need → combine_skills_workflow → render_prompt_for_coding_agent → quote final prompt.
   - "summarize what I have" → get_skill_stats(group_by=agent_id) + get_skill_stats(group_by=tag, top_n=12) → answer.

4. Date math. `current_time_unix` is provided to you below. Compute time windows from it. Examples (assume now=NOW): today = NOW - 86400, yesterday window = (NOW - 172800, NOW - 86400), 7 days = NOW - 604800, 30 days = NOW - 2592000.

5. Never invent skill ids, names, slash commands, or repository URLs. Only use values that appear in a tool result you have already received this turn.

## Mandatory output template (final assistant reply)

Every FINAL assistant reply (the one without tool_calls) must follow this template exactly, in markdown:

### Summary
One sentence: the headline answer.

### Detail
Comprehensive answer with bullet points, a short table, or numbered list. Reference specific skills by name AND id from your tool results. If you produced a prompt, embed it here in a fenced code block tagged `prompt`.

### Suggested next prompts
2 to 4 concrete prompts the user can paste back into this chat to go deeper. Each prompt in its own fenced code block tagged `prompt`. Each should be specific and actionable, using the user's real skill names where relevant. Examples of good suggested prompts:
- "Show me the body of the rust-testing skill"
- "Combine humanizer and pr-perfect into a release-notes workflow"
- "Build me a Claude Code prompt that refactors api.rs using rust-testing and code-review-quality"

If the user's question was conversational small-talk that doesn't touch their skills, you may skip the Suggested next prompts section, but Summary + Detail are still required.

## Tool-call discipline

- Prefer search_skills for keyword / natural-language questions.
- Use list_skills with filters for structured asks ("which Cursor skills", "starred only", "installed today").
- Use get_skill_stats for counts and analytics — it's cheap and returns no bodies.
- Use get_skill_detail only after you've narrowed to specific skills.
- Combine_skills_workflow + render_prompt_for_coding_agent are for prompt-building requests.
- If a tool returns zero or thin results, your next message should be ANOTHER tool call with a broader query — NOT a final answer.

Stay concise in the Detail section. Tool results are visible to the user as expandable cards next to your reply, so don't repeat their full JSON.
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurnRequest {
    pub session_id: Option<String>,
    pub user_text: String,
    pub provider_id: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurnResult {
    pub session_id: String,
    pub assistant_text: String,
    pub tool_calls_made: usize,
    pub iterations: usize,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AgentEvent {
    SessionStarted { session_id: String },
    ContentDelta { session_id: String, text: String },
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
        label: String,
        data: serde_json::Value,
    },
    ToolCallError {
        session_id: String,
        call_id: String,
        name: String,
        message: String,
    },
    Finish { session_id: String, reason: String },
    Error { session_id: String, message: String },
}

pub type EventEmitter = Arc<dyn Fn(AgentEvent) + Send + Sync>;

pub async fn run_agent_turn(
    provider: Arc<dyn LlmProvider>,
    skills: Vec<Skill>,
    req: AgentTurnRequest,
    emit: EventEmitter,
) -> Result<AgentTurnResult, String> {
    let turn_start = std::time::Instant::now();
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
        tool_label: None,
        tool_error: None,
        created_at: session::now_secs(),
    };
    session.messages.push(user_msg);

    emit(AgentEvent::SessionStarted {
        session_id: session_id.clone(),
    });

    log::info!(
        target: "skill_deck::agent",
        "turn start session_id={} provider={} model={} skills_in_context={} user_text=\"{}\"",
        session_id,
        provider.id(),
        req.model,
        skills.len(),
        truncate(&req.user_text, 160)
    );

    let system_prompt = build_system_prompt(&skills);
    let tools = all_tools();
    let mut tool_calls_made = 0usize;
    let mut assistant_text_final = String::new();
    let mut telemetry = AgentTurnTelemetry::new(req.user_text.clone());

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
            temperature: Some(DEFAULT_TEMPERATURE),
            max_tokens: None,
        };

        log::info!(
            target: "skill_deck::agent",
            "iter {} starting messages={} tools={}",
            iteration,
            request.messages.len(),
            request.tools.len()
        );

        let iter_start = std::time::Instant::now();
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
                    state.chunk_count += 1;
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
                    state.chunk_count += 1;
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
            log::error!(
                target: "skill_deck::agent",
                "iter {} stream failed: {}",
                iteration,
                msg
            );
            emit(AgentEvent::Error {
                session_id: session_id.clone(),
                message: msg.clone(),
            });
            telemetry.error = Some(msg.clone());
            session.last_telemetry = Some(telemetry);
            let _ = session::save(&session);
            return Err(msg);
        }

        let iter_ms = iter_start.elapsed().as_millis() as u64;
        let state = collector.lock().map(|g| g.clone()).unwrap_or_default();
        log::info!(
            target: "skill_deck::agent",
            "iter {} done elapsed_ms={} chunks={} content_len={} tool_calls={}",
            iteration,
            iter_ms,
            state.chunk_count,
            state.assistant_text.len(),
            state.tool_calls.len()
        );
        if state.chunk_count == 0 {
            log::warn!(
                target: "skill_deck::agent",
                "iter {} produced ZERO chunks — model may have failed silently",
                iteration
            );
        }

        if let Some(err_msg) = state.error.clone() {
            emit(AgentEvent::Error {
                session_id: session_id.clone(),
                message: err_msg.clone(),
            });
            telemetry.error = Some(err_msg.clone());
            session.last_telemetry = Some(telemetry);
            let _ = session::save(&session);
            return Err(err_msg);
        }

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

        telemetry.iterations += 1;
        telemetry.total_chunks += state.chunk_count;
        telemetry.total_assistant_chars += state.assistant_text.chars().count() as u64;
        telemetry.duration_ms += iter_ms;

        if tool_calls.is_empty() {
            assistant_text_final = state.assistant_text.clone();
            let assistant = AiSessionMessage {
                id: Uuid::new_v4().to_string(),
                role: ChatRole::Assistant,
                content: state.assistant_text.clone(),
                tool_calls: Vec::new(),
                tool_call_id: None,
                tool_name: None,
                tool_label: None,
                tool_error: None,
                created_at: session::now_secs(),
            };
            session.messages.push(assistant);
            session.updated_at = session::now_secs();
            telemetry.finish_reason = state.finish_reason.clone();
            telemetry.duration_ms = turn_start.elapsed().as_millis() as u64;
            session.last_telemetry = Some(telemetry.clone());
            let _ = session::save(&session);
            log::info!(
                target: "skill_deck::agent",
                "turn finished session_id={} iterations={} tool_calls_made={} duration_ms={}",
                session_id,
                telemetry.iterations,
                tool_calls_made,
                telemetry.duration_ms
            );
            emit(AgentEvent::Finish {
                session_id: session_id.clone(),
                reason: state.finish_reason.unwrap_or_else(|| "stop".into()),
            });
            return Ok(AgentTurnResult {
                session_id,
                assistant_text: assistant_text_final,
                tool_calls_made,
                iterations: telemetry.iterations,
                duration_ms: telemetry.duration_ms,
            });
        }

        let assistant_with_calls = AiSessionMessage {
            id: Uuid::new_v4().to_string(),
            role: ChatRole::Assistant,
            content: state.assistant_text.clone(),
            tool_calls: tool_calls.clone(),
            tool_call_id: None,
            tool_name: None,
            tool_label: None,
            tool_error: None,
            created_at: session::now_secs(),
        };
        session.messages.push(assistant_with_calls);

        for call in &tool_calls {
            tool_calls_made += 1;
            emit(AgentEvent::ToolCallStart {
                session_id: session_id.clone(),
                call_id: call.id.clone(),
                name: call.name.clone(),
                arguments_json: call.arguments_json.clone(),
            });
            let dispatch_start = std::time::Instant::now();
            let dispatched = dispatch_tool(&call.name, &call.arguments_json, &skills);
            let dispatch_ms = dispatch_start.elapsed().as_millis() as u64;
            match dispatched {
                Ok(result) => {
                    emit(AgentEvent::ToolCallResult {
                        session_id: session_id.clone(),
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        label: result.label.clone(),
                        data: result.data.clone(),
                    });
                    telemetry.tool_dispatches.push(ToolDispatchRecord {
                        name: call.name.clone(),
                        label: Some(result.label.clone()),
                        success: true,
                        error: None,
                        duration_ms: dispatch_ms,
                    });
                    let tool_content = serde_json::to_string(&result.data).unwrap_or_default();
                    session.messages.push(AiSessionMessage {
                        id: Uuid::new_v4().to_string(),
                        role: ChatRole::Tool,
                        content: tool_content,
                        tool_calls: Vec::new(),
                        tool_call_id: Some(call.id.clone()),
                        tool_name: Some(call.name.clone()),
                        tool_label: Some(result.label.clone()),
                        tool_error: None,
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
                    telemetry.tool_dispatches.push(ToolDispatchRecord {
                        name: call.name.clone(),
                        label: None,
                        success: false,
                        error: Some(err_msg.clone()),
                        duration_ms: dispatch_ms,
                    });
                    session.messages.push(AiSessionMessage {
                        id: Uuid::new_v4().to_string(),
                        role: ChatRole::Tool,
                        content: serde_json::json!({ "error": err_msg }).to_string(),
                        tool_calls: Vec::new(),
                        tool_call_id: Some(call.id.clone()),
                        tool_name: Some(call.name.clone()),
                        tool_label: None,
                        tool_error: Some(err_msg.clone()),
                        created_at: session::now_secs(),
                    });
                }
            }
        }

        let _ = session::save(&session);

        if iteration + 1 >= MAX_TOOL_ITERATIONS {
            let msg = format!(
                "max tool iterations ({}) reached without final answer",
                MAX_TOOL_ITERATIONS
            );
            log::warn!(target: "skill_deck::agent", "{}", msg);
            telemetry.error = Some(msg.clone());
            telemetry.duration_ms = turn_start.elapsed().as_millis() as u64;
            session.last_telemetry = Some(telemetry);
            let _ = session::save(&session);
            emit(AgentEvent::Error {
                session_id: session_id.clone(),
                message: msg.clone(),
            });
            return Err(msg);
        }
    }

    telemetry.duration_ms = turn_start.elapsed().as_millis() as u64;
    session.last_telemetry = Some(telemetry.clone());
    let _ = session::save(&session);
    Ok(AgentTurnResult {
        session_id,
        assistant_text: assistant_text_final,
        tool_calls_made,
        iterations: telemetry.iterations,
        duration_ms: telemetry.duration_ms,
    })
}

#[derive(Default, Clone)]
struct StreamCollector {
    assistant_text: String,
    tool_calls: Vec<AssembledToolCall>,
    finish_reason: Option<String>,
    error: Option<String>,
    chunk_count: u64,
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

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push('…');
        out
    }
}

fn build_system_prompt(skills: &[Skill]) -> String {
    use std::fmt::Write;
    let now = session::now_secs();
    let mut header = String::new();
    let _ = writeln!(&mut header, "current_time_unix: {}", now);
    let _ = writeln!(&mut header, "current_time_iso: {}", chrono::Utc.timestamp_opt(now as i64, 0).single().map(|d| d.to_rfc3339()).unwrap_or_default());
    let _ = writeln!(
        &mut header,
        "skill_registry_size: {} skill(s) across {} agent(s)",
        skills.len(),
        skills
            .iter()
            .map(|s| serde_json::to_string(&s.agent_id).unwrap_or_default())
            .collect::<std::collections::HashSet<_>>()
            .len()
    );

    let mut preview = String::new();
    for s in skills.iter().take(40) {
        let agent = serde_json::to_string(&s.agent_id)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        let _ = writeln!(
            &mut preview,
            "- [{}] {} (id={}) — {}",
            agent,
            s.name,
            s.id,
            s.description.chars().take(120).collect::<String>()
        );
    }

    format!(
        "{base}\n\n## Runtime context\n{header}\n## Skill registry preview (first 40 — call tools for full data)\n{preview}\n",
        base = DEFAULT_SYSTEM_PROMPT_BASE,
        header = header,
        preview = preview
    )
}

use chrono::TimeZone;
