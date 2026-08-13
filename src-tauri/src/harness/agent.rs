// @agent-context: The ReAct loop, hardened.
//
// Flow per turn:
//   1. parse explicit item-count from prompt; inject as USER_REQUESTED_ITEM_COUNT
//   2. memory.push_user
//   3. compact_if_needed (inline, with real summarizer)
//   4. orchestrator::build → provider-specific rig Agents + shared tool definitions
//   5. the app-owned controlled loop budgets every dispatch and projects bounded tool evidence
//   6. a tool-free finalizer or grounded deterministic fallback produces the final answer
//   7. drain tool_log into memory (so persisted history HAS tool cards)
//   8. compute deterministic FinalEnvelope from tool_log + final_text
//   9. memory.push_assistant; preserve session title + createdAt; save session

mod controlled;

use crate::harness::cancel::{CancelHandle, CancelRegistry, TurnGate};
use crate::harness::config::HarnessSettings;
use crate::harness::context::{ContextSpec, TurnBudget};
use crate::harness::error::{classify_message, HarnessErrorKind, BUSY_PREFIX, CANCELLED_MESSAGE};
use crate::harness::events::{EnvelopeSource, HarnessEvent};
use crate::harness::hooks::{EventSink, TelemetryHook, ToolLog, ToolLogEntry};
use crate::harness::intent;
use crate::harness::memory::{compaction, conversation::ConversationMemory};
use crate::harness::model_directory::ModelProfile;
use crate::harness::orchestrator::{self, OrchestratorAgent};
use crate::harness::providers::{self, ProviderClient};
use crate::harness::session::{self, HarnessSession};
use crate::harness::source_identity::{stable_dedup_by_identity, SourceIdentity};
use crate::harness::telemetry::TelemetryWriter;
use crate::harness::tools::HarnessContext;
use anyhow::{Context, Result};
use rig::agent::Agent;
use rig::agent::{MultiTurnStreamItem, StreamingError};
use rig::client::CompletionClient;
use rig::completion::{CompletionModel, GetTokenUsage};
use rig::message::Message;
use rig::streaming::StreamedAssistantContent;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSummary {
    pub session_id: String,
    pub final_text: String,
    pub in_tokens: u32,
    pub out_tokens: u32,
    pub duration_ms: u64,
    pub item_count_hint: Option<usize>,
    pub tool_calls_made: usize,
}

pub struct TurnRequest {
    pub session_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub model_profile: Option<ModelProfile>,
    pub user_text: String,
}

