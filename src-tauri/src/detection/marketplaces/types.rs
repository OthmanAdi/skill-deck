// @agent-context: Shared types for the marketplace registry layer.
//
// `RegistryItem` is the universal shape every provider normalizes to. Keep
// every field optional that is not strictly universal so a thin provider
// (no install count, no version, no homepage) can still produce valid items.

use serde::{Deserialize, Serialize};

pub const MAX_LIMIT: usize = 50;

/// Identifies which marketplace produced a given result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderId {
    SkillsSh,
    ClawHub,
}

/// Coarse-grained artifact kind. Most marketplaces only ship skills today, but
/// ClawHub is moving toward plugin + webhook packages, so the surface is here
/// when those endpoints land.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistryKind {
    Skill,
    Plugin,
    Hook,
    Command,
    Unknown,
}

/// Universal marketplace item. Lives across providers — every field after
/// `name` is optional to keep the contract honest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryItem {
    pub id: String,
    pub provider: ProviderId,
    pub kind: RegistryKind,
    pub name: String,
    pub slug: String,
    pub install_command: String,
    pub author: Option<String>,
    pub author_handle: Option<String>,
    pub author_avatar_url: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub installs: u64,
    pub source: Option<String>,
    /// Origin URL — the repo or canonical upstream (GitHub, GitLab, etc.).
    pub source_url: Option<String>,
    /// Marketplace page URL — the listing on skills.sh / clawhub.ai / etc.
    pub homepage_url: Option<String>,
    /// Last-updated timestamp from the provider, formatted ISO-8601 when
    /// possible. Optional — providers may omit it.
    pub updated_at: Option<String>,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceSearchResponse {
    pub provider: ProviderId,
    pub query: String,
    pub count: usize,
    pub duration_ms: u64,
    pub items: Vec<RegistryItem>,
}

pub fn normalize_limit(limit: usize) -> usize {
    limit.clamp(1, MAX_LIMIT)
}
