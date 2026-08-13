// @agent-context: rig-backed provider resolver. Three kinds: Ollama + Azure OpenAI + Builtin.
//
// Strategy:
// - `resolve(ProviderConfig) -> ProviderClient` returns an enum that wraps rig's
//   concrete provider clients. Each variant exposes the same surface (health,
//   list_models, agent factory) so callers don't care which one is underneath.
// - Health probe is provider-specific. For Ollama we hit `/api/tags` (cheap and
//   doesn't burn tokens). For Azure we ask the configured deployment to echo a
//   single-token reply; if that fails we surface the upstream error.
// - `list_models` for Ollama queries Ollama's `/api/tags` directly via reqwest
//   (the old detect_local pattern). For Azure we return the configured
//   deployment as the only "model" since Azure exposes deployments not models.
// - Builtin is the bundled llama-server sidecar (harness/builtin.rs). It rides
//   rig 0.35's openai CompletionsClient (Chat Completions, `/v1/chat/completions`)
//   pointed at the runtime localhost endpoint — NEVER the default Responses-API
//   `openai::Client`, which llama-server 404s.

use crate::harness::config::ProviderConfig;
use anyhow::{Context, Result};
use rig::client::Nothing;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const HTTP_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub ok: bool,
    /// Empty when ok. Either a human error message or "skipped".
    pub status: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub context_window: Option<u32>,
    pub supports_tools: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedOllama {
    pub endpoint: String,
    pub models: Vec<ModelInfo>,
}

/// Wraps the concrete rig provider clients. We keep a thin enum rather than
/// boxing a trait object because the two providers behave differently enough
/// (Azure needs a deployment param, Ollama doesn't) that an enum is honest.
pub enum ProviderClient {
    Ollama {
        config: ProviderConfig,
        rig: rig::providers::ollama::Client,
    },
    Azure {
        config: ProviderConfig,
        rig: rig::providers::azure::Client,
    },
    Builtin {
        config: ProviderConfig,
        rig: rig::providers::openai::CompletionsClient,
    },
}

impl ProviderClient {
    pub fn id(&self) -> &str {
        match self {
            ProviderClient::Ollama { config, .. } => &config.id,
            ProviderClient::Azure { config, .. } => &config.id,
            ProviderClient::Builtin { config, .. } => &config.id,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            ProviderClient::Ollama { .. } => "ollama",
            ProviderClient::Azure { .. } => "azure",
            ProviderClient::Builtin { .. } => "builtin",
        }
    }

    pub async fn health(&self) -> HealthReport {
        match self {
            ProviderClient::Ollama { config, .. } => ollama_health(config).await,
            ProviderClient::Azure { config, .. } => azure_health(config).await,
            ProviderClient::Builtin { config, .. } => builtin_health(config).await,
        }
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        match self {
            ProviderClient::Ollama { config, .. } => list_ollama_models(config).await,
            ProviderClient::Azure { config, .. } => Ok(list_azure_deployment(config)),
            ProviderClient::Builtin { .. } => Ok(vec![builtin_model_info()]),
        }
    }
}

/// Build a `ProviderClient` from a persisted `ProviderConfig`.
/// Errors only if the rig client constructor itself fails — usually only
/// happens if required fields (api_key / endpoint) are missing for Azure.
pub fn resolve(config: ProviderConfig) -> Result<ProviderClient> {
    match config.kind.as_str() {
        "ollama" => {
            let base = config
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
            // rig's typestate builder requires the api_key marker even when
            // the provider does not authenticate. `Nothing` is rig's unit
            // struct for that case.
            let rig = rig::providers::ollama::Client::builder()
                .api_key(Nothing)
                .base_url(base)
                .build()
                .context("ollama client build")?;
            Ok(ProviderClient::Ollama { config, rig })
        }
        "azure" => {
            let endpoint = config
                .endpoint
                .clone()
                .context("azure provider missing endpoint")?;
            let api_key = config
                .api_key
                .clone()
                .context("azure provider missing api_key")?;
            let builder = rig::providers::azure::Client::builder()
                .api_key(api_key)
                .azure_endpoint(endpoint);
            // rig's azure builder hides api_version under defaults — if the user
            // overrides it via config, we apply it. The same goes for the
            // deployment id, which is treated as the model name in rig.
            let rig = builder.build().context("azure client build")?;
            Ok(ProviderClient::Azure { config, rig })
        }
        "builtin" => {
            // The endpoint is the runtime localhost URL the sidecar start
            // wrote (http://127.0.0.1:PORT/v1). `harness_chat_send` auto-starts
            // the sidecar BEFORE resolving, so a missing endpoint means the
            // caller skipped the lifecycle.
            let endpoint = config.endpoint.clone().context(
                "built-in model is not running (no endpoint) — start it via harness_builtin_start",
            )?;
            // CRITICAL: CompletionsClient = Chat Completions. The default
            // `openai::Client` targets `/v1/responses`, which llama-server
            // does not implement (every request would 404).
            let rig = rig::providers::openai::CompletionsClient::builder()
                .api_key("sk-local")
                .base_url(endpoint.as_str())
                .build()
                .context("builtin (llama-server) client build")?;
            Ok(ProviderClient::Builtin { config, rig })
        }
        other => anyhow::bail!("unsupported provider kind: {other}"),
    }
}

