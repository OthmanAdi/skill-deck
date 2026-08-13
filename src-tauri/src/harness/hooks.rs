// @agent-context: TelemetryHook implements rig's PromptHook + captures every
// tool call + result into a shared log the agent loop drains into memory after
// the stream finishes. This is what makes tool cards survive past the streaming
// phase: the hook is the source of truth for what happened, the agent loop
// folds it into the persisted session.
//
// Token deltas are BATCHED (DeltaBatcher) before they become `contentDelta`
// events: emitting one Tauri event per token froze WebView2 on long outputs
// (thousands of IPC round-trips + DOM updates per answer). Deltas accumulate
// and flush on a ~40ms cadence or a size cap, whichever comes first, and any
// non-delta event (tool start/result, finish) flushes pending text FIRST so
// event ordering is preserved exactly. `flush_content()` provides the final
// end-of-stream flush.

use crate::harness::events::HarnessEvent;
use crate::harness::telemetry::TelemetryWriter;
use crate::harness::tools::ToolResult;
use rig::agent::{HookAction, PromptHook, ToolCallHookAction};
use rig::completion::CompletionModel;
use rig::message::Message;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub type EventSink = Arc<dyn Fn(HarnessEvent) + Send + Sync>;

/// Flush cadence for batched content deltas. ~40ms ≈ 25 UI updates/second —
/// smooth to read, far below WebView2's event-storm threshold.
const DELTA_FLUSH_INTERVAL: Duration = Duration::from_millis(40);
/// Size backstop so a model that emits huge chunks cannot buffer unbounded
/// text between flushes.
const DELTA_FLUSH_MAX_CHARS: usize = 2048;

/// Accumulates streamed text and decides when a batch is due.
/// Thread-safe and cheap to share (the hook is cloned into the rig stream).
pub struct DeltaBatcher {
    inner: Mutex<DeltaBuf>,
    interval: Duration,
    max_chars: usize,
}

struct DeltaBuf {
    buf: String,
    last_flush: Instant,
}

impl DeltaBatcher {
    pub fn new(interval: Duration, max_chars: usize) -> Self {
        Self {
            inner: Mutex::new(DeltaBuf {
                buf: String::new(),
                // Start "elapsed" so the very first token flushes immediately —
                // perceived latency beats batching for the opening token.
                last_flush: Instant::now() - interval,
            }),
            interval,
            max_chars,
        }
    }

    /// Append text. Returns `Some(batch)` when a flush is due (interval
    /// elapsed or size cap hit), `None` while still accumulating.
    pub fn push(&self, text: &str) -> Option<String> {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.buf.push_str(text);
        if guard.buf.len() >= self.max_chars || guard.last_flush.elapsed() >= self.interval {
            guard.last_flush = Instant::now();
            let out = std::mem::take(&mut guard.buf);
            if out.is_empty() {
                None
            } else {
                Some(out)
            }
        } else {
            None
        }
    }

    /// Drain whatever is buffered regardless of timing. Used before emitting
    /// any non-delta event (ordering) and at end of stream (completeness).
    pub fn take(&self) -> Option<String> {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.last_flush = Instant::now();
        if guard.buf.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut guard.buf))
        }
    }
}

impl Default for DeltaBatcher {
    fn default() -> Self {
        Self::new(DELTA_FLUSH_INTERVAL, DELTA_FLUSH_MAX_CHARS)
    }
}

/// One captured tool call + result. The agent loop drains the log after the
/// stream completes and pushes these into `ConversationMemory` so the persisted
/// session carries the full tool history.
#[derive(Debug, Clone)]
pub struct ToolLogEntry {
    pub call_id: String,
    pub name: String,
    pub args_json: String,
    pub result: Option<ToolResult>,
    pub error: Option<String>,
    pub vetoed: bool,
}

#[derive(Clone, Default)]
pub struct ToolLog {
    inner: Arc<Mutex<Vec<ToolLogEntry>>>,
}

