// @agent-context: ClawHub marketplace provider (OpenClaw skill registry).
//
// Endpoint:  GET https://clawhub.ai/api/v1/search?q=<query>
// Override base URL via env `CLAWHUB_REGISTRY`.
//
// Response shape (from the published CLI contract — see docs/cli.md in the
// openclaw/clawhub repository):
// {
//   "results": [
//     {
//       "score": 4.21,
//       "slug": "rust",
//       "displayName": "Rust",
//       "summary": "...",
//       "version": null,
//       "updatedAt": 1778486238781,
//       "ownerHandle": "ivangdavila",
//       "owner": { "handle": "...", "displayName": "...", "image": "..." }
//     }
//   ]
// }
//
// The search API does not return an install command or page URL, so we derive:
//   install_command = `openclaw skills install <ownerHandle>/<slug>`
//   homepage_url    = `https://clawhub.ai/<ownerHandle>/<slug>`

use anyhow::{anyhow, Context, Result};
use reqwest::StatusCode;
use serde::Deserialize;
use std::time::Instant;

use super::types::{
    normalize_limit, MarketplaceSearchResponse, ProviderId, RegistryItem, RegistryKind,
};

const DEFAULT_BASE_URL: &str = "https://clawhub.ai";

fn base_url() -> String {
    std::env::var("CLAWHUB_REGISTRY")
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawResponse {
    #[serde(default)]
    results: Vec<RawResult>,
    #[serde(default)]
    count: Option<usize>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    query: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawResult {
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    updated_at: Option<serde_json::Value>,
    #[serde(default)]
    owner_handle: Option<String>,
    #[serde(default)]
    owner: Option<RawOwner>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOwner {
    #[serde(default)]
    handle: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    image: Option<String>,
}

fn format_updated_at(value: Option<&serde_json::Value>) -> Option<String> {
    let v = value?;
    let millis = match v {
        serde_json::Value::Number(n) => n.as_i64()?,
        serde_json::Value::String(s) => s.trim().parse::<i64>().ok()?,
        _ => return None,
    };

    let seconds = millis / 1000;
    let datetime = chrono_like_iso(seconds);
    Some(datetime)
}

/// Format Unix seconds as an ISO-8601 UTC string without pulling in chrono.
/// We only need a stable, human-readable form for the UI tooltip; a full
/// chrono dependency is overkill for one line.
fn chrono_like_iso(seconds: i64) -> String {
    // Days since 1970-01-01.
    let days = seconds.div_euclid(86_400);
    let secs_in_day = seconds.rem_euclid(86_400);
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;

    // Convert days to Y/M/D using the algorithm from Howard Hinnant
    // (https://howardhinnant.github.io/date_algorithms.html#civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hour, minute, second
    )
}

fn map_item(raw: RawResult) -> Option<RegistryItem> {
    let slug = raw.slug.trim().to_string();
    if slug.is_empty() {
        return None;
    }

    let owner_handle = raw
        .owner_handle
        .clone()
        .or_else(|| raw.owner.as_ref().and_then(|o| o.handle.clone()))
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty());

    let id = match owner_handle.as_deref() {
        Some(handle) => format!("{}/{}", handle, slug),
        None => slug.clone(),
    };

    let name = if raw.display_name.trim().is_empty() {
        slug.clone()
    } else {
        raw.display_name.trim().to_string()
    };

    let homepage_url = owner_handle
        .as_ref()
        .map(|h| format!("{}/{}/{}", DEFAULT_BASE_URL, h, slug));

    let install_command = match owner_handle.as_deref() {
        Some(handle) => format!("openclaw skills install {}/{}", handle, slug),
        None => format!("openclaw skills install {}", slug),
    };

    let author = raw
        .owner
        .as_ref()
        .and_then(|o| o.display_name.clone())
        .or_else(|| owner_handle.clone());

    let author_avatar_url = raw.owner.as_ref().and_then(|o| o.image.clone());

    Some(RegistryItem {
        id,
        provider: ProviderId::ClawHub,
        kind: RegistryKind::Skill,
        name,
        slug,
        install_command,
        author,
        author_handle: owner_handle,
        author_avatar_url,
        description: raw
            .summary
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        version: raw
            .version
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        installs: 0,
        source: Some("ClawHub".to_string()),
        source_url: None,
        homepage_url,
        updated_at: format_updated_at(raw.updated_at.as_ref()),
        score: raw.score,
    })
}

pub async fn search(query: &str, limit: usize) -> Result<MarketplaceSearchResponse> {
    let trimmed = query.trim();
    if trimmed.len() < 2 {
        return Ok(MarketplaceSearchResponse {
            provider: ProviderId::ClawHub,
            query: trimmed.to_string(),
            count: 0,
            duration_ms: 0,
            items: Vec::new(),
        });
    }

    let normalized_limit = normalize_limit(limit);
    let endpoint = format!("{}/api/v1/search", base_url());

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .context("failed to initialize ClawHub HTTP client")?;

    let started = Instant::now();

    let response = client
        .get(&endpoint)
        .query(&[("q", trimmed), ("limit", &normalized_limit.to_string())])
        .send()
        .await
        .with_context(|| format!("failed to reach ClawHub endpoint {}", endpoint))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let message = match status {
            StatusCode::TOO_MANY_REQUESTS => {
                "ClawHub rate limit reached, try again shortly".to_string()
            }
            StatusCode::BAD_REQUEST => {
                "invalid ClawHub search request, verify query format".to_string()
            }
            _ => format!("ClawHub search failed with status {}", status),
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
        .context("ClawHub returned invalid JSON")?;

    let items: Vec<RegistryItem> = raw.results.into_iter().filter_map(map_item).collect();

    Ok(MarketplaceSearchResponse {
        provider: ProviderId::ClawHub,
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
    fn iso_format_known_epoch() {
        // 2025-01-02T03:04:05Z == 1735787045
        assert_eq!(chrono_like_iso(1_735_787_045), "2025-01-02T03:04:05Z");
        // 1970-01-01T00:00:00Z
        assert_eq!(chrono_like_iso(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn map_item_derives_install_and_homepage_when_owner_known() {
        let raw = RawResult {
            score: Some(1.0),
            slug: "rust".to_string(),
            display_name: "Rust".to_string(),
            summary: Some("Write idiomatic Rust".to_string()),
            version: None,
            updated_at: None,
            owner_handle: Some("ivangdavila".to_string()),
            owner: Some(RawOwner {
                handle: Some("ivangdavila".to_string()),
                display_name: Some("Iván".to_string()),
                image: Some("https://example/x.png".to_string()),
            }),
        };

        let item = map_item(raw).expect("item");
        assert_eq!(item.id, "ivangdavila/rust");
        assert_eq!(
            item.install_command,
            "openclaw skills install ivangdavila/rust"
        );
        assert_eq!(
            item.homepage_url.as_deref(),
            Some("https://clawhub.ai/ivangdavila/rust")
        );
        assert_eq!(item.author.as_deref(), Some("Iván"));
        assert_eq!(item.author_handle.as_deref(), Some("ivangdavila"));
    }

    #[test]
    fn map_item_falls_back_when_owner_missing() {
        let raw = RawResult {
            score: None,
            slug: "ungoverned".to_string(),
            display_name: String::new(),
            summary: None,
            version: None,
            updated_at: None,
            owner_handle: None,
            owner: None,
        };

        let item = map_item(raw).expect("item");
        assert_eq!(item.id, "ungoverned");
        assert_eq!(item.install_command, "openclaw skills install ungoverned");
        assert!(item.homepage_url.is_none());
    }

    #[test]
    fn map_item_rejects_empty_slug() {
        let raw = RawResult {
            score: None,
            slug: String::new(),
            display_name: "Junk".to_string(),
            summary: None,
            version: None,
            updated_at: None,
            owner_handle: None,
            owner: None,
        };
        assert!(map_item(raw).is_none());
    }
}
