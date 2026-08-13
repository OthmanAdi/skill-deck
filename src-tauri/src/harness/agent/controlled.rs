// @agent-context: Provider-neutral app-owned transcript loop.
//
// Rig is used for exactly one provider dispatch at a time. Skill Deck owns the
// transcript, tool policy, cancellation, dispatch, full-result audit trail,
// bounded model projection, and the separate tool-free finalization call.

use crate::harness::cancel::CancelHandle;
use crate::harness::context::{DispatchUsage, TurnBudget};
use crate::harness::error::parse_context_overflow;
use crate::harness::hooks::TelemetryHook;
use crate::harness::source_identity::{stable_dedup_by_identity, SourceIdentity};
use crate::harness::step_policy::{StepDecision, StepPolicy};
use crate::harness::tokens::Counter;
use crate::harness::tools::model_projection::{project_tool_result, ProjectionBudget};
use crate::harness::tools::{self, HarnessContext, ToolResult};
use futures_util::StreamExt;
use rig::agent::Agent;
use rig::completion::{CompletionModel, GetTokenUsage, ToolDefinition};
use rig::message::{
    AssistantContent, Message, Reasoning, ToolCall, ToolResultContent, UserContent,
};
use rig::streaming::{StreamedAssistantContent, StreamingCompletion};
use rig::OneOrMany;
use serde_json::{json, Value};

const FINALIZER_PROMPT: &str = "Produce the final answer now. Use only the evidence already in \
this transcript. Preserve real IDs, URLs, install commands, errors, and zero-result states.";
const DUPLICATE_REUSE_REASON: &str =
    "Equivalent tool call suppressed; the prior bounded result was reused.";
const DUPLICATE_CORRECTIVE_PROMPT: &str = "Skill Deck reused the prior result for an \
equivalent call. Continue once: use a different source required by the request, or finish \
from the evidence already gathered. Do not repeat the equivalent call.";

pub(super) struct ControlledLoopOutcome {
    pub final_text: String,
    pub stream_error: Option<String>,
    pub in_tokens: u32,
    pub out_tokens: u32,
}

struct ProviderTurn {
    assistant: Message,
    text: String,
    input_tokens: u32,
    output_tokens: u32,
    error: Option<String>,
    cancelled: bool,
}

#[allow(async_fn_in_trait)]
trait ModelDispatcher {
    fn preamble(&self) -> &str;
    fn tool_definitions(&self) -> &[ToolDefinition];

    async fn dispatch(
        &self,
        prompt: Message,
        history: Vec<Message>,
        cancel: &CancelHandle,
        hook: Option<&TelemetryHook>,
    ) -> ProviderTurn;
}

#[allow(async_fn_in_trait)]
trait AppToolDispatcher {
    async fn dispatch(&self, name: &str, args_json: &str) -> Result<ToolResult, String>;
}

struct RigModelDispatcher<'a, M>
where
    M: CompletionModel,
{
    agent: &'a Agent<M>,
    preamble: String,
    definitions: Vec<ToolDefinition>,
}

impl<'a, M> RigModelDispatcher<'a, M>
where
    M: CompletionModel,
{
    fn new(agent: &'a Agent<M>, preamble: String, definitions: Vec<ToolDefinition>) -> Self {
        Self {
            agent,
            preamble,
            definitions,
        }
    }
}

impl<M> ModelDispatcher for RigModelDispatcher<'_, M>
where
    M: CompletionModel + 'static,
    M::StreamingResponse: GetTokenUsage,
{
    fn preamble(&self) -> &str {
        &self.preamble
    }

    fn tool_definitions(&self) -> &[ToolDefinition] {
        &self.definitions
    }

    async fn dispatch(
        &self,
        prompt: Message,
        history: Vec<Message>,
        cancel: &CancelHandle,
        hook: Option<&TelemetryHook>,
    ) -> ProviderTurn {
        let builder = tokio::select! {
            biased;
            _ = cancel.cancelled() => return cancelled_turn(),
            result = self.agent.stream_completion(prompt, history) => {
                match result {
                    Ok(builder) => builder,
                    Err(error) => return failed_turn(format!("{error}")),
                }
            }
        };
        let mut stream = tokio::select! {
            biased;
            _ = cancel.cancelled() => return cancelled_turn(),
            result = builder.stream() => {
                match result {
                    Ok(stream) => stream,
                    Err(error) => return failed_turn(format!("{error}")),
                }
            }
        };

        let mut content = Vec::new();
        let mut text = String::new();
        let mut input_tokens = 0u32;
        let mut output_tokens = 0u32;
        let mut stream_error = None;
        let mut cancelled = false;

        loop {
            let item = tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    stream.cancel();
                    cancelled = true;
                    break;
                }
                item = stream.next() => item,
            };
            let Some(item) = item else {
                break;
            };
            match item {
                Ok(StreamedAssistantContent::Text(delta)) => {
                    text.push_str(&delta.text);
                    push_text(&mut content, &delta.text);
                    if let Some(hook) = hook {
                        hook.trace_text_delta(&delta.text);
                    }
                }
                Ok(StreamedAssistantContent::ToolCall { tool_call, .. }) => {
                    content.push(AssistantContent::ToolCall(tool_call));
                }
                Ok(StreamedAssistantContent::Reasoning(reasoning)) => {
                    content.push(AssistantContent::Reasoning(reasoning));
                }
                Ok(StreamedAssistantContent::ReasoningDelta { id, reasoning }) => {
                    content.push(AssistantContent::Reasoning(
                        Reasoning::new(&reasoning).optional_id(id),
                    ));
                }
                Ok(StreamedAssistantContent::ToolCallDelta { .. }) => {}
                Ok(StreamedAssistantContent::Final(response)) => {
                    if let Some(usage) = response.token_usage() {
                        input_tokens = input_tokens.saturating_add(to_u32(usage.input_tokens));
                        output_tokens = output_tokens.saturating_add(to_u32(usage.output_tokens));
                    }
                }
                Err(error) => {
                    stream_error = Some(format!("{error}"));
                    break;
                }
            }
        }

        if content.is_empty() {
            content.push(AssistantContent::text(text.clone()));
        }
        ProviderTurn {
            assistant: Message::Assistant {
                id: stream.message_id.clone(),
                content: one_or_many(content),
            },
            text,
            input_tokens,
            output_tokens,
            error: stream_error,
            cancelled,
        }
    }
}

struct HarnessToolDispatcher<'a> {
    ctx: &'a HarnessContext,
}

impl AppToolDispatcher for HarnessToolDispatcher<'_> {
    async fn dispatch(&self, name: &str, args_json: &str) -> Result<ToolResult, String> {
        tools::dispatch(name, args_json, self.ctx)
            .await
            .map_err(|error| format!("{error}"))
    }
}

pub(super) struct ControlledRun<'a, M>
where
    M: CompletionModel,
{
    pub tool_agent: &'a Agent<M>,
    pub finalizer_agent: &'a Agent<M>,
    pub tool_preamble: String,
    pub finalizer_preamble: String,
    pub tool_definitions: Vec<ToolDefinition>,
    pub ctx: &'a HarnessContext,
    pub hook: &'a TelemetryHook,
    pub cancel: &'a CancelHandle,
    pub history: Vec<Message>,
    pub initial_prompt: Message,
    pub budget: TurnBudget,
}

pub(super) async fn run<M>(input: ControlledRun<'_, M>) -> ControlledLoopOutcome
where
    M: CompletionModel + 'static,
    M::StreamingResponse: GetTokenUsage,
{
    let tool_model = RigModelDispatcher::new(
        input.tool_agent,
        input.tool_preamble,
        input.tool_definitions,
    );
    let finalizer_model =
        RigModelDispatcher::new(input.finalizer_agent, input.finalizer_preamble, Vec::new());
    let tool_dispatcher = HarnessToolDispatcher { ctx: input.ctx };
    run_loop(LoopInput {
        tool_model: &tool_model,
        finalizer_model: &finalizer_model,
        tool_dispatcher: &tool_dispatcher,
        hook: Some(input.hook),
        cancel: input.cancel,
        history: input.history,
        initial_prompt: input.initial_prompt,
        budget: input.budget,
    })
    .await
}

pub(super) fn without_current_user(mut history: Vec<Message>, current_user: &str) -> Vec<Message> {
    let is_current_user = history.last().is_some_and(|message| match message {
        Message::User { content } => {
            let mut items = content.iter();
            matches!(
                (items.next(), items.next()),
                (Some(UserContent::Text(text)), None) if text.text == current_user
            )
        }
        _ => false,
    });
    if is_current_user {
        history.pop();
    }
    history
}

struct LoopInput<'a, M, F, T> {
    tool_model: &'a M,
    finalizer_model: &'a F,
    tool_dispatcher: &'a T,
    hook: Option<&'a TelemetryHook>,
    cancel: &'a CancelHandle,
    history: Vec<Message>,
    initial_prompt: Message,
    budget: TurnBudget,
}