impl ToolLog {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn drain(&self) -> Vec<ToolLogEntry> {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        std::mem::take(&mut *guard)
    }
    fn start(&self, call_id: String, name: String, args_json: String) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        guard.push(ToolLogEntry {
            call_id,
            name,
            args_json,
            result: None,
            error: None,
            vetoed: false,
        });
    }
    fn finish_ok(&self, call_id: &str, name: &str, result: ToolResult) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(e) = guard.iter_mut().rev().find(|e| {
            e.call_id == call_id || (e.name == name && e.result.is_none() && e.error.is_none())
        }) {
            e.result = Some(result);
        } else {
            guard.push(ToolLogEntry {
                call_id: call_id.to_string(),
                name: name.to_string(),
                args_json: String::new(),
                result: Some(result),
                error: None,
                vetoed: false,
            });
        }
    }
    fn finish_err(&self, call_id: &str, name: &str, err: String) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(e) = guard.iter_mut().rev().find(|e| {
            e.call_id == call_id || (e.name == name && e.result.is_none() && e.error.is_none())
        }) {
            e.error = Some(err);
        } else {
            guard.push(ToolLogEntry {
                call_id: call_id.to_string(),
                name: name.to_string(),
                args_json: String::new(),
                result: None,
                error: Some(err),
                vetoed: false,
            });
        }
    }

    fn finish_veto(&self, call_id: &str, name: &str, reason: String) {
        let mut guard = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = guard.iter_mut().rev().find(|entry| {
            entry.call_id == call_id
                || (entry.name == name && entry.result.is_none() && entry.error.is_none())
        }) {
            entry.error = Some(reason);
            entry.vetoed = true;
        } else {
            guard.push(ToolLogEntry {
                call_id: call_id.to_string(),
                name: name.to_string(),
                args_json: String::new(),
                result: None,
                error: Some(reason),
                vetoed: true,
            });
        }
    }
}

#[derive(Clone)]
pub struct TelemetryHook {
    pub session_id: String,
    pub model: String,
    pub telemetry: Arc<TelemetryWriter>,
    pub event_sink: Option<EventSink>,
    pub tool_log: ToolLog,
    delta: Arc<DeltaBatcher>,
}

impl TelemetryHook {
    pub fn new(session_id: String, model: String, telemetry: Arc<TelemetryWriter>) -> Self {
        Self {
            session_id,
            model,
            telemetry,
            event_sink: None,
            tool_log: ToolLog::new(),
            delta: Arc::new(DeltaBatcher::default()),
        }
    }
    pub fn with_sink(mut self, sink: EventSink) -> Self {
        self.event_sink = Some(sink);
        self
    }
    pub fn tool_log(&self) -> ToolLog {
        self.tool_log.clone()
    }

    pub(crate) fn trace_pre_model(&self, message_count: usize, est_tokens: u32) {
        self.emit(HarnessEvent::PreModel {
            session_id: self.session_id.clone(),
            model: self.model.clone(),
            message_count,
            est_tokens,
        });
    }

    pub(crate) fn trace_text_delta(&self, text: &str) {
        if let Some(batch) = self.delta.push(text) {
            self.emit_now(HarnessEvent::ContentDelta {
                session_id: self.session_id.clone(),
                text: batch,
            });
        }
    }

    pub(crate) fn trace_tool_start(&self, call_id: String, name: String, arguments_json: String) {
        self.tool_log
            .start(call_id.clone(), name.clone(), arguments_json.clone());
        self.emit(HarnessEvent::ToolCallStart {
            session_id: self.session_id.clone(),
            call_id,
            name,
            arguments_json,
        });
    }

    pub(crate) fn trace_tool_result(&self, call_id: String, name: String, result: ToolResult) {
        self.tool_log.finish_ok(&call_id, &name, result.clone());
        self.emit(HarnessEvent::ToolCallResult {
            session_id: self.session_id.clone(),
            call_id,
            name,
            result,
        });
    }

    pub(crate) fn trace_tool_error(&self, call_id: String, name: String, message: String) {
        self.tool_log.finish_err(&call_id, &name, message.clone());
        self.emit(HarnessEvent::ToolCallError {
            session_id: self.session_id.clone(),
            call_id,
            name,
            message,
        });
    }

    pub(crate) fn trace_tool_veto(&self, call_id: String, name: String, reason: String) {
        self.tool_log.finish_veto(&call_id, &name, reason.clone());
        self.emit(HarnessEvent::ToolCallVetoed {
            session_id: self.session_id.clone(),
            call_id,
            name,
            reason,
        });
    }

    /// Emit an event as-is, bypassing the batcher. Only `flush_content` and
    /// batched-delta emission may call this for `ContentDelta`.
    fn emit_now(&self, event: HarnessEvent) {
        let _ = self.telemetry.append(&event);
        if let Some(sink) = self.event_sink.as_ref() {
            sink(event);
        }
    }

    /// Flush any buffered content text as a single `ContentDelta`.
    /// Public so `run_turn` can force a final flush after the stream ends
    /// (covers error/cancel exits where rig's finish hook never fires).
    pub fn flush_content(&self) {
        if let Some(text) = self.delta.take() {
            self.emit_now(HarnessEvent::ContentDelta {
                session_id: self.session_id.clone(),
                text,
            });
        }
    }

    /// Emit a non-delta event, flushing buffered text first so the frontend
    /// observes deltas and tool events in true order.
    fn emit(&self, event: HarnessEvent) {
        self.flush_content();
        self.emit_now(event);
    }
}

