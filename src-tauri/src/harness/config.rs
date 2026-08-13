// @agent-context: Harness settings + provider config types.
// Defaults live here (only file exempt from the anti-hardcode E8.1 sweep) so they
// can be sourced + tuned at runtime from %APPDATA%/skill-deck/config.json.

use serde::{Deserialize, Serialize};

/// User-tunable harness behavior. Every value here flows from Settings UI.
/// Debug is hand-written below so `web_search_api_key` can never leak into
/// logs / error chains via `{:?}`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HarnessSettings {
    /// Trigger compaction when est_tokens > floor * context_window. Default 0.70.
    pub memory_compaction_floor: f64,
    /// Keep transcript compaction in-process. When false, only the explicitly
    /// selected provider may summarize; an unselected provider is never used.
    pub local_compaction_only: bool,
    /// Tool results bigger than this get spilled to disk. Default 8000.
    pub tool_result_spill_threshold_chars: usize,
    /// Default tool `limit` when the user did not specify a count.
    pub item_count_default: usize,
    /// Max ReAct iterations per user turn.
    pub max_turns: u32,
    /// Max output tokens the model is allowed per turn.
    pub max_tokens: u32,
    /// Web search provider: "duckduckgo" (no key) or "brave" (key optional).
    pub web_search_provider: String,
    /// Optional Brave Search API key.
    pub web_search_api_key: Option<String>,
    /// Hours between model-directory refreshes from models.dev + ai-model-directory.
    pub models_cache_ttl_hours: u32,
    /// Default token cap on `web_fetch` output to keep retrieved pages from
    /// blowing the model's context. Tunable in Settings.
    pub web_fetch_default_max_tokens: u32,
    /// Completion space that is never surrendered to prompt/tool context.
    pub minimum_final_answer_tokens: u32,
    /// Fraction of the model context kept unused for token-count drift.
    pub context_safety_fraction: f64,
    /// Absolute lower bound for the context safety reserve.
    pub context_safety_min_tokens: u32,
    /// Fraction of the usable prompt budget available to retained tool results.
    pub tool_context_fraction: f64,
    /// Maximum accepted payload from one tool result.
    pub tool_result_max_tokens: u32,
    /// Maximum token budget for a conversation summary.
    pub summary_max_tokens: u32,
    /// Maximum tool dispatches admitted during one model turn.
    pub max_tool_steps: u32,
    /// Maximum consecutive dispatches of the same tool during one model turn.
    pub max_same_tool_calls: u32,
}

impl Default for HarnessSettings {
    fn default() -> Self {
        // These defaults are the single allowed source of bare numbers in the harness.
        Self {
            memory_compaction_floor: 0.70,
            local_compaction_only: true,
            tool_result_spill_threshold_chars: 8000,
            item_count_default: 3,
            max_turns: 12,
            max_tokens: 2048,
            web_search_provider: "duckduckgo".into(),
            web_search_api_key: None,
            models_cache_ttl_hours: 24,
            web_fetch_default_max_tokens: 4000,
            minimum_final_answer_tokens: 512,
            context_safety_fraction: 0.05,
            context_safety_min_tokens: 256,
            tool_context_fraction: 0.45,
            tool_result_max_tokens: 512,
            summary_max_tokens: 384,
            max_tool_steps: 6,
            max_same_tool_calls: 2,
        }
    }
}

/// Persisted provider entry. Same shape as the deleted ai::ProviderConfig but
/// scoped to the harness module so the old AI residue stays gone.
/// Debug is hand-written below: `api_key` renders as `Some("••••")`, so a
/// stray `{:?}` in a log line or error context can never echo a credential.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    /// "ollama" or "azure" for v1.
    pub kind: String,
    pub label: String,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub deployment: Option<String>,
    pub api_version: Option<String>,
    pub default_model: Option<String>,
    pub notes: Option<String>,
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("label", &self.label)
            .field("endpoint", &self.endpoint)
            .field("api_key", &self.api_key.as_ref().map(|_| "••••"))
            .field("deployment", &self.deployment)
            .field("api_version", &self.api_version)
            .field("default_model", &self.default_model)
            .field("notes", &self.notes)
            .finish()
    }
}