pub async fn run_turn(
    client: Arc<ProviderClient>,
    ctx: HarnessContext,
    settings: HarnessSettings,
    telemetry: Arc<TelemetryWriter>,
    sink: Option<EventSink>,
    memory: &mut ConversationMemory,
    req: TurnRequest,
) -> Result<TurnSummary> {
    let started_at = std::time::Instant::now();

    // Concurrency gate FIRST — before any memory mutation or event. A second
    // send while a turn is active for the same session fails fast (kind:
    // `busy` on the IPC boundary) instead of racing the running turn's memory
    // and session file. The guard releases on every exit path (RAII), so a
    // panicking or cancelled turn can never wedge the session.
    let _turn_guard = TurnGate::instance()
        .try_acquire(&req.session_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{BUSY_PREFIX} a turn is already running for session {}; cancel it or wait for it to finish",
                req.session_id
            )
        })?;
    let cancel = CancelRegistry::instance().reset(&req.session_id);

    // -- enrich model profile with Ollama-side context_window when we have no other source --
    let mut model_profile = req.model_profile.clone();
    let mut runtime_context_tokens = None;
    if let ProviderClient::Ollama { config, .. } = client.as_ref() {
        let needs_ctx = model_profile
            .as_ref()
            .map(|p| p.context_window.is_none())
            .unwrap_or(true);
        if needs_ctx {
            let endpoint = config
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
            if let Some(num_ctx) = providers::ollama_context_window(&endpoint, &req.model_id).await
            {
                runtime_context_tokens = Some(num_ctx);
                let mut p = model_profile.unwrap_or_else(|| ollama_synthetic_profile(&req));
                p.context_window = Some(num_ctx);
                model_profile = Some(p);
            }
        }
    } else if matches!(client.as_ref(), ProviderClient::Builtin { .. }) && model_profile.is_none() {
        // model_directory has no builtin entry, so req.model_profile is None
        // for builtin turns. Without this, compaction falls back to a
        // conservative ceiling instead of the real 8K window we launch with.
        model_profile = Some(builtin_synthetic_profile(&req));
        runtime_context_tokens = Some(crate::harness::builtin::BUILTIN_CTX);
    }

    // -- 1. item count parser
    let item_hint = intent::extract_item_count(&req.user_text);
    let augmented = match item_hint {
        Some(n) => format!(
            "{}\n\n[USER_REQUESTED_ITEM_COUNT: {n}] — call tools with limit={n} and return exactly {n} items.",
            req.user_text.trim()
        ),
        None => req.user_text.clone(),
    };

    emit(
        &telemetry,
        &sink,
        HarnessEvent::SessionStarted {
            session_id: req.session_id.clone(),
        },
    );

    // -- 2. record user turn
    memory.push_user(req.user_text.clone());

    // -- 3. compact if needed. Runs even without a model profile (the
    //    compactor falls back to a conservative ceiling), and compaction
    //    failure is NON-FATAL: a flaky summarizer must degrade to "turn runs
    //    with a fuller context", never "turn aborts". anchored_summarize only
    //    mutates memory after the summarizer succeeds, so nothing is lost.
    {
        let summarize_client = client.clone();
        let summarize_model = req.model_id.clone();
        let local_compaction_only = settings.local_compaction_only;
        let summary_max_tokens = settings.summary_max_tokens;
        let summarize_fn = |prior: String, block: String| {
            let client = summarize_client.clone();
            let model = summarize_model.clone();
            async move {
                if local_compaction_only {
                    Ok(compaction::local_summary(
                        &prior,
                        &block,
                        summary_max_tokens,
                    ))
                } else {
                    // The only remote compactor available is the provider the
                    // user selected for this turn. No fallback provider exists.
                    summarize(&client, &model, prior, block).await
                }
            }
        };
        // Race compaction against cancel: the summarize step is a provider
        // call and can be slow. Aborting it is safe — anchored_summarize
        // only mutates memory AFTER the summarizer succeeds. On cancel we
        // fall through; the stream drain below exits on its first (biased)
        // poll and the standard cancelled tail persists + reports.
        let compaction_outcome = tokio::select! {
            biased;
            _ = cancel.cancelled() => None,
            r = compaction::compact_if_needed(memory, model_profile.as_ref(), &settings, summarize_fn) => Some(r),
        };
        match compaction_outcome {
            Some(Ok(events)) => {
                for ev in events {
                    emit(
                        &telemetry,
                        &sink,
                        HarnessEvent::Compaction {
                            session_id: req.session_id.clone(),
                            strategy: ev.strategy,
                            freed_tokens: ev.freed_tokens,
                            before: ev.before_tokens,
                            after: ev.after_tokens,
                        },
                    );
                }
            }
            Some(Err(e)) => {
                log::warn!(
                    "harness: compaction skipped for session {}: {e:#}",
                    req.session_id
                );
            }
            None => {
                log::info!(
                    "harness: cancel received during compaction for session {}",
                    req.session_id
                );
            }
        }
    }

    // -- 4. orchestrator (uses ctx.skills snapshot to render preamble counts)
    let controlled_ctx = ctx.clone();
    let orchestrator = orchestrator::build(&client, &req.model_id, ctx, settings.max_tokens.into());

    // -- 5. hook with model name + tool log capture
    let mut hook = TelemetryHook::new(
        req.session_id.clone(),
        req.model_id.clone(),
        telemetry.clone(),
    );
    if let Some(s) = sink.clone() {
        hook = hook.with_sink(s);
    }
    let tool_log: ToolLog = hook.tool_log();

    let history = memory.to_rig_history();
    let context = ContextSpec::resolve(
        req.model_id.clone(),
        runtime_context_tokens,
        model_profile.as_ref(),
    );
    let turn_budget = TurnBudget::from_settings(context, &settings, Some(settings.max_tokens));

    // The provider stream (and every tool future inside it) lives inside the
    // match arm: when `consume_stream` returns — normally, on error, or on
    // cancel — the arm ends and the stream drops, which aborts the in-flight
    // HTTP request and any tool execution. rig 0.35 drives tools inline in
    // the stream poll (no spawned tasks), so drop == full abort, no zombies.
    let (mut final_text, stream_error, controlled_in, controlled_out) = match orchestrator {
        OrchestratorAgent::Ollama(agent) => match turn_budget.clone() {
            Ok(budget) => match orchestrator::build_ollama_finalizer(
                &client,
                &req.model_id,
                budget.reserved_completion_tokens.into(),
            ) {
                Some(finalizer) => {
                    run_controlled_agent(
                        &agent,
                        &finalizer,
                        &controlled_ctx,
                        &hook,
                        &cancel,
                        history,
                        &req.user_text,
                        &augmented,
                        budget,
                    )
                    .await
                }
                None => missing_finalizer("Ollama"),
            },
            Err(error) => invalid_budget(error),
        },
        OrchestratorAgent::Azure(agent) => match turn_budget.clone() {
            Ok(budget) => match orchestrator::build_azure_finalizer(
                &client,
                &req.model_id,
                budget.reserved_completion_tokens.into(),
            ) {
                Some(finalizer) => {
                    run_controlled_agent(
                        &agent,
                        &finalizer,
                        &controlled_ctx,
                        &hook,
                        &cancel,
                        history,
                        &req.user_text,
                        &augmented,
                        budget,
                    )
                    .await
                }
                None => missing_finalizer("Azure"),
            },
            Err(error) => invalid_budget(error),
        },
        OrchestratorAgent::Builtin(agent) => match turn_budget {
            Ok(budget) => {
                let finalizer = orchestrator::build_builtin_finalizer(
                    &client,
                    &req.model_id,
                    budget.reserved_completion_tokens.into(),
                );
                match finalizer {
                    Some(finalizer) => {
                        run_controlled_agent(
                            &agent,
                            &finalizer,
                            &controlled_ctx,
                            &hook,
                            &cancel,
                            history,
                            &req.user_text,
                            &augmented,
                            budget,
                        )
                        .await
                    }
                    None => missing_finalizer("Builtin"),
                }
            }
            Err(error) => invalid_budget(error),
        },
    };
    let in_tokens = controlled_in;
    let out_tokens = controlled_out;
    // Final delta flush: on error/cancel exits rig's finish hook never fires,
    // and the batcher may still hold the tail of the streamed text.
    hook.flush_content();
    let finish_reason = if stream_error.is_some() {
        "error"
    } else {
        "stop"
    };

    // -- 7. drain tool log into memory BEFORE pushing assistant so the persisted
    //       transcript is user → tools (in order) → assistant.
    let tool_entries = tool_log.drain();
    let tool_calls_made = tool_entries.iter().filter(|entry| !entry.vetoed).count();
    for entry in &tool_entries {
        match (entry.result.clone(), entry.error.clone()) {
            (Some(result), _) => memory.push_tool_success(
                entry.call_id.clone(),
                entry.name.clone(),
                entry.args_json.clone(),
                result,
            ),
            (None, Some(err)) => memory.push_tool_error(
                entry.call_id.clone(),
                entry.name.clone(),
                entry.args_json.clone(),
                err,
            ),
            (None, None) => {
                // running-but-never-finished — keep a stub so the user sees the
                // call attempt rather than nothing.
                memory.push_tool_error(
                    entry.call_id.clone(),
                    entry.name.clone(),
                    entry.args_json.clone(),
                    "tool returned no payload".to_string(),
                );
            }
        }
    }

    // -- RHC-02: the stream may have errored (commonly MaxTurnsError when a
    //    small model loops). The tool work above is already drained into memory;
    //    if the model produced no text of its own, synthesize a graceful message
    //    so the turn shows what was gathered instead of vanishing.
    if stream_error.is_some() && final_text.trim().is_empty() {
        final_text = if tool_calls_made > 0 {
            format!(
                "I reached the step limit before writing a final answer, but the {tool_calls_made} tool call(s) above ran and their results are shown."
            )
        } else {
            "The turn stopped before completing. Please try again.".to_string()
        };
    }

    // -- 8. PostModel + deterministic FinalEnvelope + Finish
    let cost_usd = cost_estimate(model_profile.as_ref(), in_tokens, out_tokens);
    emit(
        &telemetry,
        &sink,
        HarnessEvent::PostModel {
            session_id: req.session_id.clone(),
            model: req.model_id.clone(),
            finish_reason: finish_reason.into(),
            in_tokens,
            out_tokens,
            cost_usd,
        },
    );

    let envelope = build_final_envelope(&req.session_id, &final_text, &tool_entries);
    emit(&telemetry, &sink, envelope);

    let duration_ms = started_at.elapsed().as_millis() as u64;
    emit(
        &telemetry,
        &sink,
        HarnessEvent::Finish {
            session_id: req.session_id.clone(),
            reason: finish_reason.into(),
            duration_ms,
        },
    );

    // -- 9. record assistant + persist session, preserving createdAt + title
    memory.push_assistant(final_text.clone());
    let now = session::now_secs();
    let (created_at, title) = match session::load(&req.session_id) {
        Ok(prior) => (prior.created_at, prior.title),
        Err(_) => (now, req.user_text.chars().take(60).collect::<String>()),
    };
    let session_obj = HarnessSession {
        id: req.session_id.clone(),
        title,
        provider_id: req.provider_id.clone(),
        model: req.model_id.clone(),
        created_at,
        updated_at: now,
        memory: memory.clone(),
        final_summary: memory.running_summary.clone(),
    };
    if let Err(e) = session::save(&session_obj) {
        // Losing persistence silently would mean the turn vanishes on reload —
        // tell the user (and the log) instead of pretending it saved.
        log::warn!(
            "harness: failed to persist session {}: {e:#}",
            req.session_id
        );
        emit(
            &telemetry,
            &sink,
            HarnessEvent::Error {
                session_id: req.session_id.clone(),
                message: format!("failed to persist session: {e:#}"),
                error_kind: HarnessErrorKind::Internal,
            },
        );
    }

    // Cancelled turns persist their partial work above (tool cards + partial
    // text survive a cancel — documented behavior), then surface as an error.
    if cancel.is_cancelled() {
        emit(
            &telemetry,
            &sink,
            HarnessEvent::Error {
                session_id: req.session_id.clone(),
                message: "cancelled by user".into(),
                error_kind: HarnessErrorKind::Cancelled,
            },
        );
        anyhow::bail!("{CANCELLED_MESSAGE}");
    }

    // RHC-02: the turn is fully persisted above; surface the stream error to the
    // UI without discarding the work. Returning Ok lets the frontend load the
    // persisted partial session (tool cards finalized, graceful text) while the
    // Error event populates lastError.
    if let Some(err) = stream_error {
        emit(
            &telemetry,
            &sink,
            HarnessEvent::Error {
                session_id: req.session_id.clone(),
                message: format!("turn ended early: {err}"),
                error_kind: classify_message(&err),
            },
        );
    }

    Ok(TurnSummary {
        session_id: req.session_id,
        final_text,
        in_tokens,
        out_tokens,
        duration_ms,
        item_count_hint: item_hint,
        tool_calls_made,
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_controlled_agent<M>(
    agent: &Agent<M>,
    finalizer: &Agent<M>,
    ctx: &HarnessContext,
    hook: &TelemetryHook,
    cancel: &CancelHandle,
    history: Vec<Message>,
    current_user: &str,
    augmented: &str,
    budget: TurnBudget,
) -> (String, Option<String>, u32, u32)
where
    M: CompletionModel + 'static,
    M::StreamingResponse: GetTokenUsage,
{
    // `memory.push_user` happened before this call for persistence. The
    // controlled call supplies the augmented user message as `initial_prompt`,
    // so remove the just-pushed plain copy instead of sending it twice.
    let controlled_history = controlled::without_current_user(history, current_user);
    let outcome = controlled::run(controlled::ControlledRun {
        tool_agent: agent,
        finalizer_agent: finalizer,
        tool_preamble: agent.preamble.clone().unwrap_or_default(),
        finalizer_preamble: finalizer.preamble.clone().unwrap_or_default(),
        tool_definitions: orchestrator::tool_definitions(),
        ctx,
        hook,
        cancel,
        history: controlled_history,
        initial_prompt: Message::user(augmented.to_string()),
        budget,
    })
    .await;
    (
        outcome.final_text,
        outcome.stream_error,
        outcome.in_tokens,
        outcome.out_tokens,
    )
}

fn missing_finalizer(provider: &str) -> (String, Option<String>, u32, u32) {
    let message = format!("{provider} finalizer could not be constructed for this provider");
    (
        format!("The {provider} turn could not start its tool-free finalizer."),
        Some(message),
        0,
        0,
    )
}

fn invalid_budget(
    error: crate::harness::context::BudgetError,
) -> (String, Option<String>, u32, u32) {
    let message = format!("context budget configuration failed: {error}");
    (
        format!(
            "I could not run this request because the model context budget is invalid. {message}"
        ),
        Some(message),
        0,
        0,
    )
}

/// Drain the model stream. Returns the collected text plus an optional error
/// message. It NEVER propagates the error via `?`: a hard stream error (most
/// commonly rig's MaxTurnsError when a small model exhausts the turn budget)
/// must NOT discard the tool work + partial text already gathered, nor skip
/// session persistence (RHC-02). The caller persists whatever we return and
/// surfaces the error separately.
///
/// Cancellation: each iteration RACES the next stream item against the
/// session's `CancelHandle` (biased toward cancel). This is the fix for the
/// hung-provider case — the old flag-only check ran between items, so a
/// provider that stopped yielding could never be cancelled. Winning the race
/// drops the in-flight `next()` future; the caller then drops the stream
/// itself, aborting the provider request and any tool mid-execution.
pub(crate) async fn consume_stream<S, R>(
    stream: &mut S,
    cancel: &CancelHandle,
    in_tokens: &mut u32,
    out_tokens: &mut u32,
) -> (String, Option<String>)
where
    S: futures_util::Stream<Item = std::result::Result<MultiTurnStreamItem<R>, StreamingError>>
        + Unpin,
    R: Clone + Unpin + Send + Sync + 'static,
{
    use futures_util::StreamExt;
    let mut final_text = String::new();
    let mut aggregate_text = String::new();
    let mut stream_error: Option<String> = None;
    loop {
        let item = tokio::select! {
            biased;
            _ = cancel.cancelled() => break,
            item = stream.next() => item,
        };
        let Some(item) = item else { break };
        match item {
            Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(t))) => {
                aggregate_text.push_str(&t.text);
            }
            Ok(MultiTurnStreamItem::FinalResponse(fin)) => {
                let usage = fin.usage();
                *in_tokens = usage.input_tokens as u32;
                *out_tokens = usage.output_tokens as u32;
                final_text = if !fin.response().is_empty() {
                    fin.response().to_string()
                } else {
                    aggregate_text.clone()
                };
            }
            Err(e) => {
                stream_error = Some(format!("{e}"));
                break;
            }
            Ok(_) => { /* tool call / delta items — hook handles them */ }
        }
    }
    if final_text.is_empty() {
        final_text = aggregate_text;
    }
    (final_text, stream_error)
}

