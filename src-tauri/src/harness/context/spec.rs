use crate::harness::builtin::{BUILTIN_CTX, BUILTIN_MODEL_ID};
use crate::harness::memory::compaction::FALLBACK_CONTEXT_TOKENS;
use crate::harness::model_directory::ModelProfile;
use crate::harness::tokens::{infer_tokenizer, TokenizerKind};
use serde::{Deserialize, Serialize};

/// Shared fallback used when neither the live provider nor the model directory
/// can prove a context window. The source is retained in `ContextSpec`, so this
/// assumption cannot be mistaken for authoritative provider metadata.
pub const CONSERVATIVE_FALLBACK_CONTEXT_TOKENS: u32 = FALLBACK_CONTEXT_TOKENS;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ContextSource {
    /// Context reported by the running provider/model process.
    Runtime,
    /// Context configured for Skill Deck's pinned built-in model sidecar.
    BuiltIn,
    /// Context supplied by the merged public model directory.
    ModelProfile { sources: Vec<String> },
    /// No authoritative context was available.
    ConservativeFallback {
        assumed_tokens: u32,
        profile_sources_checked: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ContextSpec {
    pub model_id: String,
    pub context_window_tokens: u32,
    pub output_limit_tokens: Option<u32>,
    pub tokenizer: TokenizerKind,
    pub source: ContextSource,
}

impl ContextSpec {
    /// Resolve the effective model context in descending order of authority:
    /// live runtime, pinned built-in launch configuration, model directory,
    /// then a labeled conservative fallback.
    pub fn resolve(
        model_id: impl Into<String>,
        runtime_context_tokens: Option<u32>,
        profile: Option<&ModelProfile>,
    ) -> Self {
        let model_id = model_id.into();
        let tokenizer = profile
            .map(|profile| profile.tokenizer_hint.clone())
            .unwrap_or_else(|| infer_tokenizer(None, &model_id));
        let output_limit_tokens = profile.and_then(|profile| profile.output_limit);

        let (context_window_tokens, source) =
            if let Some(runtime_tokens) = non_zero(runtime_context_tokens) {
                (runtime_tokens, ContextSource::Runtime)
            } else if model_id == BUILTIN_MODEL_ID {
                (BUILTIN_CTX, ContextSource::BuiltIn)
            } else if let Some(profile_tokens) =
                non_zero(profile.and_then(|profile| profile.context_window))
            {
                (
                    profile_tokens,
                    ContextSource::ModelProfile {
                        sources: profile
                            .map(|profile| profile.source.clone())
                            .unwrap_or_default(),
                    },
                )
            } else {
                (
                    CONSERVATIVE_FALLBACK_CONTEXT_TOKENS,
                    ContextSource::ConservativeFallback {
                        assumed_tokens: CONSERVATIVE_FALLBACK_CONTEXT_TOKENS,
                        profile_sources_checked: profile
                            .map(|profile| profile.source.clone())
                            .unwrap_or_default(),
                    },
                )
            };

        Self {
            model_id,
            context_window_tokens,
            output_limit_tokens,
            tokenizer,
            source,
        }
    }
}

fn non_zero(value: Option<u32>) -> Option<u32> {
    value.filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(context_window: Option<u32>) -> ModelProfile {
        ModelProfile {
            id: "ollama::test-model".into(),
            model_id: "test-model".into(),
            provider: "ollama".into(),
            name: "Test model".into(),
            family: Some("qwen".into()),
            context_window,
            output_limit: Some(1_024),
            supports_tools: true,
            structured_output: false,
            reasoning: false,
            attachment: false,
            modalities_in: vec!["text".into()],
            modalities_out: vec!["text".into()],
            pricing_input_per_million: None,
            pricing_output_per_million: None,
            tokenizer_hint: TokenizerKind::OllamaNative,
            source: vec!["models.dev".into()],
            release_date: None,
            last_updated: None,
        }
    }

    #[test]
    fn runtime_context_precedes_profile_metadata() {
        let profile = profile(Some(32_768));
        let spec = ContextSpec::resolve("test-model", Some(12_288), Some(&profile));

        assert_eq!(spec.context_window_tokens, 12_288);
        assert_eq!(spec.output_limit_tokens, Some(1_024));
        assert_eq!(spec.source, ContextSource::Runtime);
    }

    #[test]
    fn unknown_context_uses_labeled_fallback_and_keeps_profile_sources() {
        let profile = profile(None);
        let spec = ContextSpec::resolve("test-model", None, Some(&profile));

        assert_eq!(
            spec.context_window_tokens,
            CONSERVATIVE_FALLBACK_CONTEXT_TOKENS
        );
        assert_eq!(
            spec.source,
            ContextSource::ConservativeFallback {
                assumed_tokens: CONSERVATIVE_FALLBACK_CONTEXT_TOKENS,
                profile_sources_checked: vec!["models.dev".into()],
            }
        );
        assert_eq!(spec.tokenizer, TokenizerKind::OllamaNative);
    }
}