impl std::fmt::Debug for HarnessSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HarnessSettings")
            .field("memory_compaction_floor", &self.memory_compaction_floor)
            .field("local_compaction_only", &self.local_compaction_only)
            .field(
                "tool_result_spill_threshold_chars",
                &self.tool_result_spill_threshold_chars,
            )
            .field("item_count_default", &self.item_count_default)
            .field("max_turns", &self.max_turns)
            .field("max_tokens", &self.max_tokens)
            .field("web_search_provider", &self.web_search_provider)
            .field(
                "web_search_api_key",
                &self.web_search_api_key.as_ref().map(|_| "••••"),
            )
            .field("models_cache_ttl_hours", &self.models_cache_ttl_hours)
            .field(
                "web_fetch_default_max_tokens",
                &self.web_fetch_default_max_tokens,
            )
            .field(
                "minimum_final_answer_tokens",
                &self.minimum_final_answer_tokens,
            )
            .field("context_safety_fraction", &self.context_safety_fraction)
            .field("context_safety_min_tokens", &self.context_safety_min_tokens)
            .field("tool_context_fraction", &self.tool_context_fraction)
            .field("tool_result_max_tokens", &self.tool_result_max_tokens)
            .field("summary_max_tokens", &self.summary_max_tokens)
            .field("max_tool_steps", &self.max_tool_steps)
            .field("max_same_tool_calls", &self.max_same_tool_calls)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_config_debug_never_prints_api_key() {
        let cfg = ProviderConfig {
            id: "azure-1".into(),
            kind: "azure".into(),
            label: "Azure".into(),
            endpoint: Some("https://example.openai.azure.com".into()),
            api_key: Some("sk-super-secret-key-material-12345".into()),
            deployment: Some("gpt-4o".into()),
            api_version: None,
            default_model: None,
            notes: None,
        };
        let debug = format!("{cfg:?}");
        assert!(
            !debug.contains("super-secret"),
            "debug leaked the key: {debug}"
        );
        assert!(!debug.contains("12345"), "debug leaked key tail: {debug}");
        assert!(debug.contains("••••"), "debug should show the mask");
        assert!(debug.contains("azure-1"), "non-secret fields still visible");
    }

    #[test]
    fn harness_settings_debug_never_prints_search_key() {
        let settings = HarnessSettings {
            web_search_api_key: Some("brave-key-abcdef".into()),
            ..HarnessSettings::default()
        };
        let debug = format!("{settings:?}");
        assert!(!debug.contains("brave-key-abcdef"), "debug leaked: {debug}");
        assert!(debug.contains("••••"));
    }

    #[test]
    fn legacy_settings_json_receives_context_budget_defaults() {
        let legacy = r#"{
            "memoryCompactionFloor": 0.7,
            "toolResultSpillThresholdChars": 8000,
            "itemCountDefault": 3,
            "maxTurns": 12,
            "maxTokens": 2048,
            "webSearchProvider": "duckduckgo",
            "webSearchApiKey": null,
            "modelsCacheTtlHours": 24,
            "webFetchDefaultMaxTokens": 4000
        }"#;

        let settings: HarnessSettings =
            serde_json::from_str(legacy).expect("legacy settings should remain readable");

        assert_eq!(settings.minimum_final_answer_tokens, 512);
        assert!(settings.local_compaction_only);
        assert_eq!(settings.context_safety_fraction, 0.05);
        assert_eq!(settings.context_safety_min_tokens, 256);
        assert_eq!(settings.tool_context_fraction, 0.45);
        assert_eq!(settings.tool_result_max_tokens, 512);
        assert_eq!(settings.summary_max_tokens, 384);
        assert_eq!(settings.max_tool_steps, 6);
        assert_eq!(settings.max_same_tool_calls, 2);
    }
}
