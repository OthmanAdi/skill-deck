// @agent-context: Model directory — fetch + merge + cache from two upstream sources.
//
// Sources (per .planning/harness-spec.md §0 D6):
//   1. https://models.dev/api.json
//   2. https://raw.githubusercontent.com/The-Best-Codes/ai-model-directory/main/data/all.min.json
//
// Both are public JSON, no auth. We pull both in parallel, merge by canonical id
// (provider+model), prefer models.dev fields where both have data, take the
// extras from ai-model-directory. Cached under
//   {config_dir}/skill-deck/models-cache.json
// with a TTL from `HarnessSettings::models_cache_ttl_hours`.
//
// Anti-hardcode rule: this module owns NO context-window or pricing constants.
// Every numeric field that ends up in a `ModelProfile` came from the upstream JSON.

use crate::harness::tokens::{infer_tokenizer, TokenizerKind};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// Stable identifier convention: `{provider}::{model_id}` so two providers can
/// expose the same model name without colliding (e.g. `openai::gpt-4o` vs
/// `azure::gpt-4o`).
pub fn canonical_id(provider: &str, model_id: &str) -> String {
    format!("{provider}::{model_id}")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelProfile {
    /// Canonical id: `{provider}::{model_id}`.
    pub id: String,
    /// Underlying model id (without provider prefix), e.g. "gpt-4o".
    pub model_id: String,
    /// Provider id, e.g. "openai", "anthropic", "ollama".
    pub provider: String,
    /// Display label.
    pub name: String,
    /// Family hint for tokenizer inference, e.g. "gpt", "claude-opus", "llama".
    pub family: Option<String>,
    /// Context window in tokens, when known.
    pub context_window: Option<u32>,
    /// Max output tokens, when known.
    pub output_limit: Option<u32>,
    pub supports_tools: bool,
    pub structured_output: bool,
    pub reasoning: bool,
    pub attachment: bool,
    pub modalities_in: Vec<String>,
    pub modalities_out: Vec<String>,
    /// USD per 1M input tokens.
    pub pricing_input_per_million: Option<f64>,
    /// USD per 1M output tokens.
    pub pricing_output_per_million: Option<f64>,
    /// Resolved tokenizer for counting tokens locally.
    pub tokenizer_hint: TokenizerKind,
    /// Which upstream sources contributed to this profile.
    pub source: Vec<String>,
    pub release_date: Option<String>,
    pub last_updated: Option<String>,
}

const MODELS_DEV_URL: &str = "https://models.dev/api.json";
const AI_MODEL_DIRECTORY_URL: &str =
    "https://raw.githubusercontent.com/The-Best-Codes/ai-model-directory/main/data/all.min.json";

const SOURCE_MODELS_DEV: &str = "models.dev";
const SOURCE_AI_MODEL_DIRECTORY: &str = "ai-model-directory";

const HTTP_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheFile {
    fetched_at_unix: u64,
    profiles: Vec<ModelProfile>,
}

pub fn cache_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("no config dir available")?;
    Ok(base.join("skill-deck").join("models-cache.json"))
}

fn now_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_cache(path: &PathBuf) -> Result<CacheFile> {
    let raw = fs::read_to_string(path).context("cache read")?;
    let parsed: CacheFile = serde_json::from_str(&raw).context("cache parse")?;
    Ok(parsed)
}

fn write_cache(path: &PathBuf, profiles: &[ModelProfile]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("create cache dir")?;
    }
    let payload = CacheFile {
        fetched_at_unix: now_secs(),
        profiles: profiles.to_vec(),
    };
    let json = serde_json::to_string_pretty(&payload).context("cache serialize")?;
    fs::write(path, json).context("cache write")?;
    Ok(())
}

/// Load profiles from cache if fresh, else refetch + merge + cache.
/// `ttl_hours` and `refresh` come from `HarnessSettings` + the CLI flag.
pub async fn load_or_fetch(ttl_hours: u32, refresh: bool) -> Result<Vec<ModelProfile>> {
    let path = cache_path()?;
    if !refresh {
        if let Ok(cache) = read_cache(&path) {
            let age = now_secs().saturating_sub(cache.fetched_at_unix);
            let age_hours = age / 3600;
            if (age_hours as u32) < ttl_hours {
                return Ok(cache.profiles);
            }
        }
    }
    let profiles = fetch_and_merge().await?;
    let _ = write_cache(&path, &profiles); // cache write failure non-fatal
    Ok(profiles)
}

