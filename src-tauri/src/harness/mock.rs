// @agent-context: Mock provider surface for fast, network-free test cycles.
//
// The real providers speak SSE through rig; tests must not. This module
// builds the exact `Stream<Item = Result<MultiTurnStreamItem<R>, StreamingError>>`
// shape `agent::consume_stream` consumes, so the ReAct drain loop, error
// paths, and cancellation can be exercised hermetically:
//
// - `scripted_stream(...)`  — plays back a fixed sequence of items.
// - `hung_stream()`         — never yields; models a provider that stops
//                             responding mid-turn (the cancel test target).
// - `text_item` / `final_item` / `error_item` — item constructors.
// - `collecting_sink()`     — an `EventSink` that records every emitted
//                             `HarnessEvent` for assertions.
// - `canned_summarizer()`   — deterministic summarize_fn for compaction tests.

#![allow(dead_code)]

use crate::harness::events::HarnessEvent;
use crate::harness::hooks::EventSink;
use futures_util::stream::{self, Stream};
use rig::agent::{MultiTurnStreamItem, StreamingError};
use rig::completion::{CompletionError, Usage};
use rig::message::Text;
use rig::streaming::StreamedAssistantContent;
use std::sync::{Arc, Mutex};

/// The response type parameter is irrelevant for scripted streams; `()`
/// satisfies `consume_stream`'s `R: Clone + Unpin + Send + Sync + 'static`.
pub type MockStreamItem = Result<MultiTurnStreamItem<()>, StreamingError>;

/// A streamed assistant text chunk.
pub fn text_item(text: &str) -> MockStreamItem {
    Ok(MultiTurnStreamItem::StreamAssistantItem(
        StreamedAssistantContent::Text(Text { text: text.into() }),
    ))
}

/// The stream-terminal final response with usage numbers.
pub fn final_item(response: &str, input_tokens: u64, output_tokens: u64) -> MockStreamItem {
    let mut usage = Usage::new();
    usage.input_tokens = input_tokens;
    usage.output_tokens = output_tokens;
    usage.total_tokens = input_tokens + output_tokens;
    Ok(MultiTurnStreamItem::final_response(response, usage))
}

/// A hard stream error (what a provider 500 / MaxTurns overrun looks like).
pub fn error_item(message: &str) -> MockStreamItem {
    Err(StreamingError::Completion(CompletionError::ResponseError(
        message.to_string(),
    )))
}

/// Play back a fixed item sequence.
pub fn scripted_stream(items: Vec<MockStreamItem>) -> impl Stream<Item = MockStreamItem> + Unpin {
    stream::iter(items)
}

/// A provider that never yields another item — hangs forever.
pub fn hung_stream() -> impl Stream<Item = MockStreamItem> + Unpin {
    stream::pending()
}

/// EventSink that appends every event to a shared vector for assertions.
pub fn collecting_sink() -> (EventSink, Arc<Mutex<Vec<HarnessEvent>>>) {
    let events: Arc<Mutex<Vec<HarnessEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_for_sink = events.clone();
    let sink: EventSink = Arc::new(move |event| {
        let mut guard = events_for_sink.lock().unwrap_or_else(|p| p.into_inner());
        guard.push(event);
    });
    (sink, events)
}

/// Deterministic summarize_fn for compaction tests: no model, no network.
pub fn canned_summarizer() -> impl Fn(
    String,
    String,
) -> std::pin::Pin<
    Box<dyn std::future::Future<Output = anyhow::Result<String>>>,
> {
    |prior: String, block: String| {
        Box::pin(async move {
            Ok(format!(
                "[canned summary: prior={} chars, block={} chars]",
                prior.chars().count(),
                block.chars().count()
            ))
        })
    }
}
