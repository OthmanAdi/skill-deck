// @agent-context: Harness error taxonomy for the IPC boundary.
//
// The chat frontend must never have to string-match raw provider errors to
// decide what UI to show. Every failure that crosses the IPC boundary carries
// a machine-readable `HarnessErrorKind` next to the raw message — the same
// error + errorKind pattern `commands/updates.rs` uses for update checks.
//
// Classification is message-based on purpose: errors reach us as flattened
// strings from three layers (reqwest transport, rig streaming, our own
// anyhow chains), and the substrings below are the stable fingerprints each
// layer emits. `classify_reqwest` adds a typed fast-path when we still hold
// the original reqwest error.

use serde::{Deserialize, Serialize};

/// Machine-readable failure category for harness turns.
/// Serialized camelCase, e.g. `providerUnreachable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HarnessErrorKind {
    /// Connection refused / DNS failure / TCP reset — endpoint not reachable.
    ProviderUnreachable,
    /// 401 / 403 / bad or missing API key.
    ProviderAuth,
    /// Request or read timed out (including HTTP 408).
    ProviderTimeout,
    /// 404 for a model / deployment that does not exist.
    ModelNotFound,
    /// HTTP 429 / provider-side quota exhausted.
    RateLimited,
    /// A tool call failed hard enough to end the turn.
    ToolFailed,
    /// The serialized prompt exceeded the provider's context window. The
    /// controlled loop may compact and retry this once with a changed request.
    ContextBudgetExceeded,
    /// The user cancelled the turn.
    Cancelled,
    /// A second `harness_chat_send` arrived while a turn was already running
    /// for the same session. The caller should retry after the active turn
    /// finishes (or cancel it first).
    Busy,
    /// Anything we could not classify. The raw message is the best signal.
    Internal,
}

/// The serializable error shape returned by harness IPC commands.
/// Tauri serializes command errors with serde, so the frontend receives
/// `{ "kind": "...", "message": "..." }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessError {
    pub kind: HarnessErrorKind,
    pub message: String,
}

impl HarnessError {
    pub fn new(kind: HarnessErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Classify a flattened error message into a kind, keeping the raw text.
    pub fn from_message(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            kind: classify_message(&message),
            message,
        }
    }
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.kind, self.message)
    }
}

impl std::error::Error for HarnessError {}

impl From<anyhow::Error> for HarnessError {
    fn from(err: anyhow::Error) -> Self {
        // `{:#}` flattens the whole context chain so classification sees the
        // root cause ("connection refused") and not just the outer context.
        Self::from_message(format!("{err:#}"))
    }
}

/// Message prefix `run_turn` uses when rejecting a concurrent send. Kept as a
/// constant so classification and construction cannot drift apart.
pub const BUSY_PREFIX: &str = "busy:";

/// Message `run_turn` uses when a turn was cancelled by the user.
pub const CANCELLED_MESSAGE: &str = "cancelled";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextOverflow {
    pub prompt_tokens: Option<u32>,
    pub context_tokens: Option<u32>,
}

/// Parse the context-overflow shapes emitted by llama.cpp, Ollama/OpenAI
/// compatible servers, and Azure. Numbers are optional because some providers
/// report only the semantic error.
pub fn parse_context_overflow(message: &str) -> Option<ContextOverflow> {
    let lower = message.to_ascii_lowercase();
    let has_named_pair = lower.contains("n_prompt_tokens") && lower.contains("n_ctx");
    let semantic = lower.contains("exceeds the available context size")
        || lower.contains("maximum context length")
        || lower.contains("context length exceeded")
        || lower.contains("context window")
            && (lower.contains("exceed") || lower.contains("too large"))
        || lower.contains("too many tokens") && lower.contains("context");
    if !has_named_pair && !semantic {
        return None;
    }
    Some(ContextOverflow {
        prompt_tokens: extract_named_u32(
            &lower,
            &["n_prompt_tokens", "prompt tokens", "requested"],
        ),
        context_tokens: extract_named_u32(
            &lower,
            &["n_ctx", "context size", "context length", "context window"],
        ),
    })
}

fn extract_named_u32(message: &str, labels: &[&str]) -> Option<u32> {
    labels.iter().find_map(|label| {
        let start = message.find(label)?.saturating_add(label.len());
        let tail = message.get(start..)?;
        let digits: String = tail
            .chars()
            .skip_while(|ch| !ch.is_ascii_digit())
            .take_while(|ch| ch.is_ascii_digit())
            .collect();
        (!digits.is_empty())
            .then(|| digits.parse::<u32>().ok())
            .flatten()
    })
}

