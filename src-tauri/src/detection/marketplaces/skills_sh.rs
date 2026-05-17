// @agent-context: skills.sh marketplace provider (vercel-labs/skills, npx skills).
//
// Endpoint:  GET https://skills.sh/api/search?q=<query>&limit=<n>
// Override base URL via env `SKILLS_API_URL`.
//
// This is the SAME endpoint the official `npx skills find` CLI uses (see
// vercel-labs/skills/src/find.ts), and it does NOT require an API key.
// The newer `/api/v1/skills/search` route requires a `Bearer sk_live_...`
// key, which is why we deliberately stay on `/api/search` here.
//
// Response shape (open route):
// {
//   "skills": [
//     {
//       "id": "owner/repo/slug",
//       "name": "Display Name",
//       "source": "owner/repo",
//       "installs": 24531
//     }
//   ]
// }
//
// `source` is always "owner/repo" (the GitHub source), so we derive a
// `sourceUrl` of `https://github.com/{source}` ourselves to keep the
// "Open source" button in the UI working without needing the v1 route.

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
    skills: Vec<RawSkill>,
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
    slug: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    installs: Option<serde_json::Value>,
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
    let trimmed_id = raw.id.trim();
    let slug_trimmed = raw.slug.as_deref().unwrap_or("").trim();
    let source_trimmed = raw.source.as_deref().unwrap_or("").trim();

    // The open endpoint always returns a full `id` like "owner/repo/slug",
    // but we tolerate the older split shape (source + slug) as a fallback so
    // we don't break if Vercel ever switches the wire format.
    let id = if !trimmed_id.is_empty() {
        trimmed_id.to_string()
    } else if !source_trimmed.is_empty() && !slug_trimmed.is_empty() {
        format!("{}/{}", source_trimmed.trim_end_matches('/'), slug_trimmed)
    } else {
        return None;
    };

    let derived_slug = if !slug_trimmed.is_empty() {
        slug_trimmed.to_string()
    } else {
        id.rsplit('/').next().unwrap_or(&id).to_string()
    };

    let name = if raw.name.trim().is_empty() {
        derived_slug.clone()
    } else {
        raw.name.trim().to_string()
    };

    let author_handle = if !source_trimmed.is_empty() {
        source_trimmed.split('/').next().map(|s| s.to_string())
    } else {
        // Fall back to the first segment of `id` so cards still attribute the
        // author even when the API omits `source`.
        id.split('/').next().map(|s| s.to_string())
    };

    // `source` (when present) is the GitHub repo path, so a deterministic
    // sourceUrl gives us the "Open source" button without needing the v1
    // installUrl field that requires auth.
    let source_url = if !source_trimmed.is_empty() {
        Some(format!("https://github.com/{}", source_trimmed))
    } else {
        None
    };

    let homepage_url = Some(format!("https://skills.sh/{}", id));

    Some(RegistryItem {
        id: id.clone(),
        provider: ProviderId::SkillsSh,
        kind: RegistryKind::Skill,
        name,
        slug: derived_slug,
        install_command: build_install_command(&id),
        author: author_handle.clone(),
        author_handle,
        author_avatar_url: None,
        description: None,
        version: None,
        installs: parse_installs(raw.installs.as_ref()),
        source: if source_trimmed.is_empty() {
            None
        } else {
            Some(source_trimmed.to_string())
        },
        source_url,
        homepage_url,
        updated_at: None,
        score: None,
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
    let endpoint = format!("{}/api/search", base_url());

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

    let items: Vec<RegistryItem> = raw.skills.into_iter().filter_map(map_item).collect();

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
    fn map_item_derives_github_source_url_from_source_field() {
        let raw = RawSkill {
            id: "owner/repo/skill".to_string(),
            slug: Some("skill".to_string()),
            name: "Skill".to_string(),
            source: Some("owner/repo".to_string()),
            installs: None,
        };

        let item = map_item(raw).expect("item");
        assert_eq!(
            item.source_url.as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(
            item.homepage_url.as_deref(),
            Some("https://skills.sh/owner/repo/skill")
        );
        assert_eq!(item.author.as_deref(), Some("owner"));
    }

    #[test]
    fn map_item_skips_when_no_id_or_source_slug() {
        let raw = RawSkill {
            id: String::new(),
            slug: None,
            name: String::new(),
            source: None,
            installs: None,
        };
        assert!(map_item(raw).is_none());
    }

    #[test]
    fn map_item_falls_back_to_id_tail_for_name_when_blank() {
        let raw = RawSkill {
            id: "owner/repo/cool-skill".to_string(),
            slug: None,
            name: String::new(),
            source: Some("owner/repo".to_string()),
            installs: Some(serde_json::Value::Number(serde_json::Number::from(99u64))),
        };
        let item = map_item(raw).expect("item");
        assert_eq!(item.name, "cool-skill");
        assert_eq!(item.installs, 99);
    }
}