/// Probe localhost + common Ollama endpoints, return the first reachable one
/// along with its model list. Carries over the pattern from the deleted
/// `ai::ollama::detect_local`.
pub async fn detect_local_ollama() -> Option<DetectedOllama> {
    let candidates = ["http://127.0.0.1:11434", "http://localhost:11434"];
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .ok()?;
    for ep in candidates {
        let url = format!("{ep}/api/tags");
        if let Ok(res) = client.get(&url).send().await {
            if res.status().is_success() {
                let models = parse_ollama_tag_list(res).await.unwrap_or_default();
                return Some(DetectedOllama {
                    endpoint: ep.to_string(),
                    models,
                });
            }
        }
    }
    None
}

async fn ollama_health(config: &ProviderConfig) -> HealthReport {
    let endpoint = config
        .endpoint
        .clone()
        .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return HealthReport {
                ok: false,
                status: "error".into(),
                detail: Some(e.to_string()),
            };
        }
    };
    let url = format!("{endpoint}/api/tags");
    match client.get(&url).send().await {
        Ok(res) if res.status().is_success() => HealthReport {
            ok: true,
            status: "ok".into(),
            detail: Some(format!("reachable at {endpoint}")),
        },
        Ok(res) => HealthReport {
            ok: false,
            status: "unreachable".into(),
            detail: Some(format!("HTTP {}", res.status())),
        },
        Err(e) => HealthReport {
            ok: false,
            status: "network-error".into(),
            detail: Some(e.to_string()),
        },
    }
}

async fn azure_health(config: &ProviderConfig) -> HealthReport {
    // If the user hasn't filled in the config, this is `skipped` per eval-plan
    // E2.2 rather than a hard failure — uninstalled providers shouldn't fail
    // a happy-path eval run.
    if config.endpoint.is_none() || config.api_key.is_none() || config.deployment.is_none() {
        return HealthReport {
            ok: true,
            status: "skipped".into(),
            detail: Some("azure provider not fully configured".into()),
        };
    }
    let endpoint = config.endpoint.as_deref().unwrap_or("");
    let api_key = config.api_key.as_deref().unwrap_or("");
    let deployment = config.deployment.as_deref().unwrap_or("");
    let api_version = config
        .api_version
        .as_deref()
        .unwrap_or("2024-08-01-preview");
    let url = format!(
        "{}/openai/deployments/{}/chat/completions?api-version={}",
        endpoint.trim_end_matches('/'),
        deployment,
        api_version
    );
    // No max_tokens here: GPT-5 / o-series / claude-* Azure deployments reject
    // max_tokens with HTTP 400 ("Unsupported parameter ... Use max_completion_tokens").
    // We probe with a tiny prompt and let the model emit a couple of tokens — fast
    // enough that omitting the cap is cheap, and uniform across families.
    let body = serde_json::json!({
        "messages": [{ "role": "user", "content": "ping" }],
    });
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return HealthReport {
                ok: false,
                status: "error".into(),
                detail: Some(e.to_string()),
            };
        }
    };
    match client
        .post(&url)
        .header("api-key", api_key)
        .json(&body)
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => HealthReport {
            ok: true,
            status: "ok".into(),
            detail: Some(format!("deployment {deployment} reachable")),
        },
        Ok(res) => {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            HealthReport {
                ok: false,
                status: "unreachable".into(),
                detail: Some(format!(
                    "HTTP {status}: {}",
                    text.chars().take(180).collect::<String>()
                )),
            }
        }
        Err(e) => HealthReport {
            ok: false,
            status: "network-error".into(),
            detail: Some(e.to_string()),
        },
    }
}

/// Health probe for the built-in llama-server sidecar: GET `{base}/health`
/// where base is the endpoint minus its `/v1` suffix. llama-server answers
/// 200 when ready and 503 while the GGUF is still loading.
async fn builtin_health(config: &ProviderConfig) -> HealthReport {
    let Some(endpoint) = config.endpoint.clone() else {
        return HealthReport {
            ok: false,
            status: "stopped".into(),
            detail: Some("built-in model is not running".into()),
        };
    };
    let base = endpoint
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .trim_end_matches('/')
        .to_string();
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return HealthReport {
                ok: false,
                status: "error".into(),
                detail: Some(e.to_string()),
            };
        }
    };
    let url = format!("{base}/health");
    match client.get(&url).send().await {
        Ok(res) if res.status().is_success() => HealthReport {
            ok: true,
            status: "ok".into(),
            detail: Some(format!("llama-server ready at {base}")),
        },
        Ok(res) if res.status().as_u16() == 503 => HealthReport {
            ok: false,
            status: "loading".into(),
            detail: Some("llama-server is still loading the model".into()),
        },
        Ok(res) => HealthReport {
            ok: false,
            status: "unreachable".into(),
            detail: Some(format!("HTTP {}", res.status())),
        },
        Err(e) => HealthReport {
            ok: false,
            status: "network-error".into(),
            detail: Some(e.to_string()),
        },
    }
}