/// Classify a flattened error string into a `HarnessErrorKind`.
///
/// Order matters: the earlier checks are the more specific fingerprints.
pub fn classify_message(message: &str) -> HarnessErrorKind {
    let m = message.to_ascii_lowercase();

    // Our own markers first — they are exact and cheap.
    if m.starts_with(BUSY_PREFIX) {
        return HarnessErrorKind::Busy;
    }
    if m.contains("cancelled by user") || m == CANCELLED_MESSAGE || m.starts_with("cancelled:") {
        return HarnessErrorKind::Cancelled;
    }
    if parse_context_overflow(message).is_some() {
        return HarnessErrorKind::ContextBudgetExceeded;
    }

    // Auth: HTTP status or the words every provider uses for key problems.
    if m.contains("401")
        || m.contains("403")
        || m.contains("unauthorized")
        || m.contains("forbidden")
        || m.contains("invalid api key")
        || m.contains("invalid api-key")
        || m.contains("access denied")
        || m.contains("authenticationerror")
        || m.contains("authentication failed")
    {
        return HarnessErrorKind::ProviderAuth;
    }

    // Rate limiting.
    if m.contains("429")
        || m.contains("rate limit")
        || m.contains("ratelimit")
        || m.contains("too many requests")
        || m.contains("quota exceeded")
        || m.contains("insufficient_quota")
    {
        return HarnessErrorKind::RateLimited;
    }

    // Model / deployment not found. Azure says "DeploymentNotFound", Ollama
    // says `model "x" not found, try pulling it first`, OpenAI-compat says
    // "model_not_found". A bare 404 on a chat-completions call means the
    // model/deployment path is wrong, so 404 defaults here too.
    if m.contains("deploymentnotfound")
        || m.contains("model_not_found")
        || (m.contains("model") && m.contains("not found"))
        || (m.contains("deployment") && m.contains("not found"))
        || m.contains("try pulling it first")
        || m.contains("404")
    {
        return HarnessErrorKind::ModelNotFound;
    }

    // Unreachable beats timeout: reqwest wraps connect-phase timeouts in
    // "error trying to connect: ... timed out", and the actionable fact for
    // the user is "the endpoint is unreachable", not "it was slow".
    if m.contains("connection refused")
        || m.contains("connection reset")
        || m.contains("dns error")
        || m.contains("failed to lookup")
        || m.contains("no such host")
        || m.contains("error trying to connect")
        || m.contains("tcp connect error")
        || m.contains("network unreachable")
        || m.contains("no route to host")
    {
        return HarnessErrorKind::ProviderUnreachable;
    }

    // Timeouts (read/total deadline, HTTP 408).
    if m.contains("timed out")
        || m.contains("timeout")
        || m.contains("408")
        || m.contains("deadline exceeded")
    {
        return HarnessErrorKind::ProviderTimeout;
    }

    // rig wraps tool dispatch failures as "ToolSetError: ..." and our own
    // ToolError Display starts with these prefixes.
    if m.contains("toolseterror")
        || m.contains("tool call failed")
        || m.contains("toolcallerror")
        || m.starts_with("execution failed:")
        || m.starts_with("invalid arguments:")
    {
        return HarnessErrorKind::ToolFailed;
    }

    HarnessErrorKind::Internal
}

