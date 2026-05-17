// @agent-context: Provider registry — builds a concrete LlmProvider from
// a ProviderConfig stored in AppConfig. Adding a new provider kind is a
// single match arm here plus a new module file.

use super::azure_foundry::AzureFoundryProvider;
use super::ollama::OllamaProvider;
use super::provider::{LlmProvider, ProviderConfig, ProviderError};
use serde::Serialize;
use std::sync::Arc;

pub fn build_provider(config: ProviderConfig) -> Result<Arc<dyn LlmProvider>, ProviderError> {
    match config.kind.as_str() {
        "ollama" => Ok(Arc::new(OllamaProvider::new(config))),
        "azure-foundry" => Ok(Arc::new(AzureFoundryProvider::new(config))),
        other => Err(ProviderError::NotConfigured(format!(
            "unknown provider kind: {}",
            other
        ))),
    }
}

/// Frontend-facing snapshot of all configured providers and which is active.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRegistrySnapshot {
    pub providers: Vec<ProviderConfig>,
    pub active_provider: Option<String>,
    pub active_model: Option<String>,
}