async fn summarize(
    client: &ProviderClient,
    model_id: &str,
    prior: String,
    block: String,
) -> Result<String> {
    use rig::completion::Prompt;
    let prompt_text = format!(
        "Summarize the following conversation block in 3-6 sentences. Preserve facts and \
         decisions. Merge with the prior summary; never repeat its contents.\n\n\
         Prior summary:\n{prior}\n\nBlock:\n{block}\n\nMerged summary:"
    );
    let response = match client {
        ProviderClient::Ollama { rig, .. } => {
            let agent = rig.agent(model_id).build();
            agent
                .prompt(prompt_text.as_str())
                .await
                .context("summarize")?
        }
        ProviderClient::Azure { rig, .. } => {
            let agent = rig.agent(model_id).build();
            agent
                .prompt(prompt_text.as_str())
                .await
                .context("summarize")?
        }
        ProviderClient::Builtin { rig, .. } => {
            let agent = rig.agent(model_id).build();
            agent
                .prompt(prompt_text.as_str())
                .await
                .context("summarize")?
        }
    };
    Ok(response)
}

fn emit(telemetry: &Arc<TelemetryWriter>, sink: &Option<EventSink>, event: HarnessEvent) {
    let _ = telemetry.append(&event);
    if let Some(s) = sink.as_ref() {
        s(event);
    }
}

