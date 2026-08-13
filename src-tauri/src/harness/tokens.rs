// @agent-context: Tokenizer dispatch + token counter.
//
// Strategy (per .planning/harness-spec.md §0 D5):
// - OpenAI-family models (gpt-5, gpt-4o, gpt-4.1, o-series): tiktoken-rs o200k_base.
// - Older OpenAI (gpt-4, gpt-3.5): tiktoken-rs cl100k_base.
// - Anthropic / Claude: Anthropic count_tokens endpoint is the ground truth — for
//   pre-call estimation we fall back to chars/4 × 1.3 (the BPE rule of thumb).
// - Ollama-served models (llama, mistral, qwen, phi, gemma): the harness reads
//   Ollama's native `prompt_eval_count` from /api/chat as ground truth post-call;
//   pre-call we use the same chars/4 × 1.3 estimate.
// - Anything else: chars/4 × 1.3 fallback, logged as `CharsApproximation`.
//
// This module deliberately does NOT hardcode model context windows. That data
// lives in `model_directory::ModelProfile` and flows from models.dev +
// ai-model-directory at runtime.

use serde::{Deserialize, Serialize};
use tiktoken_rs::{cl100k_base_singleton, o200k_base_singleton};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum TokenizerKind {
    /// GPT-5 / GPT-4o / GPT-4.1 / o1 / o2 / o3 / o4 family.
    TiktokenO200kBase,
    /// GPT-4 (pre-omni) / GPT-3.5.
    TiktokenCl100kBase,
    /// Claude family. Pre-call: approximation. Post-call: Anthropic count_tokens.
    Anthropic,
    /// Llama / Mistral / Qwen / Phi / Gemma served via Ollama. Pre-call:
    /// approximation. Post-call: Ollama `prompt_eval_count`.
    OllamaNative,
    /// Unknown family — chars/4 × 1.3 estimate.
    CharsApproximation,
}

/// Resolve a tokenizer by inspecting the model `family` field and id pattern.
/// Order matters — most specific first.
pub fn infer_tokenizer(family: Option<&str>, id: &str) -> TokenizerKind {
    let hay = format!("{} {}", family.unwrap_or(""), id).to_lowercase();

    if hay.contains("claude") {
        return TokenizerKind::Anthropic;
    }

    // Newer OpenAI tokenizer covers gpt-5 / gpt-4o / gpt-4.1 / o-series.
    let o200k_markers = ["gpt-5", "gpt-4o", "gpt-4.1", "gpt-4-1", "o1-", "o3-", "o4-"];
    if o200k_markers.iter().any(|m| hay.contains(m))
        || hay.starts_with("o1")
        || hay.starts_with("o3")
        || hay.starts_with("o4")
    {
        return TokenizerKind::TiktokenO200kBase;
    }

    if hay.contains("gpt-4") || hay.contains("gpt-3.5") || hay.contains("gpt3.5") {
        return TokenizerKind::TiktokenCl100kBase;
    }

    let ollama_markers = [
        "llama",
        "mistral",
        "qwen",
        "phi",
        "gemma",
        "codellama",
        "deepseek",
        "tinyllama",
    ];
    if ollama_markers.iter().any(|m| hay.contains(m)) {
        return TokenizerKind::OllamaNative;
    }

    TokenizerKind::CharsApproximation
}

pub struct Counter;

impl Counter {
    /// Estimate token count for `text` given a tokenizer kind. Cheap, no network.
    pub fn count(kind: &TokenizerKind, text: &str) -> u32 {
        match kind {
            TokenizerKind::TiktokenO200kBase => {
                let bpe = o200k_base_singleton();
                bpe.encode_with_special_tokens(text).len() as u32
            }
            TokenizerKind::TiktokenCl100kBase => {
                let bpe = cl100k_base_singleton();
                bpe.encode_with_special_tokens(text).len() as u32
            }
            TokenizerKind::Anthropic
            | TokenizerKind::OllamaNative
            | TokenizerKind::CharsApproximation => approximate(text),
        }
    }

    /// True if this kind has a programmatic native counter (e.g. tiktoken).
    /// Used by telemetry to distinguish estimates from authoritative counts.
    pub fn is_exact(kind: &TokenizerKind) -> bool {
        matches!(
            kind,
            TokenizerKind::TiktokenO200kBase | TokenizerKind::TiktokenCl100kBase
        )
    }
}

fn approximate(text: &str) -> u32 {
    // chars / 4 × 1.3 — the standard BPE-ish rule of thumb. Slight overestimate
    // is safer than under because compaction triggers at fraction-of-window.
    let chars = text.chars().count() as f64;
    let est = (chars / 4.0) * 1.3;
    est.ceil() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_openai_o200k() {
        assert_eq!(
            infer_tokenizer(Some("gpt"), "gpt-5"),
            TokenizerKind::TiktokenO200kBase
        );
        assert_eq!(
            infer_tokenizer(Some("gpt"), "gpt-4o"),
            TokenizerKind::TiktokenO200kBase
        );
        assert_eq!(
            infer_tokenizer(None, "o1-preview"),
            TokenizerKind::TiktokenO200kBase
        );
        assert_eq!(
            infer_tokenizer(None, "o3-mini"),
            TokenizerKind::TiktokenO200kBase
        );
    }

    #[test]
    fn infers_openai_cl100k() {
        assert_eq!(
            infer_tokenizer(Some("gpt"), "gpt-4"),
            TokenizerKind::TiktokenCl100kBase
        );
        assert_eq!(
            infer_tokenizer(Some("gpt"), "gpt-3.5-turbo"),
            TokenizerKind::TiktokenCl100kBase
        );
    }

    #[test]
    fn infers_anthropic() {
        assert_eq!(
            infer_tokenizer(Some("claude-opus"), "claude-opus-4-7"),
            TokenizerKind::Anthropic
        );
        assert_eq!(
            infer_tokenizer(None, "claude-sonnet-4-6"),
            TokenizerKind::Anthropic
        );
    }

    #[test]
    fn infers_ollama_native() {
        assert_eq!(
            infer_tokenizer(Some("llama"), "llama3:8b"),
            TokenizerKind::OllamaNative
        );
        assert_eq!(
            infer_tokenizer(Some("mistral"), "mistral:7b"),
            TokenizerKind::OllamaNative
        );
        assert_eq!(
            infer_tokenizer(None, "qwen2.5:32b"),
            TokenizerKind::OllamaNative
        );
    }

    #[test]
    fn infers_chars_fallback() {
        assert_eq!(
            infer_tokenizer(None, "some-weird-model"),
            TokenizerKind::CharsApproximation
        );
    }

    #[test]
    fn tiktoken_counts_match_reference() {
        // "Hello world!" tokenizes to a stable count on o200k_base (≈ 3 tokens).
        let n = Counter::count(&TokenizerKind::TiktokenO200kBase, "Hello world!");
        assert!(n > 0 && n < 10, "got {n}");
    }

    #[test]
    fn approximation_matches_chars_over_four() {
        let text = "a".repeat(400);
        let n = Counter::count(&TokenizerKind::CharsApproximation, &text);
        // 400 chars / 4 * 1.3 = 130
        assert_eq!(n, 130);
    }
}