/// Typed fast-path when the original `reqwest::Error` is still in hand.
pub fn classify_reqwest(err: &reqwest::Error) -> HarnessErrorKind {
    if err.is_connect() {
        return HarnessErrorKind::ProviderUnreachable;
    }
    if err.is_timeout() {
        return HarnessErrorKind::ProviderTimeout;
    }
    if let Some(status) = err.status() {
        return match status.as_u16() {
            401 | 403 => HarnessErrorKind::ProviderAuth,
            404 => HarnessErrorKind::ModelNotFound,
            408 => HarnessErrorKind::ProviderTimeout,
            429 => HarnessErrorKind::RateLimited,
            _ => HarnessErrorKind::Internal,
        };
    }
    // Fall back to the string fingerprints for everything else.
    classify_message(&err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_unreachable_variants() {
        for msg in [
            "error sending request: error trying to connect: tcp connect error: Connection refused (os error 111)",
            "dns error: failed to lookup address information",
            "HttpError: connection reset by peer",
            "network unreachable",
        ] {
            assert_eq!(
                classify_message(msg),
                HarnessErrorKind::ProviderUnreachable,
                "msg: {msg}"
            );
        }
    }

    #[test]
    fn classifies_auth_variants() {
        for msg in [
            "HTTP 401: Access denied due to invalid subscription key",
            "HTTP 403 Forbidden",
            "CompletionError: ProviderError: Unauthorized",
            "invalid api key provided",
        ] {
            assert_eq!(
                classify_message(msg),
                HarnessErrorKind::ProviderAuth,
                "msg: {msg}"
            );
        }
    }

    #[test]
    fn classifies_timeout_variants() {
        for msg in [
            "operation timed out",
            "HTTP 408 Request Timeout",
            "request timeout while reading response body",
            "deadline exceeded waiting for stream",
        ] {
            assert_eq!(
                classify_message(msg),
                HarnessErrorKind::ProviderTimeout,
                "msg: {msg}"
            );
        }
    }

    #[test]
    fn connect_phase_timeout_is_unreachable_not_timeout() {
        let msg = "error trying to connect: operation timed out";
        assert_eq!(classify_message(msg), HarnessErrorKind::ProviderUnreachable);
    }

    #[test]
    fn classifies_model_not_found_variants() {
        for msg in [
            "HTTP 404: DeploymentNotFound",
            "model \"hermes9:900b\" not found, try pulling it first",
            "The model `gpt-9` does not exist: model_not_found",
            "ollama HTTP 404 Not Found",
        ] {
            assert_eq!(
                classify_message(msg),
                HarnessErrorKind::ModelNotFound,
                "msg: {msg}"
            );
        }
    }

    #[test]
    fn classifies_rate_limited_variants() {
        for msg in [
            "HTTP 429 Too Many Requests",
            "Rate limit reached for gpt-4o",
            "insufficient_quota: You exceeded your current quota",
        ] {
            assert_eq!(
                classify_message(msg),
                HarnessErrorKind::RateLimited,
                "msg: {msg}"
            );
        }
    }

    #[test]
    fn classifies_cancel_busy_tool_internal() {
        assert_eq!(classify_message("cancelled"), HarnessErrorKind::Cancelled);
        assert_eq!(
            classify_message("cancelled by user"),
            HarnessErrorKind::Cancelled
        );
        assert_eq!(
            classify_message("busy: a turn is already running for session s1"),
            HarnessErrorKind::Busy
        );
        assert_eq!(
            classify_message("ToolSetError: tool dispatch blew up"),
            HarnessErrorKind::ToolFailed
        );
        assert_eq!(
            classify_message("something exotic happened"),
            HarnessErrorKind::Internal
        );
    }

    #[test]
    fn auth_wins_over_not_found_when_both_present() {
        // "401 Access denied ... resource not found"-style composites should
        // read as auth: fixing the key is the actionable step.
        assert_eq!(
            classify_message("HTTP 401: resource not found for this key"),
            HarnessErrorKind::ProviderAuth
        );
    }

    #[test]
    fn serializes_camel_case() {
        let err = HarnessError::new(HarnessErrorKind::ProviderUnreachable, "boom");
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["kind"], "providerUnreachable");
        assert_eq!(json["message"], "boom");
        for (kind, expected) in [
            (HarnessErrorKind::ProviderAuth, "providerAuth"),
            (HarnessErrorKind::ProviderTimeout, "providerTimeout"),
            (HarnessErrorKind::ModelNotFound, "modelNotFound"),
            (HarnessErrorKind::RateLimited, "rateLimited"),
            (HarnessErrorKind::ToolFailed, "toolFailed"),
            (
                HarnessErrorKind::ContextBudgetExceeded,
                "contextBudgetExceeded",
            ),
            (HarnessErrorKind::Cancelled, "cancelled"),
            (HarnessErrorKind::Busy, "busy"),
            (HarnessErrorKind::Internal, "internal"),
        ] {
            let v = serde_json::to_value(kind).expect("kind serialize");
            assert_eq!(v, expected);
        }
    }

    #[test]
    fn from_anyhow_flattens_context_chain() {
        let root = anyhow::anyhow!("connection refused (os error 10061)");
        let wrapped = root.context("provider call failed");
        let err: HarnessError = wrapped.into();
        assert_eq!(err.kind, HarnessErrorKind::ProviderUnreachable);
        assert!(err.message.contains("provider call failed"));
        assert!(err.message.contains("connection refused"));
    }

    #[test]
    fn parses_llama_context_telemetry_and_classifies_it() {
        let message = r#"request (9299 tokens) exceeds the available context size
            {"n_prompt_tokens":9299,"n_ctx":8192}"#;
        assert_eq!(
            parse_context_overflow(message),
            Some(ContextOverflow {
                prompt_tokens: Some(9299),
                context_tokens: Some(8192),
            })
        );
        assert_eq!(
            classify_message(message),
            HarnessErrorKind::ContextBudgetExceeded
        );
    }
}