async fn fetch_and_merge() -> Result<Vec<ModelProfile>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .user_agent("skill-deck-harness/0.1")
        .build()
        .context("reqwest client")?;

    let (md_res, am_res) =
        tokio::join!(fetch_models_dev(&client), fetch_ai_model_directory(&client),);

    let mut merged: HashMap<String, ModelProfile> = HashMap::new();
    let md = md_res.unwrap_or_else(|e| {
        log::warn!(target: "harness::model_directory", "models.dev fetch failed: {e:#}");
        Vec::new()
    });
    for p in md {
        merged.insert(p.id.clone(), p);
    }
    let am = am_res.unwrap_or_else(|e| {
        log::warn!(target: "harness::model_directory", "ai-model-directory fetch failed: {e:#}");
        Vec::new()
    });
    for p in am {
        merged
            .entry(p.id.clone())
            .and_modify(|existing| existing.absorb(&p))
            .or_insert(p);
    }
    let mut out: Vec<ModelProfile> = merged.into_values().collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

impl ModelProfile {
    /// Merge data from another profile (typically from a second upstream).
    /// `self` is the preferred source; only fill in unset / empty fields.
    fn absorb(&mut self, other: &ModelProfile) {
        if self.context_window.is_none() {
            self.context_window = other.context_window;
        }
        if self.output_limit.is_none() {
            self.output_limit = other.output_limit;
        }
        if !self.supports_tools && other.supports_tools {
            self.supports_tools = true;
        }
        if !self.structured_output && other.structured_output {
            self.structured_output = true;
        }
        if !self.reasoning && other.reasoning {
            self.reasoning = true;
        }
        if !self.attachment && other.attachment {
            self.attachment = true;
        }
        if self.family.is_none() {
            self.family = other.family.clone();
        }
        if self.pricing_input_per_million.is_none() {
            self.pricing_input_per_million = other.pricing_input_per_million;
        }
        if self.pricing_output_per_million.is_none() {
            self.pricing_output_per_million = other.pricing_output_per_million;
        }
        if self.modalities_in.is_empty() {
            self.modalities_in = other.modalities_in.clone();
        }
        if self.modalities_out.is_empty() {
            self.modalities_out = other.modalities_out.clone();
        }
        if self.release_date.is_none() {
            self.release_date = other.release_date.clone();
        }
        if self.last_updated.is_none() {
            self.last_updated = other.last_updated.clone();
        }
        for src in &other.source {
            if !self.source.contains(src) {
                self.source.push(src.clone());
            }
        }
        // Re-infer tokenizer if our self resolution was the fallback.
        if matches!(self.tokenizer_hint, TokenizerKind::CharsApproximation) {
            self.tokenizer_hint = infer_tokenizer(self.family.as_deref(), &self.model_id);
        }
    }
}

async fn fetch_models_dev(client: &reqwest::Client) -> Result<Vec<ModelProfile>> {
    let res = client
        .get(MODELS_DEV_URL)
        .send()
        .await
        .context("get models.dev")?;
    if !res.status().is_success() {
        anyhow::bail!("models.dev HTTP {}", res.status());
    }
    let root: Value = res.json().await.context("models.dev json")?;
    let mut out = Vec::new();
    // Schema: { provider_id: { id, name, models: { model_id: { ... } } } }
    let providers = match root.as_object() {
        Some(o) => o,
        None => return Ok(out),
    };
    for (provider_id, provider_node) in providers {
        let models = match provider_node.get("models").and_then(|m| m.as_object()) {
            Some(m) => m,
            None => continue,
        };
        for (model_id, m) in models {
            out.push(parse_models_dev_entry(provider_id, model_id, m));
        }
    }
    Ok(out)
}