async fn run_loop<M, F, T>(input: LoopInput<'_, M, F, T>) -> ControlledLoopOutcome
where
    M: ModelDispatcher,
    F: ModelDispatcher,
    T: AppToolDispatcher,
{
    let LoopInput {
        tool_model,
        finalizer_model,
        tool_dispatcher,
        hook,
        cancel,
        mut history,
        initial_prompt: mut current_prompt,
        budget,
    } = input;
    let mut policy = StepPolicy::new(budget.max_tool_steps, budget.max_same_tool_calls);
    let mut evidence = Vec::new();
    let mut retained_tool_tokens = 0u32;
    let mut in_tokens = 0u32;
    let mut out_tokens = 0u32;
    let mut last_error = None;
    let mut had_tool_dispatch = false;
    let mut current_prompt_in_history = false;
    let mut duplicate_reuses = 0u32;
    let mut overflow_retry_used = false;
    let mut calibrator = PromptCalibrator::default();

    loop {
        let base_estimate = estimate_request_tokens(tool_model, &history, &current_prompt, &budget);
        let estimate = calibrator.apply(base_estimate);
        if estimate > budget.prompt_budget_tokens {
            last_error = Some(format!(
                "context budget exceeded before provider dispatch: estimated {estimate} tokens, limit {}",
                budget.prompt_budget_tokens
            ));
            break;
        }
        if let Some(hook) = hook {
            hook.trace_pre_model(history.len().saturating_add(2), estimate);
        }

        // After tool evidence exists, tool-enabled calls are transcript
        // continuations, not the final answer. Suppress their draft prose so
        // the UI receives one final stream from the tool-free finalizer.
        let stream_hook = if had_tool_dispatch { None } else { hook };
        let mut turn = tool_model
            .dispatch(current_prompt.clone(), history.clone(), cancel, stream_hook)
            .await;
        if let Some(error) = turn.error.as_deref() {
            if let Some(overflow) = parse_context_overflow(error) {
                if let Some(exact_prompt_tokens) = overflow.prompt_tokens {
                    calibrator.observe_exact(base_estimate, exact_prompt_tokens);
                }
            }
            if !overflow_retry_used && parse_context_overflow(error).is_some() {
                let before = serialized_request_fingerprint(&history, &current_prompt);
                if reduce_request_after_overflow(&mut history, &mut current_prompt) {
                    let after = serialized_request_fingerprint(&history, &current_prompt);
                    if before != after {
                        overflow_retry_used = true;
                        let retry_base =
                            estimate_request_tokens(tool_model, &history, &current_prompt, &budget);
                        let retry_estimate = calibrator.apply(retry_base);
                        if retry_estimate <= budget.prompt_budget_tokens {
                            if let Some(hook) = hook {
                                hook.trace_pre_model(
                                    history.len().saturating_add(2),
                                    retry_estimate,
                                );
                            }
                            turn = tool_model
                                .dispatch(
                                    current_prompt.clone(),
                                    history.clone(),
                                    cancel,
                                    stream_hook,
                                )
                                .await;
                            if turn.input_tokens > 0 {
                                calibrator.observe_exact(retry_base, turn.input_tokens);
                            }
                        }
                    }
                }
            }
        }
        if turn.input_tokens > 0 {
            calibrator.observe_exact(base_estimate, turn.input_tokens);
        }
        in_tokens = in_tokens.saturating_add(turn.input_tokens);
        out_tokens = out_tokens.saturating_add(turn.output_tokens);
        let partial_text = turn.text.clone();

        if turn.cancelled || cancel.is_cancelled() {
            let final_text = if evidence.is_empty() {
                nonempty_or(
                    partial_text,
                    "The turn was cancelled before any evidence was gathered.",
                )
            } else {
                evidence_answer(&evidence)
            };
            return ControlledLoopOutcome {
                final_text,
                stream_error: None,
                in_tokens,
                out_tokens,
            };
        }
        if let Some(error) = turn.error {
            last_error = Some(error);
            if !evidence.is_empty() {
                break;
            }
            return ControlledLoopOutcome {
                final_text: nonempty_or(
                    partial_text,
                    "The provider stopped before Skill Deck could gather evidence.",
                ),
                stream_error: last_error,
                in_tokens,
                out_tokens,
            };
        }

        let tool_calls = assistant_tool_calls(&turn.assistant);
        history.push(current_prompt.clone());
        current_prompt_in_history = true;
        history.push(turn.assistant);

        if tool_calls.is_empty() {
            if !had_tool_dispatch {
                return ControlledLoopOutcome {
                    final_text: nonempty_or(turn.text, "The model returned an empty response."),
                    stream_error: None,
                    in_tokens,
                    out_tokens,
                };
            }
            break;
        }

        let mut result_contents = Vec::with_capacity(tool_calls.len());
        let mut force_finalize = false;
        let mut offer_duplicate_correction = false;
        for tool_call in tool_calls {
            let call_id = tool_call.id.clone();
            let tool_name = tool_call.function.name.clone();
            let args_value = tool_call.function.arguments.clone();
            let args_json = serde_json::to_string(&args_value).unwrap_or_else(|_| "{}".to_string());
            if let Some(hook) = hook {
                hook.trace_tool_start(call_id.clone(), tool_name.clone(), args_json.clone());
            }

            let decision = policy.evaluate(&tool_name, &args_value);
            let projected = match decision {
                StepDecision::Reuse {
                    bounded_projection, ..
                } => {
                    duplicate_reuses = duplicate_reuses.saturating_add(1);
                    if duplicate_reuses == 1 {
                        offer_duplicate_correction = true;
                    } else {
                        force_finalize = true;
                    }
                    if let Some(hook) = hook {
                        hook.trace_tool_veto(
                            call_id.clone(),
                            tool_name.clone(),
                            DUPLICATE_REUSE_REASON.to_string(),
                        );
                    }
                    evidence.push(EvidenceEntry::not_executed(
                        tool_name.clone(),
                        DUPLICATE_REUSE_REASON.to_string(),
                    ));
                    bounded_projection
                }
                StepDecision::Veto { reason, .. } => {
                    force_finalize = true;
                    if let Some(hook) = hook {
                        hook.trace_tool_veto(
                            call_id.clone(),
                            tool_name.clone(),
                            reason.to_string(),
                        );
                    }
                    evidence.push(EvidenceEntry::not_executed(
                        tool_name.clone(),
                        reason.to_string(),
                    ));
                    veto_projection(&tool_name, reason)
                }
                StepDecision::Execute { fingerprint } => {
                    let same_tool_before = policy.calls_for(&tool_name).saturating_sub(1);
                    let steps_before = policy.attempted_steps().saturating_sub(1);
                    let base_prompt_tokens =
                        estimate_history_tokens(tool_model, &history, &result_contents, &budget);
                    let admission = budget.admit_dispatch(
                        DispatchUsage {
                            prompt_tokens: base_prompt_tokens,
                            tool_result_tokens: retained_tool_tokens,
                            tool_steps: steps_before,
                            same_tool_calls: same_tool_before,
                        },
                        budget.per_tool_result_max_tokens,
                    );
                    if let Err(rejection) = admission {
                        let reason = format!("Tool dispatch vetoed by context budget: {rejection}");
                        force_finalize = true;
                        if let Some(hook) = hook {
                            hook.trace_tool_veto(
                                call_id.clone(),
                                tool_name.clone(),
                                reason.clone(),
                            );
                        }
                        evidence.push(EvidenceEntry::not_executed(
                            tool_name.clone(),
                            reason.clone(),
                        ));
                        veto_projection(&tool_name, &reason)
                    } else {
                        had_tool_dispatch = true;
                        let dispatch = tokio::select! {
                            biased;
                            _ = cancel.cancelled() => None,
                            result = tool_dispatcher.dispatch(&tool_name, &args_json) => Some(result),
                        };
                        let Some(dispatch) = dispatch else {
                            let message = "cancelled during tool dispatch".to_string();
                            if let Some(hook) = hook {
                                hook.trace_tool_error(
                                    call_id.clone(),
                                    tool_name.clone(),
                                    message.clone(),
                                );
                            }
                            evidence.push(EvidenceEntry::not_executed(tool_name.clone(), message));
                            force_finalize = true;
                            break;
                        };
                        match dispatch {
                            Ok(full_result) => {
                                let serialized = serde_json::to_string(&full_result)
                                    .unwrap_or_else(|_| "{}".to_string());
                                let bounded = project_tool_result(
                                    &tool_name,
                                    &serialized,
                                    ProjectionBudget::from_max_tokens(
                                        budget.per_tool_result_max_tokens as usize,
                                    ),
                                );
                                policy.record_result(
                                    &fingerprint,
                                    bounded.clone(),
                                    is_zero_result(&full_result),
                                );
                                if let Some(hook) = hook {
                                    hook.trace_tool_result(
                                        call_id.clone(),
                                        tool_name.clone(),
                                        full_result.clone(),
                                    );
                                }
                                evidence
                                    .push(EvidenceEntry::success(tool_name.clone(), full_result));
                                bounded
                            }
                            Err(error) => {
                                if let Some(hook) = hook {
                                    hook.trace_tool_error(
                                        call_id.clone(),
                                        tool_name.clone(),
                                        error.clone(),
                                    );
                                }
                                evidence.push(EvidenceEntry::dispatch_error(
                                    tool_name.clone(),
                                    error.clone(),
                                ));
                                let bounded = error_projection(&tool_name, &error, &budget);
                                policy.record_result(&fingerprint, bounded.clone(), false);
                                bounded
                            }
                        }
                    }
                }
            };

            retained_tool_tokens = retained_tool_tokens
                .saturating_add(Counter::count(&budget.context.tokenizer, &projected));
            result_contents.push(correlated_tool_result(&tool_call, projected));
        }

        if cancel.is_cancelled() {
            return ControlledLoopOutcome {
                final_text: if evidence.is_empty() {
                    "The turn was cancelled before any evidence was gathered.".to_string()
                } else {
                    evidence_answer(&evidence)
                },
                stream_error: None,
                in_tokens,
                out_tokens,
            };
        }
        if result_contents.is_empty() {
            break;
        }
        if offer_duplicate_correction && !force_finalize {
            result_contents.push(UserContent::text(DUPLICATE_CORRECTIVE_PROMPT));
        }
        current_prompt = Message::User {
            content: one_or_many(result_contents),
        };
        current_prompt_in_history = false;
        if force_finalize || policy.attempted_steps() >= budget.max_tool_steps {
            break;
        }
    }

    if !had_tool_dispatch {
        let message = last_error.clone().unwrap_or_else(|| {
            "The controlled turn stopped before a provider response could fit.".to_string()
        });
        return ControlledLoopOutcome {
            final_text: format!(
                "I could not run this request because the model context budget was exceeded. {message}"
            ),
            stream_error: last_error,
            in_tokens,
            out_tokens,
        };
    }

    if !current_prompt_in_history {
        history.push(current_prompt);
    }
    let finalizer_prompt = Message::user(FINALIZER_PROMPT);
    let finalizer_estimate = calibrator.apply(estimate_request_tokens(
        finalizer_model,
        &history,
        &finalizer_prompt,
        &budget,
    ));
    if finalizer_estimate > budget.prompt_budget_tokens {
        last_error = Some(format!(
            "context budget exceeded before tool-free finalization: estimated {finalizer_estimate} tokens, limit {}",
            budget.prompt_budget_tokens
        ));
    } else if finalizer_model.tool_definitions().is_empty() {
        if let Some(hook) = hook {
            hook.trace_pre_model(history.len().saturating_add(2), finalizer_estimate);
        }
        // Finalizer text is evidence synthesis, so buffer it until source claims
        // have been checked. Streaming it directly would expose an unsupported
        // claim before the app had a chance to repair the answer.
        let final_turn = finalizer_model
            .dispatch(finalizer_prompt, history, cancel, None)
            .await;
        in_tokens = in_tokens.saturating_add(final_turn.input_tokens);
        out_tokens = out_tokens.saturating_add(final_turn.output_tokens);
        if final_turn.cancelled || cancel.is_cancelled() {
            return ControlledLoopOutcome {
                final_text: evidence_answer(&evidence),
                stream_error: None,
                in_tokens,
                out_tokens,
            };
        }
        if let Some(error) = final_turn.error {
            last_error = Some(error);
        } else if !final_turn.text.trim().is_empty()
            && assistant_tool_calls(&final_turn.assistant).is_empty()
        {
            let final_text = grounded_final_text(final_turn.text, &evidence);
            emit_final_text(hook, &final_text);
            return ControlledLoopOutcome {
                final_text,
                stream_error: None,
                in_tokens,
                out_tokens,
            };
        } else {
            last_error = Some("tool-free finalizer returned no usable text".to_string());
        }
    } else {
        last_error = Some("tool-free finalizer was configured with tool definitions".to_string());
    }

    let final_text = evidence_answer(&evidence);
    emit_final_text(hook, &final_text);
    ControlledLoopOutcome {
        final_text,
        stream_error: last_error,
        in_tokens,
        out_tokens,
    }
}

fn estimate_request_tokens<M: ModelDispatcher>(
    model: &M,
    history: &[Message],
    prompt: &Message,
    budget: &TurnBudget,
) -> u32 {
    let mut corpus = String::new();
    corpus.push_str(model.preamble());
    corpus.push('\n');
    corpus.push_str(
        &serde_json::to_string(model.tool_definitions()).unwrap_or_else(|_| "[]".to_string()),
    );
    corpus.push('\n');
    for message in history {
        corpus.push_str(&serde_json::to_string(message).unwrap_or_default());
        corpus.push('\n');
    }
    corpus.push_str(&serde_json::to_string(prompt).unwrap_or_default());
    Counter::count(&budget.context.tokenizer, &corpus).max(1)
}

/// Exact provider-reported prompt usage includes the provider's tokenizer and
/// chat template. Once observed, use the largest ratio (plus 5% drift reserve)
/// to calibrate later preflight estimates in this turn. Before an exact sample
/// exists, the underlying counter remains the conservative estimator selected
/// by `ContextSpec`.
#[derive(Debug, Default)]
struct PromptCalibrator {
    multiplier_milli: u32,
}

impl PromptCalibrator {
    fn apply(&self, estimate: u32) -> u32 {
        let multiplier = self.multiplier_milli.max(1_000);
        u32::try_from(
            u64::from(estimate)
                .saturating_mul(u64::from(multiplier))
                .saturating_add(999)
                / 1_000,
        )
        .unwrap_or(u32::MAX)
    }

    fn observe_exact(&mut self, estimated: u32, exact: u32) {
        if estimated == 0 || exact == 0 {
            return;
        }
        let ratio_milli = u32::try_from(
            u64::from(exact)
                .saturating_mul(1_000)
                .saturating_add(u64::from(estimated) - 1)
                / u64::from(estimated),
        )
        .unwrap_or(u32::MAX);
        let with_drift = u32::try_from(
            u64::from(ratio_milli)
                .saturating_mul(105)
                .saturating_add(99)
                / 100,
        )
        .unwrap_or(u32::MAX);
        self.multiplier_milli = self.multiplier_milli.max(with_drift).max(1_000);
    }
}

fn serialized_request_fingerprint(history: &[Message], prompt: &Message) -> String {
    serde_json::to_string(&(history, prompt)).unwrap_or_default()
}

/// Make one deterministic, materially smaller retry request after a provider
/// proves the estimator was optimistic. Oldest history is removed first. If
/// there is no history, long text fields in the current prompt are reduced
/// while structural fields such as tool-call IDs and names are preserved.
fn reduce_request_after_overflow(history: &mut Vec<Message>, prompt: &mut Message) -> bool {
    if !history.is_empty() {
        let remove = history.len().div_ceil(3).max(1);
        history.drain(..remove.min(history.len()));
        return true;
    }

    let Ok(mut value) = serde_json::to_value(&*prompt) else {
        return false;
    };
    let changed = shrink_payload_strings(&mut value, None);
    if !changed {
        return false;
    }
    match serde_json::from_value(value) {
        Ok(reduced) => {
            *prompt = reduced;
            true
        }
        Err(_) => false,
    }
}

fn shrink_payload_strings(value: &mut Value, field: Option<&str>) -> bool {
    match value {
        Value::String(text)
            if !matches!(
                field,
                Some("id" | "call_id" | "callId" | "name" | "type" | "role")
            ) && text.chars().count() > 256 =>
        {
            let total = text.chars().count();
            let head: String = text.chars().take(112).collect();
            let tail: String = text
                .chars()
                .rev()
                .take(112)
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            *text =
                format!("{head}\n[... {total} chars reduced after context overflow ...]\n{tail}");
            true
        }
        Value::Array(values) => {
            let mut changed = false;
            for value in values {
                changed |= shrink_payload_strings(value, field);
            }
            changed
        }
        Value::Object(values) => {
            let mut changed = false;
            for (key, value) in values {
                changed |= shrink_payload_strings(value, Some(key.as_str()));
            }
            changed
        }
        _ => false,
    }
}

fn estimate_history_tokens<M: ModelDispatcher>(
    model: &M,
    history: &[Message],
    pending_results: &[UserContent],
    budget: &TurnBudget,
) -> u32 {
    let prompt = if pending_results.is_empty() {
        Message::user("")
    } else {
        Message::User {
            content: one_or_many(pending_results.to_vec()),
        }
    };
    estimate_request_tokens(model, history, &prompt, budget)
}

fn correlated_tool_result(tool_call: &ToolCall, projection: String) -> UserContent {
    let content = OneOrMany::one(ToolResultContent::text(projection));
    match tool_call.call_id.clone() {
        Some(call_id) => {
            UserContent::tool_result_with_call_id(tool_call.id.clone(), call_id, content)
        }
        None => UserContent::tool_result(tool_call.id.clone(), content),
    }
}

fn assistant_tool_calls(message: &Message) -> Vec<ToolCall> {
    match message {
        Message::Assistant { content, .. } => content
            .iter()
            .filter_map(|content| match content {
                AssistantContent::ToolCall(tool_call) => Some(tool_call.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn is_zero_result(result: &ToolResult) -> bool {
    result.items_count == Some(0)
        || result
            .data
            .get("count")
            .and_then(Value::as_u64)
            .is_some_and(|count| count == 0)
}

fn veto_projection(tool_name: &str, reason: &str) -> String {
    json!({
        "schema": "skill-deck.tool-veto.v1",
        "tool": tool_name,
        "status": "vetoed",
        "reason": reason,
    })
    .to_string()
}

fn error_projection(tool_name: &str, error: &str, budget: &TurnBudget) -> String {
    let envelope = json!({
        "title": format!("{tool_name} failed"),
        "kind": "tool-error",
        "snippet": error,
        "data": { "errors": [error] },
        "itemsCount": 0,
        "source": tool_name,
        "elapsedMs": 0,
    })
    .to_string();
    project_tool_result(
        tool_name,
        &envelope,
        ProjectionBudget::from_max_tokens(budget.per_tool_result_max_tokens as usize),
    )
}

fn failed_turn(error: String) -> ProviderTurn {
    ProviderTurn {
        assistant: Message::assistant(""),
        text: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        error: Some(error),
        cancelled: false,
    }
}

fn cancelled_turn() -> ProviderTurn {
    ProviderTurn {
        assistant: Message::assistant(""),
        text: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        error: None,
        cancelled: true,
    }
}

fn push_text(content: &mut Vec<AssistantContent>, delta: &str) {
    if let Some(AssistantContent::Text(text)) = content.last_mut() {
        text.text.push_str(delta);
    } else {
        content.push(AssistantContent::text(delta));
    }
}

fn one_or_many<T: Clone>(mut values: Vec<T>) -> OneOrMany<T> {
    let mut values = values.drain(..);
    let Some(first) = values.next() else {
        unreachable!("controlled transcript content is never empty");
    };
    let mut content = OneOrMany::one(first);
    for value in values {
        content.push(value);
    }
    content
}

fn to_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn nonempty_or(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

#[derive(Clone)]
struct EvidenceEntry {
    tool: String,
    result: Option<ToolResult>,
    error: Option<String>,
    origin: EvidenceOrigin,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EvidenceOrigin {
    SuccessfulResult,
    DispatchError,
    NotExecuted,
}

impl EvidenceEntry {
    fn success(tool: String, result: ToolResult) -> Self {
        Self {
            tool,
            result: Some(result),
            error: None,
            origin: EvidenceOrigin::SuccessfulResult,
        }
    }

    fn dispatch_error(tool: String, error: String) -> Self {
        Self {
            tool,
            result: None,
            error: Some(error),
            origin: EvidenceOrigin::DispatchError,
        }
    }

    fn not_executed(tool: String, reason: String) -> Self {
        Self {
            tool,
            result: None,
            error: Some(reason),
            origin: EvidenceOrigin::NotExecuted,
        }
    }
}

#[derive(Clone, Copy)]
enum EvidenceSource {
    Marketplace,
    Web,
}

impl EvidenceSource {
    const ALL: [Self; 2] = [Self::Marketplace, Self::Web];

    fn matches_tool(self, tool: &str) -> bool {
        match self {
            Self::Marketplace => tool == "search_marketplace",
            Self::Web => tool == "web_search",
        }
    }

    fn mentioned_in(self, sentence: &str) -> bool {
        match self {
            Self::Marketplace => {
                [
                    "marketplace",
                    "public skill",
                    "community skill",
                    "published skill",
                    "installable skill",
                    "skills.sh",
                    "skills sh",
                    "skillsh",
                    "clawhub",
                    "claw hub",
                    "claw-hub",
                    "the hub",
                ]
                .iter()
                .any(|alias| sentence.contains(alias))
                    || (sentence.contains("registry") && !sentence.contains("local registry"))
            }
            Self::Web => [
                "web search",
                "web result",
                "the web",
                "internet",
                "online search",
                "online result",
            ]
            .iter()
            .any(|alias| sentence.contains(alias)),
        }
    }
}

fn grounded_final_text(candidate: String, entries: &[EvidenceEntry]) -> String {
    if contains_unsupported_source_claim(&candidate, entries) {
        evidence_answer_with_mode(entries, EvidenceAnswerMode::GroundingRepair)
    } else {
        candidate
    }
}

#[derive(Clone, Copy)]
enum SourceClaim {
    SearchPerformed,
    Failure,
    ZeroResults,
    PositiveResults,
}

fn contains_unsupported_source_claim(candidate: &str, entries: &[EvidenceEntry]) -> bool {
    candidate
        .to_lowercase()
        .split(['.', '!', '?', '\n'])
        .any(|sentence| {
            if is_future_source_instruction(sentence) || is_source_noncoverage_disclaimer(sentence)
            {
                return false;
            }
            EvidenceSource::ALL.into_iter().any(|source| {
                if !source.mentioned_in(sentence) {
                    return false;
                }
                if !source_was_queried(source, entries) {
                    return true;
                }
                classify_source_claim(sentence)
                    .is_some_and(|claim| !source_claim_is_supported(source, claim, entries))
            })
        })
}

fn classify_source_claim(sentence: &str) -> Option<SourceClaim> {
    if is_future_source_instruction(sentence) || is_source_noncoverage_disclaimer(sentence) {
        return None;
    }
    if is_result_absence_claim(sentence) {
        return Some(SourceClaim::ZeroResults);
    }
    if is_source_failure_claim(sentence) {
        return Some(SourceClaim::Failure);
    }
    if is_positive_result_claim(sentence) {
        return Some(SourceClaim::PositiveResults);
    }
    if is_search_performed_claim(sentence) {
        return Some(SourceClaim::SearchPerformed);
    }
    None
}

fn is_source_failure_claim(sentence: &str) -> bool {
    [
        " failed",
        "failure",
        " error",
        "errored",
        "timed out",
        "timeout",
        "unavailable",
        "could not complete",
        "couldn't complete",
    ]
    .iter()
    .any(|marker| sentence.contains(marker))
}

fn is_positive_result_claim(sentence: &str) -> bool {
    [
        "found ",
        "returned ",
        "yielded ",
        "offers ",
        "lists ",
        "shows ",
        "contains ",
        "provides ",
        "available ",
        "result is",
        "results are",
        "results include",
        "results show",
        "match is",
        "matches are",
        " has a ",
        " has an ",
    ]
    .iter()
    .any(|marker| sentence.contains(marker))
}

fn is_search_performed_claim(sentence: &str) -> bool {
    [
        "searched",
        "search found",
        "search returned",
        "search yielded",
        "checked",
        "queried",
        "looked on",
        "looked in",
        "looked at",
        "according to",
    ]
    .iter()
    .any(|marker| sentence.contains(marker))
}

fn is_source_noncoverage_disclaimer(sentence: &str) -> bool {
    [
        "not searched",
        "not search",
        "not queried",
        "not checked",
        "did not search",
        "didn't search",
        "did not use",
        "didn't use",
        "was not run",
        "wasn't run",
        "was not used",
        "wasn't used",
    ]
    .iter()
    .any(|disclaimer| sentence.contains(disclaimer))
}

fn is_future_source_instruction(sentence: &str) -> bool {
    let sentence = sentence.trim().trim_start_matches(['-', '*', '•']).trim();
    let has_future_prefix = [
        "search ",
        "try ",
        "try searching ",
        "try the ",
        "consider ",
        "consider searching ",
        "consider the ",
        "you can search ",
        "you could search ",
        "you should search ",
        "you can try ",
        "you could try ",
        "you should try ",
        "we can search ",
        "we can try ",
        "i can search ",
        "i can try ",
        "next, search ",
        "next, try ",
    ]
    .iter()
    .any(|prefix| sentence.starts_with(prefix));
    has_future_prefix
        && !is_result_absence_claim(sentence)
        && !is_positive_result_claim(sentence)
        && !is_search_performed_claim(sentence)
}

fn is_result_absence_claim(sentence: &str) -> bool {
    let has_absence = [
        "no ",
        "none",
        "zero",
        "not find",
        "not found",
        "did not find",
        "didn't find",
        "could not find",
        "couldn't find",
        "found nothing",
        "returned nothing",
        "yielded nothing",
        "was empty",
    ]
    .iter()
    .any(|marker| sentence.contains(marker));
    let has_result_noun = [
        "skill",
        "result",
        "match",
        "item",
        "candidate",
        "recommendation",
        "nothing",
        "empty",
    ]
    .iter()
    .any(|noun| sentence.contains(noun));
    has_absence && has_result_noun
}

fn source_claim_is_supported(
    source: EvidenceSource,
    claim: SourceClaim,
    entries: &[EvidenceEntry],
) -> bool {
    let matching = entries
        .iter()
        .filter(|entry| source.matches_tool(&entry.tool))
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return false;
    }
    let successful = matching
        .iter()
        .filter(|entry| entry.origin == EvidenceOrigin::SuccessfulResult)
        .filter_map(|entry| entry.result.as_ref())
        .collect::<Vec<_>>();
    let had_dispatch_error = matching
        .iter()
        .any(|entry| entry.origin == EvidenceOrigin::DispatchError);
    match claim {
        SourceClaim::SearchPerformed => !successful.is_empty(),
        SourceClaim::Failure => had_dispatch_error,
        SourceClaim::ZeroResults => {
            !successful.is_empty() && successful.iter().all(|result| is_zero_result(result))
        }
        SourceClaim::PositiveResults => successful.iter().any(|result| !is_zero_result(result)),
    }
}

fn source_was_queried(source: EvidenceSource, entries: &[EvidenceEntry]) -> bool {
    entries.iter().any(|entry| {
        source.matches_tool(&entry.tool) && entry.origin != EvidenceOrigin::NotExecuted
    })
}

fn emit_final_text(hook: Option<&TelemetryHook>, text: &str) {
    if let Some(hook) = hook {
        hook.trace_text_delta(text);
    }
}

#[derive(Clone)]
struct EvidenceItem {
    group: &'static str,
    name: String,
    id: Option<String>,
    url: Option<String>,
    install: Option<String>,
    description: Option<String>,
    source: Option<String>,
}

fn evidence_answer(entries: &[EvidenceEntry]) -> String {
    evidence_answer_with_mode(entries, EvidenceAnswerMode::FinalizerFailure)
}

#[derive(Clone, Copy)]
enum EvidenceAnswerMode {
    FinalizerFailure,
    GroundingRepair,
}

fn evidence_answer_with_mode(entries: &[EvidenceEntry], mode: EvidenceAnswerMode) -> String {
    let mut items = Vec::new();
    let mut zero_sources = Vec::new();
    let mut errors = Vec::new();
    for entry in entries {
        if let Some(error) = entry.error.as_ref() {
            errors.push(format!("{}: {error}", entry.tool));
        }
        let Some(result) = entry.result.as_ref() else {
            continue;
        };
        if is_zero_result(result) {
            zero_sources.push(result.source.clone().unwrap_or_else(|| entry.tool.clone()));
        }
        collect_evidence_items(&entry.tool, result, &mut items);
    }
    let items = stable_dedup_by_identity(items, |item| {
        SourceIdentity::new(
            item.id.as_deref(),
            item.url.as_deref(),
            item.source.as_deref(),
        )
    });

    let mut out = String::new();
    if items.is_empty() {
        out.push_str(match mode {
            EvidenceAnswerMode::FinalizerFailure => {
                "I could not produce a model-written final answer, and no matching evidence was returned."
            }
            EvidenceAnswerMode::GroundingRepair => {
                "The draft final answer contained a source claim not supported by this turn. No matching evidence was returned by the tools that actually ran."
            }
        });
    } else {
        out.push_str(match mode {
            EvidenceAnswerMode::FinalizerFailure => {
                "I could not complete model finalization, so here is the verified evidence gathered this turn."
            }
            EvidenceAnswerMode::GroundingRepair => {
                "The draft final answer contained a source claim not supported by this turn. Here is the verified evidence gathered this turn."
            }
        });
        for group in ["Local", "Marketplace", "Web", "Other"] {
            let group_items = items
                .iter()
                .filter(|item| item.group == group)
                .collect::<Vec<_>>();
            if group_items.is_empty() {
                continue;
            }
            out.push_str(&format!("\n\n### {group}\n"));
            for item in group_items {
                out.push_str("- **");
                out.push_str(&item.name);
                out.push_str("**");
                if let Some(id) = item.id.as_deref() {
                    out.push_str(&format!(" `{id}`"));
                }
                if let Some(description) = item.description.as_deref() {
                    out.push_str(" — ");
                    out.push_str(description);
                }
                out.push('\n');
                if let Some(url) = item.url.as_deref() {
                    out.push_str(&format!("  {url}\n"));
                }
                if let Some(install) = item.install.as_deref() {
                    out.push_str(&format!("  Install: `{install}`\n"));
                }
            }
        }
    }
    zero_sources.sort();
    zero_sources.dedup();
    if !zero_sources.is_empty() {
        out.push_str("\n\nNo results: ");
        out.push_str(&zero_sources.join(", "));
        out.push('.');
    }
    errors.sort();
    errors.dedup();
    if !errors.is_empty() {
        out.push_str("\n\nErrors:\n");
        for error in errors {
            out.push_str("- ");
            out.push_str(&error);
            out.push('\n');
        }
    }
    out
}

fn collect_evidence_items(tool: &str, result: &ToolResult, out: &mut Vec<EvidenceItem>) {
    let data = &result.data;
    match tool {
        "search_skills" | "list_skills" => {
            collect_array(data.get("skills"), "Local", result.source.as_deref(), out)
        }
        "translate_to_skills" => {
            if let Some(candidates) = data.get("candidates").and_then(Value::as_array) {
                for candidate in candidates {
                    if let Some(skill) = candidate.get("skill") {
                        push_item(skill, "Local", result.source.as_deref(), out);
                    }
                }
            }
        }
        "search_marketplace" => collect_array(
            data.get("items"),
            "Marketplace",
            result.source.as_deref(),
            out,
        ),
        "web_search" => collect_array(data.get("results"), "Web", result.source.as_deref(), out),
        _ => {
            if let Some(skill) = data.get("skill") {
                push_item(skill, "Local", result.source.as_deref(), out);
            } else if !result.snippet.trim().is_empty() {
                out.push(EvidenceItem {
                    group: "Other",
                    name: result.title.clone(),
                    id: None,
                    url: None,
                    install: None,
                    description: Some(result.snippet.clone()),
                    source: result.source.clone(),
                });
            }
        }
    }
}

fn collect_array(
    value: Option<&Value>,
    group: &'static str,
    source: Option<&str>,
    out: &mut Vec<EvidenceItem>,
) {
    if let Some(items) = value.and_then(Value::as_array) {
        for item in items {
            push_item(item, group, source, out);
        }
    }
}

fn push_item(
    item: &Value,
    group: &'static str,
    fallback_source: Option<&str>,
    out: &mut Vec<EvidenceItem>,
) {
    let name = first_string(item, &["name", "title", "label"]).unwrap_or("Unnamed result");
    let id = first_string(item, &["id", "skill_id", "canonical_id"]).map(str::to_string);
    let url = first_string(
        item,
        &["homepage_url", "source_url", "url", "canonical_url"],
    )
    .map(str::to_string);
    let install = first_string(item, &["install_command", "installCommand"]).map(str::to_string);
    let description =
        first_string(item, &["description", "summary", "snippet"]).map(str::to_string);
    let source = first_string(item, &["provider", "source", "source_provider"])
        .or(fallback_source)
        .map(str::to_string);
    out.push(EvidenceItem {
        group,
        name: name.to_string(),
        id,
        url,
        install,
        description,
        source,
    });
}

fn first_string<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::builtin::BUILTIN_MODEL_ID;
    use crate::harness::context::{ContextSource, ContextSpec};
    use crate::harness::hooks::ToolLogEntry;
    use crate::harness::tokens::TokenizerKind;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    enum MockTurn {
        Response(Box<ProviderTurn>),
        Error(&'static str),
        Hang,
    }

    struct MockModel {
        preamble: String,
        definitions: Vec<ToolDefinition>,
        turns: Mutex<VecDeque<MockTurn>>,
        requests: Mutex<Vec<(Message, Vec<Message>)>>,
    }

    impl MockModel {
        fn new(definitions: Vec<ToolDefinition>, turns: Vec<MockTurn>) -> Self {
            Self {
                preamble: "mock preamble".into(),
                definitions,
                turns: Mutex::new(turns.into()),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn request_count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }

        fn requests(&self) -> Vec<(Message, Vec<Message>)> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl ModelDispatcher for MockModel {
        fn preamble(&self) -> &str {
            &self.preamble
        }

        fn tool_definitions(&self) -> &[ToolDefinition] {
            &self.definitions
        }

        async fn dispatch(
            &self,
            prompt: Message,
            history: Vec<Message>,
            cancel: &CancelHandle,
            _hook: Option<&TelemetryHook>,
        ) -> ProviderTurn {
            self.requests.lock().unwrap().push((prompt, history));
            let turn = self.turns.lock().unwrap().pop_front().unwrap();
            match turn {
                MockTurn::Response(turn) => *turn,
                MockTurn::Error(error) => failed_turn(error.to_string()),
                MockTurn::Hang => {
                    cancel.cancelled().await;
                    cancelled_turn()
                }
            }
        }
    }

    struct MockTools {
        result: ToolResult,
        calls: Mutex<Vec<(String, String)>>,
        hang: bool,
    }

    impl MockTools {
        fn new(result: ToolResult) -> Self {
            Self {
                result,
                calls: Mutex::new(Vec::new()),
                hang: false,
            }
        }
    }

    impl AppToolDispatcher for MockTools {
        async fn dispatch(&self, name: &str, args_json: &str) -> Result<ToolResult, String> {
            self.calls
                .lock()
                .unwrap()
                .push((name.to_string(), args_json.to_string()));
            if self.hang {
                futures_util::future::pending().await
            } else {
                Ok(self.result.clone())
            }
        }
    }

    struct TelemetryTools {
        calls: Mutex<Vec<(String, String)>>,
    }

    impl TelemetryTools {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl AppToolDispatcher for TelemetryTools {
        async fn dispatch(&self, name: &str, args_json: &str) -> Result<ToolResult, String> {
            self.calls
                .lock()
                .unwrap()
                .push((name.to_string(), args_json.to_string()));
            if name == "search_skills" {
                return Ok(ToolResult {
                    title: "Local skills".into(),
                    kind: "skill-search".into(),
                    snippet: "no matches".into(),
                    data: json!({"items": []}),
                    items_count: Some(0),
                    source: Some("local".into()),
                    elapsed_ms: 1,
                });
            }
            let items: Vec<Value> = (0..10)
                .map(|index| {
                    json!({
                        "id": format!("market-skill-{index}"),
                        "name": format!("Marketplace Skill {index}"),
                        "homepage_url": format!("https://example.invalid/skills/{index}"),
                        "install_command": format!("npx skills add market-skill-{index}"),
                        "description": format!("Candidate {index}"),
                        "provider": "skills.sh",
                        "opaque_private_payload": "PRIVATE_RAW_SENTINEL".repeat(300),
                    })
                })
                .collect();
            Ok(ToolResult {
                title: "Marketplace".into(),
                kind: "marketplace-search".into(),
                snippet: "10 candidates".into(),
                data: json!({"items": items}),
                items_count: Some(10),
                source: Some("skills.sh".into()),
                elapsed_ms: 2,
            })
        }
    }

    fn budget() -> TurnBudget {
        TurnBudget {
            context: ContextSpec {
                model_id: BUILTIN_MODEL_ID.into(),
                context_window_tokens: 8192,
                output_limit_tokens: None,
                tokenizer: TokenizerKind::CharsApproximation,
                source: ContextSource::BuiltIn,
            },
            reserved_completion_tokens: 512,
            safety_reserve_tokens: 256,
            prompt_budget_tokens: 7424,
            tool_result_budget_tokens: 4096,
            per_tool_result_max_tokens: 512,
            summary_max_tokens: 384,
            max_tool_steps: 6,
            max_same_tool_calls: 2,
        }
    }

    fn definition(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.into(),
            description: format!("{name} description"),
            parameters: json!({"type":"object"}),
        }
    }

    fn text_turn(text: &str, input: u32, output: u32) -> ProviderTurn {
        ProviderTurn {
            assistant: Message::assistant(text),
            text: text.into(),
            input_tokens: input,
            output_tokens: output,
            error: None,
            cancelled: false,
        }
    }

    fn response(turn: ProviderTurn) -> MockTurn {
        MockTurn::Response(Box::new(turn))
    }

    fn tool_turn(calls: Vec<ToolCall>) -> ProviderTurn {
        ProviderTurn {
            assistant: Message::Assistant {
                id: Some("assistant-message-id".into()),
                content: one_or_many(calls.into_iter().map(AssistantContent::ToolCall).collect()),
            },
            text: String::new(),
            input_tokens: 10,
            output_tokens: 2,
            error: None,
            cancelled: false,
        }
    }

    fn call(id: &str, call_id: Option<&str>, name: &str, args: Value) -> ToolCall {
        let mut call = ToolCall::new(
            id.into(),
            rig::message::ToolFunction::new(name.into(), args),
        );
        call.call_id = call_id.map(str::to_string);
        call
    }

    fn full_result() -> ToolResult {
        ToolResult {
            title: "market results".into(),
            kind: "marketplace-search".into(),
            snippet: "one result".into(),
            data: json!({
                "items": [{
                    "id": "market:one",
                    "name": "Actual Skill",
                    "description": "Verified description",
                    "homepage_url": "https://example.com/skill",
                    "install_command": "npx skills add owner/repo/skill -g",
                    "raw": "INJECTION_SENTINEL ".repeat(2_000)
                }]
            }),
            items_count: Some(1),
            source: Some("skills.sh".into()),
            elapsed_ms: 1,
        }
    }

    fn local_result() -> ToolResult {
        ToolResult {
            title: "local results".into(),
            kind: "local-skill-search".into(),
            snippet: "one local result".into(),
            data: json!({
                "skills": [{
                    "id": "local:verified",
                    "name": "Verified Local Skill",
                    "description": "Evidence from the local skill index"
                }]
            }),
            items_count: Some(1),
            source: Some("local-index".into()),
            elapsed_ms: 1,
        }
    }

    #[tokio::test]
    async fn oversized_raw_result_is_projected_and_ids_survive() {
        let tool = MockModel::new(
            vec![definition("search_marketplace")],
            vec![
                response(tool_turn(vec![
                    call(
                        "tool-id-1",
                        Some("provider-call-1"),
                        "search_marketplace",
                        json!({"query":"rust"}),
                    ),
                    call(
                        "tool-id-2",
                        Some("provider-call-2"),
                        "search_marketplace",
                        json!({"query":"go"}),
                    ),
                ])),
                response(text_turn("draft", 20, 4)),
            ],
        );
        let finalizer =
            MockModel::new(Vec::new(), vec![response(text_turn("Final answer", 30, 5))]);
        let tools = MockTools::new(full_result());
        let outcome = run_loop(LoopInput {
            tool_model: &tool,
            finalizer_model: &finalizer,
            tool_dispatcher: &tools,
            hook: None,
            cancel: &CancelHandle::new(),
            history: Vec::new(),
            initial_prompt: Message::user("find skills"),
            budget: budget(),
        })
        .await;

        assert_eq!(outcome.final_text, "Final answer");
        assert_eq!(outcome.in_tokens, 60);
        assert_eq!(outcome.out_tokens, 11);
        let requests = tool.requests();
        let (second_prompt, second_history) = &requests[1];
        let serialized = serde_json::to_string(&(second_history, second_prompt)).unwrap();
        assert!(!serialized.contains("INJECTION_SENTINEL"));
        assert!(serialized.len() < 8_192);
        assert!(serialized.contains("tool-id-1"));
        assert!(serialized.contains("provider-call-1"));
        assert!(serialized.contains("tool-id-2"));
        assert!(serialized.contains("provider-call-2"));
        let full = serde_json::to_string(&full_result()).unwrap();
        assert_ne!(serialized, full);
        assert!(full.contains("INJECTION_SENTINEL"));
        assert!(finalizer.tool_definitions().is_empty());
        assert_eq!(finalizer.request_count(), 1);
        let finalizer_requests = finalizer.requests();
        let finalizer_history = &finalizer_requests[0].1;
        assert_eq!(
            finalizer_history.len(),
            4,
            "user, assistant tool calls, correlated tool results, and draft assistant exactly once"
        );
        assert!(matches!(finalizer_history[0], Message::User { .. }));
        assert!(matches!(finalizer_history[1], Message::Assistant { .. }));
        assert!(matches!(finalizer_history[2], Message::User { .. }));
        assert!(matches!(finalizer_history[3], Message::Assistant { .. }));
    }

    #[tokio::test]
    async fn finalizer_failure_returns_deduplicated_evidence_answer() {
        let tool = MockModel::new(
            vec![definition("search_marketplace")],
            vec![
                response(tool_turn(vec![call(
                    "tool-id",
                    None,
                    "search_marketplace",
                    json!({"query":"rust"}),
                )])),
                response(text_turn("draft", 2, 1)),
            ],
        );
        let finalizer = MockModel::new(Vec::new(), vec![MockTurn::Error("HTTP 503 provider down")]);
        let tools = MockTools::new(full_result());
        let outcome = run_loop(LoopInput {
            tool_model: &tool,
            finalizer_model: &finalizer,
            tool_dispatcher: &tools,
            hook: None,
            cancel: &CancelHandle::new(),
            history: Vec::new(),
            initial_prompt: Message::user("find skills"),
            budget: budget(),
        })
        .await;
        assert!(outcome.final_text.contains("Actual Skill"));
        assert!(outcome.final_text.contains("market:one"));
        assert!(outcome.final_text.contains("https://example.com/skill"));
        assert!(outcome
            .final_text
            .contains("npx skills add owner/repo/skill -g"));
        assert!(!outcome.final_text.contains("step limit"));
        assert_eq!(
            outcome.stream_error.as_deref(),
            Some("HTTP 503 provider down")
        );
    }

    #[tokio::test]
    async fn finalizer_cannot_invent_zero_results_for_unqueried_marketplace() {
        let tool = MockModel::new(
            vec![definition("search_skills")],
            vec![
                response(tool_turn(vec![
                    call(
                        "local-tool-id-1",
                        None,
                        "search_skills",
                        json!({"query":"public speaking"}),
                    ),
                    call(
                        "local-tool-id-2",
                        None,
                        "search_skills",
                        json!({"query":"public speaking"}),
                    ),
                ])),
                response(text_turn("draft", 2, 1)),
            ],
        );
        let unsupported =
            "No public skills were found in the marketplace. Try a broader search next time.";
        let finalizer = MockModel::new(Vec::new(), vec![response(text_turn(unsupported, 3, 2))]);
        let tools = MockTools::new(local_result());
        let outcome = run_loop(LoopInput {
            tool_model: &tool,
            finalizer_model: &finalizer,
            tool_dispatcher: &tools,
            hook: None,
            cancel: &CancelHandle::new(),
            history: Vec::new(),
            initial_prompt: Message::user("find a public speaking skill"),
            budget: budget(),
        })
        .await;

        assert!(!outcome.final_text.contains(unsupported));
        assert!(outcome.final_text.contains("Verified Local Skill"));
        assert!(outcome.final_text.contains("local:verified"));
        assert!(outcome.final_text.contains("not supported by this turn"));
        assert_eq!(
            tool.request_count(),
            2,
            "the first duplicate must allow one corrective model continuation"
        );
        let calls = tools.calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "the duplicate local call must be reused");
        assert_eq!(calls[0].0, "search_skills");
        assert!(
            calls.iter().all(|(name, _)| name != "search_marketplace"),
            "the final answer must not imply an unobserved marketplace dispatch"
        );
    }

    #[tokio::test]
    async fn first_duplicate_reuse_can_continue_with_a_different_source() {
        let tool = MockModel::new(
            vec![
                definition("search_skills"),
                definition("search_marketplace"),
            ],
            vec![
                response(tool_turn(vec![
                    call("local-id-1", None, "search_skills", json!({"query":"rust"})),
                    call("local-id-2", None, "search_skills", json!({"query":"rust"})),
                ])),
                response(tool_turn(vec![call(
                    "market-id",
                    None,
                    "search_marketplace",
                    json!({"query":"rust"}),
                )])),
                response(text_turn("draft", 2, 1)),
            ],
        );
        let finalizer = MockModel::new(Vec::new(), vec![response(text_turn("Final answer", 1, 1))]);
        let tools = MockTools::new(full_result());
        let outcome = run_loop(LoopInput {
            tool_model: &tool,
            finalizer_model: &finalizer,
            tool_dispatcher: &tools,
            hook: None,
            cancel: &CancelHandle::new(),
            history: Vec::new(),
            initial_prompt: Message::user("search local and marketplace"),
            budget: budget(),
        })
        .await;

        assert_eq!(outcome.final_text, "Final answer");
        assert_eq!(tool.request_count(), 3);
        let requests = tool.requests();
        let corrective_prompt = serde_json::to_string(&requests[1].0).unwrap();
        assert!(corrective_prompt.contains("use a different source"));
        let calls = tools.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "search_skills");
        assert_eq!(calls[1].0, "search_marketplace");
    }

    #[tokio::test]
    async fn recurring_duplicate_reuse_forces_finalization() {
        let repeated = || {
            call(
                "repeated-id",
                None,
                "search_skills",
                json!({"query":"rust"}),
            )
        };
        let tool = MockModel::new(
            vec![definition("search_skills")],
            vec![
                response(tool_turn(vec![repeated(), repeated()])),
                response(tool_turn(vec![repeated()])),
            ],
        );
        let finalizer = MockModel::new(Vec::new(), vec![response(text_turn("Final answer", 1, 1))]);
        let tools = MockTools::new(local_result());
        let outcome = run_loop(LoopInput {
            tool_model: &tool,
            finalizer_model: &finalizer,
            tool_dispatcher: &tools,
            hook: None,
            cancel: &CancelHandle::new(),
            history: Vec::new(),
            initial_prompt: Message::user("find rust"),
            budget: budget(),
        })
        .await;

        assert_eq!(outcome.final_text, "Final answer");
        assert_eq!(tool.request_count(), 2);
        assert_eq!(finalizer.request_count(), 1);
        assert_eq!(tools.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn unsupported_positive_marketplace_and_web_claims_are_repaired() {
        let evidence = [EvidenceEntry::success(
            "search_skills".into(),
            local_result(),
        )];
        for candidate in [
            "The ClawHub registry carries a public skill called Imaginary Pro.",
            "The web search returned three matching skills.",
        ] {
            let grounded = grounded_final_text(candidate.to_string(), &evidence);
            assert_ne!(grounded, candidate);
            assert!(grounded.contains("Verified Local Skill"));
            assert!(grounded.contains("not supported by this turn"));
        }
    }

    #[test]
    fn future_marketplace_search_recommendation_is_preserved() {
        for candidate in [
            "Search the marketplace next.",
            "You could try ClawHub next.",
            "Consider the marketplace next.",
        ] {
            assert_eq!(grounded_final_text(candidate.to_string(), &[]), candidate);
        }

        let asserted = "You could try ClawHub, which has a skill called Imaginary Pro.";
        assert_ne!(grounded_final_text(asserted.to_string(), &[]), asserted);
    }

    #[test]
    fn policy_veto_does_not_satisfy_marketplace_coverage() {
        let vetoed = [EvidenceEntry::not_executed(
            "search_marketplace".into(),
            "Tool dispatch vetoed by context budget".into(),
        )];
        let candidate = "We searched the marketplace.";
        let grounded = grounded_final_text(candidate.to_string(), &vetoed);

        assert_ne!(grounded, candidate);
        assert!(grounded.contains("not supported by this turn"));
        assert!(!source_was_queried(EvidenceSource::Marketplace, &vetoed));
    }

    #[test]
    fn dispatcher_error_supports_only_a_failure_claim() {
        let failed = [EvidenceEntry::dispatch_error(
            "search_marketplace".into(),
            "network timeout".into(),
        )];
        let failure_claim = "The marketplace search failed with a network error.";
        assert_eq!(
            grounded_final_text(failure_claim.to_string(), &failed),
            failure_claim
        );
        assert!(source_was_queried(EvidenceSource::Marketplace, &failed));

        for unsupported in [
            "We searched the marketplace.",
            "The marketplace returned a skill called Imaginary Pro.",
            "No public skills were found in the marketplace.",
        ] {
            assert_ne!(
                grounded_final_text(unsupported.to_string(), &failed),
                unsupported
            );
        }
    }

    #[test]
    fn verified_marketplace_zero_result_claim_is_preserved() {
        let zero = ToolResult {
            title: "market results".into(),
            kind: "marketplace-search".into(),
            snippet: "no results".into(),
            data: json!({"items": [], "count": 0}),
            items_count: Some(0),
            source: Some("skills.sh".into()),
            elapsed_ms: 1,
        };
        let candidate = "No public skills were found in the marketplace.";
        let grounded = grounded_final_text(
            candidate.to_string(),
            &[EvidenceEntry::success("search_marketplace".into(), zero)],
        );
        assert_eq!(grounded, candidate);
    }

    #[tokio::test]
    async fn at_most_six_tool_dispatches_are_admitted() {
        let calls = (0..8)
            .map(|index| {
                call(
                    &format!("id-{index}"),
                    None,
                    &format!("tool-{index}"),
                    json!({}),
                )
            })
            .collect();
        let definitions = (0..8)
            .map(|index| definition(&format!("tool-{index}")))
            .collect();
        let tool = MockModel::new(definitions, vec![response(tool_turn(calls))]);
        let finalizer = MockModel::new(Vec::new(), vec![response(text_turn("final", 1, 1))]);
        let tools = MockTools::new(full_result());
        let outcome = run_loop(LoopInput {
            tool_model: &tool,
            finalizer_model: &finalizer,
            tool_dispatcher: &tools,
            hook: None,
            cancel: &CancelHandle::new(),
            history: Vec::new(),
            initial_prompt: Message::user("screenshot sequence"),
            budget: budget(),
        })
        .await;
        assert!(!outcome.final_text.is_empty());
        assert!(tools.calls.lock().unwrap().len() <= 6);
        for (prompt, history) in tool.requests() {
            let turn_budget = budget();
            let estimated = estimate_request_tokens(&tool, &history, &prompt, &turn_budget);
            assert!(estimated > 0);
            assert!(
                estimated
                    + turn_budget.reserved_completion_tokens
                    + turn_budget.safety_reserve_tokens
                    <= turn_budget.context.context_window_tokens
            );
        }
        for (prompt, history) in finalizer.requests() {
            let turn_budget = budget();
            let estimated = estimate_request_tokens(&finalizer, &history, &prompt, &turn_budget);
            assert!(estimated > 0);
            assert!(
                estimated
                    + turn_budget.reserved_completion_tokens
                    + turn_budget.safety_reserve_tokens
                    <= turn_budget.context.context_window_tokens
            );
        }
    }

    #[tokio::test]
    async fn cancellation_interrupts_provider_and_tool_dispatch() {
        let provider = Arc::new(MockModel::new(Vec::new(), vec![MockTurn::Hang]));
        let finalizer = MockModel::new(Vec::new(), Vec::new());
        let tools = MockTools::new(full_result());
        let cancel = Arc::new(CancelHandle::new());
        let canceller = cancel.clone();
        let provider_for_run = provider.clone();
        let run = tokio::spawn(async move {
            run_loop(LoopInput {
                tool_model: provider_for_run.as_ref(),
                finalizer_model: &finalizer,
                tool_dispatcher: &tools,
                hook: None,
                cancel: &cancel,
                history: Vec::new(),
                initial_prompt: Message::user("hello"),
                budget: budget(),
            })
            .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        canceller.cancel();
        let outcome = tokio::time::timeout(Duration::from_secs(1), run)
            .await
            .unwrap()
            .unwrap();
        assert!(outcome.final_text.contains("cancelled"));

        let tool_model = MockModel::new(
            vec![definition("search_skills")],
            vec![response(tool_turn(vec![call(
                "id",
                None,
                "search_skills",
                json!({"query":"rust"}),
            )]))],
        );
        let finalizer = MockModel::new(Vec::new(), Vec::new());
        let hanging_tools = Arc::new(MockTools {
            result: full_result(),
            calls: Mutex::new(Vec::new()),
            hang: true,
        });
        let cancel = Arc::new(CancelHandle::new());
        let canceller = cancel.clone();
        let tools_for_run = hanging_tools.clone();
        let run = tokio::spawn(async move {
            run_loop(LoopInput {
                tool_model: &tool_model,
                finalizer_model: &finalizer,
                tool_dispatcher: tools_for_run.as_ref(),
                hook: None,
                cancel: &cancel,
                history: Vec::new(),
                initial_prompt: Message::user("find"),
                budget: budget(),
            })
            .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        canceller.cancel();
        let outcome = tokio::time::timeout(Duration::from_secs(1), run)
            .await
            .unwrap()
            .unwrap();
        assert!(outcome.final_text.contains("cancel"));
    }

    #[tokio::test]
    async fn normal_builtin_text_does_not_use_finalizer() {
        let tool = MockModel::new(Vec::new(), vec![response(text_turn("Normal answer", 7, 3))]);
        let finalizer = MockModel::new(Vec::new(), Vec::new());
        let outcome = run_loop(LoopInput {
            tool_model: &tool,
            finalizer_model: &finalizer,
            tool_dispatcher: &MockTools::new(full_result()),
            hook: None,
            cancel: &CancelHandle::new(),
            history: Vec::new(),
            initial_prompt: Message::user("hello"),
            budget: budget(),
        })
        .await;
        assert_eq!(outcome.final_text, "Normal answer");
        assert_eq!(finalizer.request_count(), 0);
    }

    #[tokio::test]
    async fn context_overflow_is_not_mislabeled_as_step_limit() {
        let tool = MockModel::new(Vec::new(), Vec::new());
        let finalizer = MockModel::new(Vec::new(), Vec::new());
        let mut tiny = budget();
        tiny.prompt_budget_tokens = 1;
        let outcome = run_loop(LoopInput {
            tool_model: &tool,
            finalizer_model: &finalizer,
            tool_dispatcher: &MockTools::new(full_result()),
            hook: None,
            cancel: &CancelHandle::new(),
            history: Vec::new(),
            initial_prompt: Message::user("this cannot fit"),
            budget: tiny,
        })
        .await;
        assert!(outcome.final_text.contains("context budget"));
        assert!(!outcome.final_text.contains("step limit"));
        assert_eq!(tool.request_count(), 0);
    }

    #[tokio::test]
    async fn captured_8192_telemetry_shape_stays_bounded_for_all_provider_routes() {
        for route in ["builtin", "ollama", "azure"] {
            let calls = (0..7)
                .map(|index| {
                    call(
                        &format!("local-{index}"),
                        Some(&format!("local-provider-{index}")),
                        "search_skills",
                        json!({"query": format!("local query {index}")}),
                    )
                })
                .chain((0..3).map(|index| {
                    call(
                        &format!("market-{index}"),
                        Some(&format!("market-provider-{index}")),
                        "search_marketplace",
                        json!({"query": format!("market query {index}"), "limit": 10}),
                    )
                }))
                .collect();
            let tool = MockModel::new(
                vec![
                    definition("search_skills"),
                    definition("search_marketplace"),
                ],
                vec![response(tool_turn(calls))],
            );
            let finalizer = MockModel::new(
                Vec::new(),
                vec![response(text_turn(
                    "Marketplace Skill 0 (market-skill-0) is a grounded candidate.",
                    700,
                    18,
                ))],
            );
            let tools = TelemetryTools::new();
            let cancel = CancelHandle::new();
            let prompt = "P".repeat(2_859);
            assert_eq!(prompt.chars().count(), 2_859);
            let turn_budget = budget();

            let outcome = run_loop(LoopInput {
                tool_model: &tool,
                finalizer_model: &finalizer,
                tool_dispatcher: &tools,
                hook: None,
                cancel: &cancel,
                history: Vec::new(),
                initial_prompt: Message::user(prompt),
                budget: turn_budget.clone(),
            })
            .await;

            let dispatched = tools.calls.lock().unwrap().clone();
            assert!(
                dispatched.len() <= 6,
                "{route}: app dispatched {} tools",
                dispatched.len()
            );
            for name in ["search_skills", "search_marketplace"] {
                assert!(
                    dispatched.iter().filter(|(tool, _)| tool == name).count() <= 2,
                    "{route}: {name} exceeded the identical-tool ceiling"
                );
            }
            assert!(!outcome.final_text.trim().is_empty(), "{route}");
            assert!(
                outcome.final_text.contains("market-skill-0"),
                "{route}: IDs must survive synthesis"
            );

            for (request_prompt, history) in tool.requests() {
                let estimate =
                    estimate_request_tokens(&tool, &history, &request_prompt, &turn_budget);
                assert!(
                    estimate <= turn_budget.prompt_budget_tokens,
                    "{route}: tool request estimate {estimate} exceeded {}",
                    turn_budget.prompt_budget_tokens
                );
                let serialized = serde_json::to_string(&(history, request_prompt)).unwrap();
                assert!(
                    !serialized.contains("PRIVATE_RAW_SENTINEL"),
                    "{route}: full local payload crossed the model boundary"
                );
            }
            for (request_prompt, history) in finalizer.requests() {
                let estimate =
                    estimate_request_tokens(&finalizer, &history, &request_prompt, &turn_budget);
                assert!(
                    estimate <= turn_budget.prompt_budget_tokens,
                    "{route}: final request estimate {estimate} exceeded {}",
                    turn_budget.prompt_budget_tokens
                );
                let serialized = serde_json::to_string(&(history, request_prompt)).unwrap();
                assert!(!serialized.contains("PRIVATE_RAW_SENTINEL"), "{route}");
                assert!(serialized.contains("market-skill-0"), "{route}");
                assert!(serialized.contains("market-provider-0"), "{route}");
            }
        }
    }

    #[tokio::test]
    async fn provider_overflow_gets_one_changed_retry_and_keeps_typed_label() {
        const OVERFLOW: &str =
            "request exceeds the available context size: n_prompt_tokens=9299 n_ctx=8192";
        let tool = MockModel::new(
            Vec::new(),
            vec![
                MockTurn::Error(OVERFLOW),
                response(text_turn("Recovered after reduced retry.", 1_200, 8)),
                response(text_turn("This third response must never run.", 1, 1)),
            ],
        );
        let finalizer = MockModel::new(Vec::new(), Vec::new());
        let tools = MockTools::new(full_result());
        let cancel = CancelHandle::new();

        let outcome = run_loop(LoopInput {
            tool_model: &tool,
            finalizer_model: &finalizer,
            tool_dispatcher: &tools,
            hook: None,
            cancel: &cancel,
            history: vec![Message::user("old private transcript ".repeat(500))],
            initial_prompt: Message::user("current request"),
            budget: budget(),
        })
        .await;

        assert_eq!(tool.request_count(), 2, "exactly one retry is permitted");
        let requests = tool.requests();
        let first = serde_json::to_string(&requests[0]).unwrap();
        let second = serde_json::to_string(&requests[1]).unwrap();
        assert_ne!(first, second, "unchanged requests must never be resent");
        assert!(
            second.len() < first.len(),
            "retry must be materially reduced"
        );
        assert_eq!(outcome.final_text, "Recovered after reduced retry.");
        assert!(outcome.stream_error.is_none());

        let exhausted = MockModel::new(
            Vec::new(),
            vec![
                MockTurn::Error(OVERFLOW),
                MockTurn::Error(OVERFLOW),
                response(text_turn("must not run", 1, 1)),
            ],
        );
        let failed = run_loop(LoopInput {
            tool_model: &exhausted,
            finalizer_model: &finalizer,
            tool_dispatcher: &tools,
            hook: None,
            cancel: &cancel,
            history: vec![Message::user("old transcript ".repeat(500))],
            initial_prompt: Message::user("current request"),
            budget: budget(),
        })
        .await;
        assert_eq!(exhausted.request_count(), 2);
        let error = failed.stream_error.expect("overflow remains typed");
        assert!(parse_context_overflow(&error).is_some());
        assert!(!failed
            .final_text
            .to_ascii_lowercase()
            .contains("step limit"));
    }

    #[test]
    fn evidence_fallback_deduplicates_by_source_identity() {
        let result = full_result();
        let answer = evidence_answer(&[
            EvidenceEntry::success("search_marketplace".into(), result.clone()),
            EvidenceEntry::success("search_marketplace".into(), result),
        ]);
        assert_eq!(answer.matches("**Actual Skill**").count(), 1);
    }

    #[test]
    fn tool_log_entry_shape_retains_veto_marker() {
        let entry = ToolLogEntry {
            call_id: "call".into(),
            name: "tool".into(),
            args_json: "{}".into(),
            result: None,
            error: Some("veto".into()),
            vetoed: true,
        };
        assert!(entry.vetoed);
    }

    #[test]
    fn current_persisted_user_is_not_duplicated_in_builtin_history() {
        let history = vec![
            Message::user("prior"),
            Message::assistant("prior answer"),
            Message::user("current"),
        ];
        let prior = without_current_user(history, "current");
        assert_eq!(prior.len(), 2);
        assert!(matches!(prior.last(), Some(Message::Assistant { .. })));

        let untouched = without_current_user(vec![Message::assistant("answer")], "current");
        assert_eq!(untouched.len(), 1);
    }
}