/// Deterministic FinalEnvelope builder.
///
/// `suggested_next_prompts` are derived from which tools fired this turn, so
/// the model doesn't need to obey a markdown structural rule. `sources` come
/// from `web_search` + `search_marketplace` tool results. `skills_referenced`
/// are pulled from tool result payloads (canonical skill ids) plus any name
/// matches in the final text.
fn build_final_envelope(
    session_id: &str,
    final_text: &str,
    tool_entries: &[ToolLogEntry],
) -> HarnessEvent {
    let mut sources: Vec<EnvelopeSource> = Vec::new();
    let mut skills_referenced: Vec<String> = Vec::new();
    let mut next_prompts: Vec<String> = Vec::new();

    let mut had_search_skills = false;
    let mut had_translate = false;
    let mut had_marketplace = false;
    let mut had_web_search = false;
    let mut top_skill_id: Option<String> = None;
    let mut top_skill_name: Option<String> = None;
    let mut top_marketplace_install: Option<String> = None;
    let mut top_marketplace_name: Option<String> = None;

    for entry in tool_entries {
        let Some(result) = entry.result.as_ref() else {
            continue;
        };
        match entry.name.as_str() {
            "search_skills" | "list_skills" => {
                had_search_skills = true;
                if let Some(skills) = result.data.get("skills").and_then(|v| v.as_array()) {
                    for s in skills {
                        if let Some(id) = s.get("id").and_then(|v| v.as_str()) {
                            if !skills_referenced.iter().any(|x| x == id) {
                                skills_referenced.push(id.to_string());
                            }
                            if top_skill_id.is_none() {
                                top_skill_id = Some(id.to_string());
                                top_skill_name = s
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .map(|n| n.to_string());
                            }
                        }
                    }
                }
            }
            "translate_to_skills" => {
                had_translate = true;
                if let Some(cands) = result.data.get("candidates").and_then(|v| v.as_array()) {
                    for c in cands {
                        if let Some(id) = c.pointer("/skill/id").and_then(|v| v.as_str()) {
                            if !skills_referenced.iter().any(|x| x == id) {
                                skills_referenced.push(id.to_string());
                            }
                            if top_skill_id.is_none() {
                                top_skill_id = Some(id.to_string());
                                top_skill_name = c
                                    .pointer("/skill/name")
                                    .and_then(|v| v.as_str())
                                    .map(|n| n.to_string());
                            }
                        }
                    }
                }
            }
            "search_marketplace" => {
                had_marketplace = true;
                if let Some(items) = result.data.get("items").and_then(|v| v.as_array()) {
                    for item in items {
                        let title = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let url = item
                            .get("homepage_url")
                            .and_then(|v| v.as_str())
                            .or_else(|| item.get("source_url").and_then(|v| v.as_str()))
                            .unwrap_or("")
                            .to_string();
                        if !title.is_empty() && !url.is_empty() {
                            sources.push(EnvelopeSource {
                                title,
                                url,
                                kind: "marketplace".into(),
                            });
                        }
                        if top_marketplace_install.is_none() {
                            top_marketplace_install = item
                                .get("install_command")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            top_marketplace_name = item
                                .get("name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                        }
                    }
                }
            }
            "web_search" => {
                had_web_search = true;
                if let Some(results) = result.data.get("results").and_then(|v| v.as_array()) {
                    for r in results {
                        let title = r
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let url = r
                            .get("url")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !title.is_empty() && !url.is_empty() {
                            sources.push(EnvelopeSource {
                                title,
                                url,
                                kind: "web".into(),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Harvest skill ids from the assistant's prose. The model commonly puts
    // skills in two shapes:
    //   1. `[name](https://…/artifact:hash)` — markdown link with our id
    //   2. bare `artifact:hash` mentions outside a link
    // We do NOT match plain **bold** anymore — too many false positives
    // ("**Note:**", "**Description**", "**Score: 24**" all polluted the list).
    if let Ok(link_re) = regex::Regex::new(r"artifact:([a-f0-9]{6,32})") {
        for cap in link_re.captures_iter(final_text) {
            if let Some(id_match) = cap.get(1) {
                let id = format!("artifact:{}", id_match.as_str());
                if !skills_referenced.iter().any(|x| x == &id) {
                    skills_referenced.push(id);
                }
            }
        }
    }

    // Deterministic suggested next prompts.
    if let Some(name) = top_skill_name.as_ref() {
        next_prompts.push(format!("Show me the body of {name}"));
    }
    if had_translate && top_skill_name.is_some() {
        if let (Some(name), _) = (top_skill_name.as_ref(), top_skill_id.as_ref()) {
            next_prompts.push(format!(
                "Build a Claude Code prompt using {name} for my current task"
            ));
        }
    }
    if had_search_skills && !had_translate {
        next_prompts.push("Translate my intent into 3 skill candidates".to_string());
    }
    if had_marketplace {
        if let Some(install) = top_marketplace_install.as_ref() {
            next_prompts.push(format!(
                "Show me how to install: {}",
                install.chars().take(80).collect::<String>()
            ));
        }
        if let Some(name) = top_marketplace_name.as_ref() {
            next_prompts.push(format!("Find similar marketplace skills to {name}"));
        }
    }
    if had_web_search {
        next_prompts.push("Fetch the top result and summarize".to_string());
    }
    if next_prompts.is_empty() {
        next_prompts.push("What skills do I have for testing rust?".to_string());
        next_prompts.push("Translate my intent into 3 skill candidates".to_string());
    }
    next_prompts.truncate(4);
    sources = stable_dedup_by_identity(sources, |source| {
        SourceIdentity::new(None, Some(&source.url), Some(&source.kind))
    });

    HarnessEvent::FinalEnvelope {
        session_id: session_id.to_string(),
        suggested_next_prompts: next_prompts,
        sources,
        skills_referenced,
    }
}

fn cost_estimate(profile: Option<&ModelProfile>, in_tokens: u32, out_tokens: u32) -> Option<f64> {
    let profile = profile?;
    let pin = profile.pricing_input_per_million?;
    let pout = profile.pricing_output_per_million?;
    let usd = (pin * in_tokens as f64 + pout * out_tokens as f64) / 1_000_000.0;
    Some(usd)
}

fn ollama_synthetic_profile(req: &TurnRequest) -> ModelProfile {
    use crate::harness::tokens::TokenizerKind;
    ModelProfile {
        id: format!("ollama::{}", req.model_id),
        model_id: req.model_id.clone(),
        provider: "ollama".into(),
        name: req.model_id.clone(),
        family: None,
        context_window: None,
        output_limit: None,
        supports_tools: true,
        structured_output: false,
        reasoning: false,
        attachment: false,
        modalities_in: vec!["text".into()],
        modalities_out: vec!["text".into()],
        pricing_input_per_million: None,
        pricing_output_per_million: None,
        tokenizer_hint: TokenizerKind::OllamaNative,
        source: vec!["ollama-runtime".into()],
        release_date: None,
        last_updated: None,
    }
}

/// Synthetic profile for the built-in model. `context_window` is the `-c` we
/// launch llama-server with (`builtin::BUILTIN_CTX`) — the compactor budgets
/// against the real window instead of a conservative fallback. Pricing stays
/// None so `cost_estimate` correctly returns None (local inference is free).
fn builtin_synthetic_profile(req: &TurnRequest) -> ModelProfile {
    use crate::harness::builtin::BUILTIN_CTX;
    use crate::harness::tokens::TokenizerKind;
    ModelProfile {
        id: format!("builtin::{}", req.model_id),
        model_id: req.model_id.clone(),
        provider: "builtin".into(),
        name: req.model_id.clone(),
        family: None,
        context_window: Some(BUILTIN_CTX),
        output_limit: None,
        supports_tools: true,
        structured_output: false,
        reasoning: false,
        attachment: false,
        modalities_in: vec!["text".into()],
        modalities_out: vec!["text".into()],
        pricing_input_per_million: None,
        pricing_output_per_million: None,
        tokenizer_hint: TokenizerKind::OllamaNative,
        source: vec!["builtin-runtime".into()],
        release_date: None,
        last_updated: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::mock::{error_item, final_item, hung_stream, scripted_stream, text_item};
    use std::time::Duration;

    #[tokio::test]
    async fn consume_stream_collects_text_and_usage() {
        let mut stream = scripted_stream(vec![
            text_item("Hello "),
            text_item("world"),
            final_item("Hello world", 42, 7),
        ]);
        let cancel = CancelHandle::new();
        let (mut in_tok, mut out_tok) = (0u32, 0u32);
        let (text, err) = consume_stream(&mut stream, &cancel, &mut in_tok, &mut out_tok).await;
        assert_eq!(text, "Hello world");
        assert!(err.is_none());
        assert_eq!(in_tok, 42);
        assert_eq!(out_tok, 7);
    }

    #[tokio::test]
    async fn consume_stream_keeps_partial_text_on_error() {
        // RHC-02: a mid-stream provider error must not discard gathered text.
        let mut stream = scripted_stream(vec![
            text_item("partial answer "),
            error_item("HTTP 429 Too Many Requests"),
            // Never reached — the drain stops at the error.
            text_item("unreachable"),
        ]);
        let cancel = CancelHandle::new();
        let (mut in_tok, mut out_tok) = (0u32, 0u32);
        let (text, err) = consume_stream(&mut stream, &cancel, &mut in_tok, &mut out_tok).await;
        assert_eq!(text, "partial answer ");
        let err = err.expect("stream error surfaced");
        assert!(err.contains("429"), "raw provider text kept: {err}");
        assert_eq!(
            classify_message(&err),
            HarnessErrorKind::RateLimited,
            "stream errors classify for the turn-error event"
        );
    }

    #[tokio::test]
    async fn cancel_aborts_a_hung_provider_stream() {
        // THE regression this rework fixes: the old flag-only check ran
        // between stream items, so a provider that stopped yielding could
        // never be cancelled. The select-based drain must exit promptly.
        let cancel = Arc::new(CancelHandle::new());
        let canceller = cancel.clone();
        let waker = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            canceller.cancel();
        });
        let mut stream = hung_stream();
        let (mut in_tok, mut out_tok) = (0u32, 0u32);
        let drained = tokio::time::timeout(
            Duration::from_millis(2_000),
            consume_stream(&mut stream, &cancel, &mut in_tok, &mut out_tok),
        )
        .await
        .expect("cancel must abort a hung stream well inside the timeout");
        let (text, err) = drained;
        assert!(text.is_empty());
        assert!(err.is_none(), "cancel is not a stream error");
        assert!(cancel.is_cancelled());
        waker.await.expect("waker task");
    }

    #[tokio::test]
    async fn cancel_before_drain_exits_immediately() {
        let cancel = CancelHandle::new();
        cancel.cancel();
        let mut stream = scripted_stream(vec![text_item("never consumed")]);
        let (mut in_tok, mut out_tok) = (0u32, 0u32);
        let (text, err) = tokio::time::timeout(
            Duration::from_millis(500),
            consume_stream(&mut stream, &cancel, &mut in_tok, &mut out_tok),
        )
        .await
        .expect("pre-cancelled drain returns at once");
        // biased select checks cancellation first — nothing is consumed.
        assert!(text.is_empty());
        assert!(err.is_none());
    }

    #[test]
    fn envelope_extracts_sources_and_skills_deterministically() {
        let tool_entries = vec![
            ToolLogEntry {
                call_id: "c1".into(),
                name: "search_skills".into(),
                args_json: "{}".into(),
                result: Some(crate::harness::tools::ToolResult {
                    title: "search_skills".into(),
                    kind: "skill-search".into(),
                    snippet: "1 result".into(),
                    data: serde_json::json!({
                        "skills": [{ "id": "artifact:abc123", "name": "rust-testing" }]
                    }),
                    items_count: Some(1),
                    source: Some("local".into()),
                    elapsed_ms: 1,
                }),
                error: None,
                vetoed: false,
            },
            ToolLogEntry {
                call_id: "c2".into(),
                name: "web_search".into(),
                args_json: "{}".into(),
                result: Some(crate::harness::tools::ToolResult {
                    title: "web_search".into(),
                    kind: "web-search".into(),
                    snippet: "1 result".into(),
                    data: serde_json::json!({
                        "results": [
                            { "title": "Rig docs", "url": "https://docs.rig.rs" },
                            { "title": "Duplicate Rig docs", "url": "HTTPS://DOCS.RIG.RS/#start" }
                        ]
                    }),
                    items_count: Some(1),
                    source: Some("duckduckgo".into()),
                    elapsed_ms: 1,
                }),
                error: None,
                vetoed: false,
            },
        ];
        let event = build_final_envelope(
            "s1",
            "use **rust-testing** `artifact:abc123`",
            &tool_entries,
        );
        match event {
            HarnessEvent::FinalEnvelope {
                session_id,
                suggested_next_prompts,
                sources,
                skills_referenced,
            } => {
                assert_eq!(session_id, "s1");
                assert!(!suggested_next_prompts.is_empty());
                assert!(suggested_next_prompts.len() <= 4);
                assert_eq!(sources.len(), 1);
                assert_eq!(sources[0].url, "https://docs.rig.rs");
                assert_eq!(skills_referenced, vec!["artifact:abc123".to_string()]);
            }
            other => panic!("expected FinalEnvelope, got {other:?}"),
        }
    }
}
