// @agent-context: skills.sh marketplace provider (vercel-labs/skills, npx skills).
//
// Endpoint:  GET https://skills.sh/api/v1/skills/search?q=<query>&limit=<n>
// Override base URL via env `SKILLS_API_URL`.
//
// Response shape (documented v1):
// {
//   "data": [
//     {
//       "id": "owner/repo/slug",
//       "slug": "slug",
//       "name": "Display Name",
//       "source": "owner/repo",
//       "installs": 24531,
//       "sourceType": "github",
//       "installUrl": "https://github.com/owner/repo",
//       "url": "https://skills.sh/owner/repo/slug"
//     }
//   ],
//   "query": "...",
//   "searchType": "semantic",
//   "count": 5,
//   "durationMs": 142
// }

use anyhow::{anyhow, Context, Result};
use reqwest::StatusCode;
use serde::Deserialize;
use std::time::Instant;

use super::types::{
    normalize_limit, MarketplaceSearchResponse, ProviderId, RegistryItem, RegistryKind,
};

const DEFAULT_BASE_URL: &str = "https://skills.sh";

fn base_url() -> String {
    std::env::var("SKILLS_API_URL")
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawResponse {
    #[serde(default)]
    data: Vec<RawSkill>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    count: Option<usize>,
    #[serde(default)]
    duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSkill {
    #[serde(default)]
    id: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    installs: Option<serde_json::Value>,
    #[serde(default)]
    source_type: Option<String>,
    #[serde(default)]
    install_url: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

fn parse_installs(value: Option<&serde_json::Value>) -> u64 {
    match value {
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
        Some(serde_json::Value::String(s)) => s.trim().parse::<u64>().unwrap_or(0),
        _ => 0,
    }
}

fn build_install_command(id: &str) -> String {
    format!("npx skills add {} -g", id)
}

fn map_item(raw: RawSkill) -> Option<RegistryItem> {
    let id = if !raw.id.trim().is_empty() {
        raw.id.trim().to_string()
    } else if !raw.source.as_deref().unwrap_or("").trim().is_empty()
        && !raw.slug.trim().is_empty()
    {
        format!(
            "{}/{}",
            raw.source.as_deref().unwrap_or("").trim_end_matches('/'),
            raw.slug.trim()
        )
    } else {
        return None;
    };

    let name = if raw.name.trim().is_empty() {
        id.rsplit('/').next().unwrap_or(&id).to_string()
    } else {
        raw.name.trim().to_string()
    };

    Some(RegistryItem {
        id: id.clone(),
        provider: ProviderId::SkillsSh,
        kind: RegistryKind::Skill,
        name,
        slug: raw.slug.trim().to_string(),
        install_command: build_install_command(&id),
        author: raw
            .source
            .as_ref()
            .and_then(|s| s.split('/').next())
            .map(|s| s.to_string()),
        author_handle: raw
            .source
            .as_ref()
            .and_then(|s| s.split('/').next())
            .map(|s| s.to_string()),
        author_avatar_url: None,
        description: None,
        version: None,
        installs: parse_installs(raw.installs.as_ref()),
        source: raw
            .source
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        source_url: raw
            .install_url
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        homepage_url: raw
            .url
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        updated_at: None,
        score: None,
    })
    .map(|mut item| {
        if let Some(st) = raw.source_type.as_deref() {
            if st.eq_ignore_ascii_case("github") && item.source_url.is_none() {
                if let Some(source) = &item.source {
                    item.source_url = Some(format!("https://github.com/{}", source));
                }
            }
        }
        item
    })
}

pub async fn search(query: &str, limit: usize) -> Result<MarketplaceSearchResponse> {
    let trimmed = query.trim();
    if trimmed.len() < 2 {
        return Ok(MarketplaceSearchResponse {
            provider: ProviderId::SkillsSh,
            query: trimmed.to_string(),
            count: 0,
            duration_ms: 0,
            items: Vec::new(),
        });
    }

    let normalized_limit = normalize_limit(limit);
    let endpoint = format!("{}/api/v1/skills/search", base_url());

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("failed to initialize skills.sh HTTP client")?;

    let started = Instant::now();

    let response = client
        .get(&endpoint)
        .query(&[("q", trimmed), ("limit", &normalized_limit.to_string())])
        .send()
        .await
        .with_context(|| format!("failed to reach skills.sh endpoint {}", endpoint))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let message = match status {
            StatusCode::TOO_MANY_REQUESTS => {
                "skills.sh rate limit reached, try again shortly".to_string()
            }
            StatusCode::BAD_REQUEST => {
                "invalid skills.sh search request, verify query format".to_string()
            }
            _ => format!("skills.sh search failed with status {}", status),
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

    let raw: RawResponse = response
        .json()
        .await
        .context("skills.sh returned invalid JSON")?;

    let items: Vec<RegistryItem> = raw.data.into_iter().filter_map(map_item).collect();

    Ok(MarketplaceSearchResponse {
        provider: ProviderId::SkillsSh,
        query: raw.query.unwrap_or_else(|| trimmed.to_string()),
        count: raw.count.unwrap_or(items.len()),
        duration_ms: raw.duration_ms.unwrap_or_else(|| started.elapsed().as_millis() as u64),
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_command_format() {
        assert_eq!(
            build_install_command("vercel-labs/agent-skills/next-js"),
            "npx skills add vercel-labs/agent-skills/next-js -g"
        );
    }

    #[test]
    fn parse_installs_accepts_number_and_string() {
        assert_eq!(
            parse_installs(Some(&serde_json::Value::Number(serde_json::Number::from(
                42u64
            )))),
            42
        );
        assert_eq!(
            parse_installs(Some(&serde_json::Value::String("17".to_string()))),
            17
        );
        assert_eq!(
            parse_installs(Some(&serde_json::Value::String("nope".to_string()))),
            0
        );
        assert_eq!(parse_installs(None), 0);
    }

    #[test]
    fn map_item_derives_github_source_url_when_missing() {
        let raw = RawSkill {
            id: "owner/repo/skill".to_string(),
            slug: "skill".to_string(),
            name: "Skill".to_string(),
            source: Some("owner/repo".to_string()),
            installs: None,
            source_type: Some("github".to_string()),
            install_url: None,
            url: None,
        };

        let item = map_item(raw).expect("item");
        assert_eq!(
            item.source_url.as_deref(),
            Some("https://github.com/owner/repo")
        );
    }

    #[test]
    fn map_item_skips_when_no_id_or_source_slug() {
        let raw = RawSkill {
            id: String::new(),
            slug: String::new(),
            name: String::new(),
            source: None,
            installs: None,
            source_type: None,
            install_url: None,
            url: None,
        };
        assert!(map_item(raw).is_none());
    }
}