fn parse_models_dev_entry(provider_id: &str, model_id: &str, m: &Value) -> ModelProfile {
    let name = m
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(model_id)
        .to_string();
    let family = m
        .get("family")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let limit = m.get("limit");
    let context_window = limit
        .and_then(|l| l.get("context"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let output_limit = limit
        .and_then(|l| l.get("output"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let supports_tools = m
        .get("tool_call")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let structured_output = m
        .get("structured_output")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let reasoning = m
        .get("reasoning")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let attachment = m
        .get("attachment")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let cost = m.get("cost");
    let pricing_input_per_million = cost.and_then(|c| c.get("input")).and_then(|v| v.as_f64());
    let pricing_output_per_million = cost.and_then(|c| c.get("output")).and_then(|v| v.as_f64());
    let modalities_in = m
        .pointer("/modalities/input")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let modalities_out = m
        .pointer("/modalities/output")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let release_date = m
        .get("release_date")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let last_updated = m
        .get("last_updated")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let tokenizer_hint = infer_tokenizer(family.as_deref(), model_id);
    ModelProfile {
        id: canonical_id(provider_id, model_id),
        model_id: model_id.to_string(),
        provider: provider_id.to_string(),
        name,
        family,
        context_window,
        output_limit,
        supports_tools,
        structured_output,
        reasoning,
        attachment,
        modalities_in,
        modalities_out,
        pricing_input_per_million,
        pricing_output_per_million,
        tokenizer_hint,
        source: vec![SOURCE_MODELS_DEV.to_string()],
        release_date,
        last_updated,
    }
}

async fn fetch_ai_model_directory(client: &reqwest::Client) -> Result<Vec<ModelProfile>> {
    let res = client
        .get(AI_MODEL_DIRECTORY_URL)
        .send()
        .await
        .context("get ai-model-directory")?;
    if !res.status().is_success() {
        anyhow::bail!("ai-model-directory HTTP {}", res.status());
    }
    let root: Value = res.json().await.context("ai-model-directory json")?;
    let mut out = Vec::new();
    // Schema: { provider_id: { id, name, models: { model_id: { ... } } } }
    let providers = match root.as_object() {
        Some(o) => o,
        None => return Ok(out),
    };
    for (provider_id, provider_node) in providers {
        let models = match provider_node.get("models").and_then(|m| m.as_object()) {
            Some(m) => m,
            None => continue,
        };
        for (model_id, m) in models {
            out.push(parse_ai_model_directory_entry(provider_id, model_id, m));
        }
    }
    Ok(out)
}

fn parse_ai_model_directory_entry(provider_id: &str, model_id: &str, m: &Value) -> ModelProfile {
    let name = m
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(model_id)
        .to_string();
    // ai-model-directory does not surface a `family` field; we infer from id.
    let family = None::<String>;
    let limit = m.get("limit");
    let context_window = limit
        .and_then(|l| l.get("context"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let output_limit = limit
        .and_then(|l| l.get("output"))
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let features = m.get("features");
    let supports_tools = features
        .and_then(|f| f.get("tool_call"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let structured_output = features
        .and_then(|f| f.get("structured_output"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let reasoning = features
        .and_then(|f| f.get("reasoning"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let attachment = features
        .and_then(|f| f.get("attachment"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let pricing = m.get("pricing");
    let pricing_input_per_million = pricing
        .and_then(|p| p.get("input"))
        .and_then(|v| v.as_f64());
    let pricing_output_per_million = pricing
        .and_then(|p| p.get("output"))
        .and_then(|v| v.as_f64());
    let modalities_in = m
        .pointer("/modalities/input")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let modalities_out = m
        .pointer("/modalities/output")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let release_date = m
        .get("release_date")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let last_updated = m
        .get("last_updated")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let tokenizer_hint = infer_tokenizer(family.as_deref(), model_id);
    ModelProfile {
        id: canonical_id(provider_id, model_id),
        model_id: model_id.to_string(),
        provider: provider_id.to_string(),
        name,
        family,
        context_window,
        output_limit,
        supports_tools,
        structured_output,
        reasoning,
        attachment,
        modalities_in,
        modalities_out,
        pricing_input_per_million,
        pricing_output_per_million,
        tokenizer_hint,
        source: vec![SOURCE_AI_MODEL_DIRECTORY.to_string()],
        release_date,
        last_updated,
    }
}

/// Try to resolve a model profile by canonical id, or by raw model id (first match).
pub fn find<'a>(profiles: &'a [ModelProfile], lookup: &str) -> Option<&'a ModelProfile> {
    if let Some(p) = profiles.iter().find(|p| p.id == lookup) {
        return Some(p);
    }
    profiles.iter().find(|p| p.model_id == lookup)
}
