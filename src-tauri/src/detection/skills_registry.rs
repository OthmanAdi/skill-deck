// @agent-context: Client for browsing the public npx skills registry API.
//
// API contract:
// GET {SKILLS_API_URL|https://skills.sh}/api/search?q=<query>&limit=<n>
// Returns JSON with a `skills` array and optional metadata fields.

use anyhow::{anyhow, Context, Result};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

const DEFAULT_REGISTRY_BASE_URL: &str = "https://skills.sh";
pub const MAX_LIMIT: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySkillSummary {
    pub id: String,
    pub skill_id: Option<String>,
    pub name: String,
    pub installs: u64,
    pub source: Option<String>,
    pub install_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySearchResponse {
    pub query: String,
    pub count: usize,
    pub duration_ms: u64,
    pub skills: Vec<RegistrySkillSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRegistrySearchResponse {
    #[serde(default)]
    query: String,
    #[serde(default)]
    count: Option<usize>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    skills: Vec<RawRegistrySkill>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRegistrySkill {
    #[serde(default)]
    id: String,
    #[serde(default)]
    skill_id: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    installs: Option<serde_json::Value>,
    #[serde(default)]
    source: Option<String>,
}

fn parse_installs(value: Option<serde_json::Value>) -> u64 {
    let Some(value) = value else {
        return 0;
    };

    match value {
        serde_json::Value::Number(number) => number.as_u64().unwrap_or(0),
        serde_json::Value::String(text) => text.trim().parse::<u64>().unwrap_or(0),
        _ => 0,
    }
}

fn resolve_skill_id(raw: &RawRegistrySkill) -> Option<String> {
    let id = raw.id.trim();
    if !id.is_empty() {
        return Some(id.to_string());
    }

    let source = raw.source.as_deref().unwrap_or("").trim();
    let skill_id = raw.skill_id.as_deref().unwrap_or("").trim();

    if source.is_empty() || skill_id.is_empty() {
        return None;
    }

    Some(format!("{}/{}", source.trim_end_matches('/'), skill_id))
}

fn default_name(raw: &RawRegistrySkill, resolved_id: &str) -> String {
    let name = raw.name.trim();
    if !name.is_empty() {
        return name.to_string();
    }

    resolved_id
        .rsplit('/')
        .next()
        .map(str::to_string)
        .unwrap_or_else(|| resolved_id.to_string())
}

fn build_install_command(id: &str) -> String {
    format!("npx skills add {} -g", id)
}

pub fn normalize_limit(limit: usize) -> usize {
    limit.clamp(1, MAX_LIMIT)
}

fn registry_base_url() -> String {
    std::env::var("SKILLS_API_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRY_BASE_URL.to_string())
}

pub async fn search_registry(query: &str, limit: usize) -> Result<RegistrySearchResponse> {
    let trimmed_query = query.trim();
    if trimmed_query.len() < 2 {
        return Ok(RegistrySearchResponse {
            query: trimmed_query.to_string(),
            count: 0,
            duration_ms: 0,
            skills: Vec::new(),
        });
    }

    let base_url = registry_base_url();
    let endpoint = format!("{}/api/search", base_url);
    let normalized_limit = normalize_limit(limit);

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("failed to initialize registry HTTP client")?;

    let response = client
        .get(&endpoint)
        .query(&[("q", trimmed_query), ("limit", &normalized_limit.to_string())])
        .send()
        .await
        .with_context(|| format!("failed to reach registry endpoint {}", endpoint))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let message = match status {
            StatusCode::TOO_MANY_REQUESTS => {
                "skills registry rate limit reached, try again shortly".to_string()
            }
            StatusCode::BAD_REQUEST => {
                "invalid registry search request, verify query format".to_string()
            }
            _ => format!("registry search failed with status {}", status),
        };

        return Err(anyhow!(
            "{}{}",
            message,
            if body.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", body.trim())
            }
        ));
    }

    let raw: RawRegistrySearchResponse = response
        .json()
        .await
        .context("registry returned invalid JSON")?;

    let mut skills: Vec<RegistrySkillSummary> = Vec::new();
    for raw_skill in &raw.skills {
        let Some(resolved_id) = resolve_skill_id(raw_skill) else {
            continue;
        };

        skills.push(RegistrySkillSummary {
            id: resolved_id.clone(),
            skill_id: raw_skill.skill_id.clone(),
            name: default_name(raw_skill, &resolved_id),
            installs: parse_installs(raw_skill.installs.clone()),
            source: raw_skill
                .source
                .as_ref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            install_command: build_install_command(&resolved_id),
        });
    }

    Ok(RegistrySearchResponse {
        query: if raw.query.trim().is_empty() {
            trimmed_query.to_string()
        } else {
            raw.query
        },
        count: raw.count.unwrap_or(skills.len()),
        duration_ms: raw.duration_ms.unwrap_or(0),
        skills,
    })
}

#[cfg(test)]
mod tests {
    use super::{default_name, parse_installs, resolve_skill_id, RawRegistrySkill};

    #[test]
    fn resolve_skill_id_prefers_explicit_id() {
        let raw = RawRegistrySkill {
            id: "owner/repo/skill-name".to_string(),
            skill_id: Some("skill-name".to_string()),
            name: "Skill Name".to_string(),
            installs: None,
            source: Some("owner/repo".to_string()),
        };

        assert_eq!(
            resolve_skill_id(&raw).as_deref(),
            Some("owner/repo/skill-name")
        );
    }

    #[test]
    fn resolve_skill_id_falls_back_to_source_and_skill_id() {
        let raw = RawRegistrySkill {
            id: String::new(),
            skill_id: Some("skill-name".to_string()),
            name: String::new(),
            installs: None,
            source: Some("owner/repo".to_string()),
        };

        assert_eq!(
            resolve_skill_id(&raw).as_deref(),
            Some("owner/repo/skill-name")
        );
    }

    #[test]
    fn parse_installs_handles_number_and_string() {
        assert_eq!(
            parse_installs(Some(serde_json::Value::Number(serde_json::Number::from(
                42_u64
            )))),
            42
        );
        assert_eq!(
            parse_installs(Some(serde_json::Value::String("17".to_string()))),
            17
        );
        assert_eq!(
            parse_installs(Some(serde_json::Value::String("bad".to_string()))),
            0
        );
    }

    #[test]
    fn default_name_uses_tail_from_id_when_empty() {
        let raw = RawRegistrySkill {
            id: String::new(),
            skill_id: None,
            name: String::new(),
            installs: None,
            source: None,
        };

        assert_eq!(default_name(&raw, "owner/repo/rust-patterns"), "rust-patterns");
    }
}