/// The single bundled model, with its context window pinned to the `-c` we
/// launch llama-server with (single source of truth in `builtin::BUILTIN_CTX`).
pub fn builtin_model_info() -> ModelInfo {
    use crate::harness::builtin::{BUILTIN_CTX, BUILTIN_MODEL_ID, BUILTIN_MODEL_NAME};
    ModelInfo {
        id: BUILTIN_MODEL_ID.to_string(),
        label: BUILTIN_MODEL_NAME.to_string(),
        context_window: Some(BUILTIN_CTX),
        supports_tools: true,
    }
}

/// Models that reject `max_tokens` in favor of `max_completion_tokens` (Azure
/// gpt-5* family, o-series, claude-* on Foundry). Match on the deployment id
/// the user configured. Conservative pattern: any deployment whose id begins
/// with gpt-5, o1, o2, o3, o4, claude, or is the gpt-chat-latest / model-router
/// catch-alls that route to the new shape.
pub fn requires_max_completion_tokens(deployment: &str) -> bool {
    let d = deployment.to_ascii_lowercase();
    d.starts_with("gpt-5")
        || d.starts_with("o1")
        || d.starts_with("o2")
        || d.starts_with("o3")
        || d.starts_with("o4")
        || d.starts_with("claude")
        || d == "gpt-chat-latest"
        || d == "model-router"
}

async fn list_ollama_models(config: &ProviderConfig) -> Result<Vec<ModelInfo>> {
    let endpoint = config
        .endpoint
        .clone()
        .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .context("reqwest client")?;
    let url = format!("{endpoint}/api/tags");
    let res = client.get(&url).send().await.context("ollama /api/tags")?;
    if !res.status().is_success() {
        anyhow::bail!("ollama HTTP {}", res.status());
    }
    parse_ollama_tag_list(res).await
}

/// Probe Ollama's `/api/show` for a specific model and extract its native
/// context window. Returns `None` if the model isn't installed, the endpoint
/// is unreachable, or the parameters block doesn't expose `num_ctx`.
pub async fn ollama_context_window(endpoint: &str, model: &str) -> Option<u32> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .ok()?;
    let url = format!("{}/api/show", endpoint.trim_end_matches('/'));
    let body = serde_json::json!({ "model": model });
    let res = client.post(&url).json(&body).send().await.ok()?;
    if !res.status().is_success() {
        return None;
    }
    let payload: serde_json::Value = res.json().await.ok()?;
    // Ollama's `/api/show` returns `parameters` as a newline-delimited blob
    // like "num_ctx 8192\nstop ...". We grep for `num_ctx <number>`.
    if let Some(params_str) = payload.get("parameters").and_then(|v| v.as_str()) {
        for line in params_str.lines() {
            let mut parts = line.split_whitespace();
            if matches!(parts.next(), Some("num_ctx")) {
                if let Some(n) = parts.next().and_then(|s| s.parse::<u32>().ok()) {
                    return Some(n);
                }
            }
        }
    }
    // Newer Ollama also exposes `model_info.{family}.context_length` in some shapes.
    if let Some(model_info) = payload.get("model_info").and_then(|v| v.as_object()) {
        for (k, v) in model_info {
            if k.ends_with(".context_length") {
                if let Some(n) = v.as_u64() {
                    return Some(n as u32);
                }
            }
        }
    }
    None
}

async fn parse_ollama_tag_list(res: reqwest::Response) -> Result<Vec<ModelInfo>> {
    #[derive(Deserialize)]
    struct TagList {
        models: Vec<Tag>,
    }
    #[derive(Deserialize)]
    struct Tag {
        name: String,
        #[serde(default)]
        details: Option<TagDetails>,
    }
    #[derive(Deserialize)]
    struct TagDetails {
        #[serde(default)]
        parameter_size: Option<String>,
    }
    let body = res
        .json::<TagList>()
        .await
        .context("ollama tag list parse")?;
    Ok(body
        .models
        .into_iter()
        .map(|t| {
            let label = match t.details.as_ref().and_then(|d| d.parameter_size.as_ref()) {
                Some(size) => format!("{} ({size})", t.name),
                None => t.name.clone(),
            };
            ModelInfo {
                id: t.name,
                label,
                context_window: None,
                supports_tools: true, // Ollama models with `tools:` capability — we'll refine when wiring agent.
            }
        })
        .collect())
}

fn list_azure_deployment(config: &ProviderConfig) -> Vec<ModelInfo> {
    let id = config.deployment.clone().unwrap_or_default();
    if id.is_empty() {
        return Vec::new();
    }
    vec![ModelInfo {
        id: id.clone(),
        label: id,
        context_window: None,
        supports_tools: true,
    }]
}