impl<M: CompletionModel> PromptHook<M> for TelemetryHook {
    async fn on_completion_call(&self, _prompt: &Message, history: &[Message]) -> HookAction {
        self.trace_pre_model(history.len(), 0);
        HookAction::cont()
    }

    async fn on_completion_response(
        &self,
        _prompt: &Message,
        _response: &rig::completion::CompletionResponse<M::Response>,
    ) -> HookAction {
        HookAction::cont()
    }

    async fn on_tool_call(
        &self,
        tool_name: &str,
        tool_call_id: Option<String>,
        internal_call_id: &str,
        args: &str,
    ) -> ToolCallHookAction {
        let call_id = tool_call_id.unwrap_or_else(|| internal_call_id.to_string());
        self.trace_tool_start(call_id, tool_name.to_string(), args.to_string());
        ToolCallHookAction::cont()
    }

    async fn on_tool_result(
        &self,
        tool_name: &str,
        tool_call_id: Option<String>,
        internal_call_id: &str,
        _args: &str,
        result: &str,
    ) -> HookAction {
        let call_id = tool_call_id.unwrap_or_else(|| internal_call_id.to_string());
        // The result is the tool's serde-serialized return value. For our
        // tools that is a ToolResult envelope JSON. For raw / errored cases
        // serde_json::from_str returns Err and we ship a synthetic envelope.
        match serde_json::from_str::<ToolResult>(result) {
            Ok(tr) => {
                self.trace_tool_result(call_id, tool_name.to_string(), tr);
            }
            Err(_) => {
                // The result string isn't our envelope — could be an upstream
                // error message from rig (e.g. tool dispatch failed before our
                // code ran). Treat as a tool error.
                let err = if result.is_empty() {
                    "tool returned no payload".to_string()
                } else {
                    result.chars().take(400).collect::<String>()
                };
                self.trace_tool_error(call_id, tool_name.to_string(), err);
            }
        }
        HookAction::cont()
    }

    async fn on_text_delta(&self, text_delta: &str, _aggregated_text: &str) -> HookAction {
        self.trace_text_delta(text_delta);
        HookAction::cont()
    }

    async fn on_tool_call_delta(
        &self,
        _tool_call_id: &str,
        _internal_call_id: &str,
        _tool_name: Option<&str>,
        _tool_call_delta: &str,
    ) -> HookAction {
        HookAction::cont()
    }

    async fn on_stream_completion_response_finish(
        &self,
        _prompt: &Message,
        _response: &M::StreamingResponse,
    ) -> HookAction {
        self.flush_content();
        HookAction::cont()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batcher_flushes_first_token_immediately() {
        let b = DeltaBatcher::new(Duration::from_millis(30), 2048);
        // last_flush starts "already elapsed", so the first push flushes.
        assert_eq!(b.push("Hello").as_deref(), Some("Hello"));
    }

    #[test]
    fn batcher_accumulates_within_interval_and_flushes_on_take() {
        let b = DeltaBatcher::new(Duration::from_secs(60), 2048);
        assert!(b.push("a").is_some(), "first push flushes");
        assert!(b.push("b").is_none());
        assert!(b.push("c").is_none());
        assert_eq!(b.take().as_deref(), Some("bc"));
        assert!(b.take().is_none(), "second take is empty");
    }

    #[test]
    fn batcher_flushes_on_size_cap_before_interval() {
        let b = DeltaBatcher::new(Duration::from_secs(60), 8);
        assert!(b.push("x").is_some(), "first push flushes");
        assert!(b.push("1234").is_none());
        // 4 + 5 = 9 chars ≥ 8 cap → flush both together, in order.
        assert_eq!(b.push("56789").as_deref(), Some("123456789"));
    }

    #[test]
    fn batcher_flushes_after_interval_elapses() {
        let b = DeltaBatcher::new(Duration::from_millis(20), 2048);
        assert!(b.push("t0").is_some());
        assert!(b.push("t1").is_none());
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(b.push("t2").as_deref(), Some("t1t2"));
    }

    #[test]
    fn batcher_preserves_order_and_loses_nothing() {
        let b = DeltaBatcher::new(Duration::from_millis(5), 32);
        let tokens: Vec<String> = (0..200).map(|i| format!("tok{i} ")).collect();
        let mut out = String::new();
        for t in &tokens {
            if let Some(batch) = b.push(t) {
                out.push_str(&batch);
            }
        }
        if let Some(rest) = b.take() {
            out.push_str(&rest);
        }
        let expected: String = tokens.concat();
        assert_eq!(out, expected, "batched output must equal raw token stream");
    }
}
