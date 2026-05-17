// @agent-context: Multi-source registry / marketplace abstraction.
//
// Each provider lives in its own file and implements a `search(query, limit)`
// async function returning a normalized `MarketplaceSearchResponse`. The top
// of this module exposes a dispatcher `search(provider, query, limit)` that
// the Tauri command layer calls.
//
// Add a new provider:
//   1. Create `marketplaces/<name>.rs` with `pub async fn search(query, limit)`
//   2. Add a variant to `ProviderId`
//   3. Add a match arm in `search()` below
//
// The shared `RegistryItem` type is deliberately flat and serializable —
// the frontend renders it in a uniform card UI regardless of source.

pub mod clawhub;
pub mod skills_sh;
pub mod types;

pub use types::{normalize_limit, MarketplaceSearchResponse, ProviderId};

use anyhow::Result;

/// Dispatch a search to a single provider.
pub async fn search(
    provider: ProviderId,
    query: &str,
    limit: usize,
) -> Result<MarketplaceSearchResponse> {
    match provider {
        ProviderId::SkillsSh => skills_sh::search(query, limit).await,
        ProviderId::ClawHub => clawhub::search(query, limit).await,
    }
}

/// Fan-out search across every known provider in parallel. Returns one merged
/// response per provider, so the caller can choose to display per-source
/// sections or one mixed list.
pub async fn search_all(query: &str, limit: usize) -> Vec<ProviderSearchOutcome> {
    let (sk, cl) = tokio::join!(
        skills_sh::search(query, limit),
        clawhub::search(query, limit),
    );

    vec![
        ProviderSearchOutcome::from_result(ProviderId::SkillsSh, sk),
        ProviderSearchOutcome::from_result(ProviderId::ClawHub, cl),
    ]
}

/// One provider's result in a multi-search, preserving partial failures so the
/// UI can still render the providers that succeeded.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSearchOutcome {
    pub provider: ProviderId,
    pub response: Option<MarketplaceSearchResponse>,
    pub error: Option<String>,
}

impl ProviderSearchOutcome {
    fn from_result(provider: ProviderId, result: Result<MarketplaceSearchResponse>) -> Self {
        match result {
            Ok(response) => Self {
                provider,
                response: Some(response),
                error: None,
            },
            Err(e) => Self {
                provider,
                response: None,
                error: Some(e.to_string()),
            },
        }
    }
}
