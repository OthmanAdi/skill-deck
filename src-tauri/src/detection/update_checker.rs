// @agent-context: Update checker for skills with detected GitHub repository URLs.
//
// STRATEGY:
// 1. Strictly parse and canonicalize user provided repo URLs
// 2. Call GitHub API: GET /repos/{owner}/{repo}/commits?per_page=1
// 3. Compare the remote SHA against a cached previous remote reference
// 4. Cache results to avoid hammering the API (max 1 check per skill per hour)

use regex::Regex;
use reqwest::{StatusCode, Url};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::detection::repo_detector::github_first_segment_is_repo;
use crate::models::{UpdateCheckEntry, UpdateErrorKind};

/// Minimum seconds between update checks for the same skill
const CHECK_COOLDOWN_SECS: u64 = 3600; // 1 hour

#[derive(Debug, Clone, PartialEq, Eq)]
struct GithubRepoRef {
    owner: String,
    repo: String,
}

impl GithubRepoRef {
    fn canonical_url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.repo)
    }

    fn repo_ref(&self) -> String {
        format!("github:{}/{}", self.owner, self.repo)
    }
}

/// Result of an update check
#[derive(Debug, Clone)]
pub struct UpdateCheckResult {
    pub update_available: bool,
    pub remote_ref: Option<String>,
    pub error: Option<String>,
    pub error_kind: Option<UpdateErrorKind>,
    pub canonical_repo_url: Option<String>,
}

pub enum UpdateComparison {
    Same,
    Different,
    Unknown,
}

fn github_owner_repo_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^([\w.\-]+)/([\w.\-]+)$").expect("github owner/repo shorthand regex")
    })
}

fn is_valid_repo_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn parse_github_repo_ref(input: &str) -> Result<GithubRepoRef, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Repository URL is empty".to_string());
    }

    if let Some(caps) = github_owner_repo_re().captures(trimmed) {
        let owner = caps
            .get(1)
            .map(|m| m.as_str().to_string())
            .ok_or_else(|| "Missing GitHub owner".to_string())?;
        let repo = caps
            .get(2)
            .map(|m| m.as_str().trim_end_matches(".git").to_string())
            .ok_or_else(|| "Missing GitHub repository".to_string())?;

        if !is_valid_repo_segment(&owner) || !is_valid_repo_segment(&repo) {
            return Err("GitHub owner/repo contains unsupported characters".to_string());
        }

        if !github_first_segment_is_repo(&owner) {
            return Err(format!(
                "{} is a GitHub site path, not a repository",
                owner
            ));
        }

        return Ok(GithubRepoRef { owner, repo });
    }

    let normalized = if trimmed.starts_with("github.com/") {
        format!("https://{}", trimmed)
    } else {
        trimmed.to_string()
    };

    let parsed =
        Url::parse(&normalized).map_err(|_| "Repository URL is not a valid URL".to_string())?;

    if parsed.scheme() != "https" {
        return Err("Only https GitHub URLs are allowed".to_string());
    }

    let host = parsed
        .host_str()
        .map(|h| h.to_ascii_lowercase())
        .ok_or_else(|| "Repository URL is missing host".to_string())?;

    if host != "github.com" && host != "www.github.com" {
        return Err("Only github.com repositories are supported for update checks".to_string());
    }

    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.filter(|seg| !seg.is_empty()).collect())
        .unwrap_or_default();

    if segments.len() < 2 {
        return Err("Repository URL must include owner and repository name".to_string());
    }

    let owner = segments[0].to_string();
    let repo = segments[1].trim_end_matches(".git").to_string();

    if !is_valid_repo_segment(&owner) || !is_valid_repo_segment(&repo) {
        return Err("GitHub owner/repo contains unsupported characters".to_string());
    }

    if !github_first_segment_is_repo(&owner) {
        return Err(format!(
            "github.com/{}/* is a GitHub site path, not a repository",
            owner
        ));
    }

    Ok(GithubRepoRef { owner, repo })
}

pub fn canonicalize_github_repo_url(input: &str) -> Option<String> {
    parse_github_repo_ref(input)
        .ok()
        .map(|repo| repo.canonical_url())
}

pub fn repo_ref_from_repo_url(input: &str) -> Option<String> {
    parse_github_repo_ref(input)
        .ok()
        .map(|repo| repo.repo_ref())
}

/// Extract owner/repo from a GitHub URL
#[cfg(test)]
pub fn parse_github_owner_repo(url: &str) -> Option<(String, String)> {
    parse_github_repo_ref(url)
        .ok()
        .map(|repo| (repo.owner, repo.repo))
}

/// Check if enough time has passed since the last check
pub fn should_check(cache_entry: Option<&UpdateCheckEntry>) -> bool {
    match cache_entry {
        None => true,
        Some(entry) => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now.saturating_sub(entry.last_checked) > CHECK_COOLDOWN_SECS
        }
    }
}

/// Check a single skill for updates by querying the GitHub API.
pub async fn check_github_update(repo_url: &str, local_ref: Option<&str>) -> UpdateCheckResult {
    let repo = match parse_github_repo_ref(repo_url) {
        Ok(parsed) => parsed,
        Err(message) => {
            return UpdateCheckResult {
                update_available: false,
                remote_ref: None,
                error: Some(message),
                error_kind: Some(UpdateErrorKind::InvalidRepoUrl),
                canonical_repo_url: None,
            }
        }
    };

    // GitHub API: latest commit on default branch
    let api_url = format!(
        "https://api.github.com/repos/{}/{}/commits?per_page=1",
        repo.owner, repo.repo
    );

    let client = match reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            return UpdateCheckResult {
                update_available: false,
                remote_ref: None,
                error: Some(format!("Failed to build HTTP client: {}", e)),
                error_kind: Some(UpdateErrorKind::ProviderError),
                canonical_repo_url: Some(repo.canonical_url()),
            }
        }
    };

    let response = match client
        .get(&api_url)
        .header("User-Agent", "SkillDeck/0.1")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            return UpdateCheckResult {
                update_available: false,
                remote_ref: None,
                error: Some(format!("HTTP request failed: {}", e)),
                error_kind: Some(UpdateErrorKind::Network),
                canonical_repo_url: Some(repo.canonical_url()),
            }
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        let error_kind = match status {
            StatusCode::NOT_FOUND | StatusCode::GONE => UpdateErrorKind::RepoNotFound,
            StatusCode::TOO_MANY_REQUESTS => UpdateErrorKind::RateLimited,
            StatusCode::FORBIDDEN => {
                let is_rate_limited = response
                    .headers()
                    .get("x-ratelimit-remaining")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v == "0")
                    .unwrap_or(false);
                if is_rate_limited {
                    UpdateErrorKind::RateLimited
                } else {
                    UpdateErrorKind::AccessDenied
                }
            }
            StatusCode::UNAUTHORIZED => UpdateErrorKind::AccessDenied,
            _ => UpdateErrorKind::ProviderError,
        };

        return UpdateCheckResult {
            update_available: false,
            remote_ref: None,
            error: Some(format!("GitHub API returned {}", status)),
            error_kind: Some(error_kind),
            canonical_repo_url: Some(repo.canonical_url()),
        };
    }

    let body: serde_json::Value = match response.json().await {
        Ok(json) => json,
        Err(e) => {
            return UpdateCheckResult {
                update_available: false,
                remote_ref: None,
                error: Some(format!("Failed to parse GitHub response: {}", e)),
                error_kind: Some(UpdateErrorKind::InvalidResponse),
                canonical_repo_url: Some(repo.canonical_url()),
            }
        }
    };

    let remote_sha = body
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|commit| commit.get("sha"))
        .and_then(|sha| sha.as_str())
        .map(|s| s.to_string());

    if remote_sha.is_none() {
        return UpdateCheckResult {
            update_available: false,
            remote_ref: None,
            error: Some("GitHub API response did not contain a commit SHA".to_string()),
            error_kind: Some(UpdateErrorKind::InvalidResponse),
            canonical_repo_url: Some(repo.canonical_url()),
        };
    }

    let update_available = match compare_refs(remote_sha.as_deref(), local_ref) {
        UpdateComparison::Different => true,
        UpdateComparison::Same | UpdateComparison::Unknown => false,
    };

    UpdateCheckResult {
        update_available,
        remote_ref: remote_sha,
        error: None,
        error_kind: None,
        canonical_repo_url: Some(repo.canonical_url()),
    }
}

pub fn compare_refs(remote_ref: Option<&str>, local_ref: Option<&str>) -> UpdateComparison {
    match (remote_ref, local_ref) {
        (Some(remote), Some(local)) if remote == local => UpdateComparison::Same,
        (Some(_), Some(_)) => UpdateComparison::Different,
        _ => UpdateComparison::Unknown,
    }
}

/// Build a new cache entry from a check result
pub fn make_cache_entry(result: &UpdateCheckResult) -> UpdateCheckEntry {
    UpdateCheckEntry {
        last_checked: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        update_available: result.update_available,
        remote_ref: result.remote_ref.clone(),
        repo_ref: None,
        last_error: result.error.clone(),
        last_error_kind: result.error_kind.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_owner_repo_https() {
        let result = parse_github_owner_repo("https://github.com/OthmanAdi/vibe-skills");
        assert_eq!(
            result,
            Some(("OthmanAdi".to_string(), "vibe-skills".to_string()))
        );
    }

    #[test]
    fn test_parse_github_owner_repo_rejects_sponsor_path() {
        let result = parse_github_owner_repo("https://github.com/sponsors/safishamsi");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_github_owner_repo_rejects_marketplace_path() {
        let result = parse_github_owner_repo("https://github.com/marketplace/actions/setup-node");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_github_owner_repo_rejects_shorthand_for_site_path() {
        // Shorthand "sponsors/foo" is still a site path, not a repo.
        let result = parse_github_owner_repo("sponsors/safishamsi");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_github_owner_repo_with_git() {
        let result = parse_github_owner_repo("https://github.com/owner/repo.git");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    #[test]
    fn test_parse_github_owner_repo_with_path() {
        let result = parse_github_owner_repo("https://github.com/owner/repo/tree/main");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    #[test]
    fn test_parse_github_owner_repo_shorthand() {
        let result = parse_github_owner_repo("owner/repo");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    #[test]
    fn test_parse_github_owner_repo_without_scheme() {
        let result = parse_github_owner_repo("github.com/owner/repo");
        assert_eq!(result, Some(("owner".to_string(), "repo".to_string())));
    }

    #[test]
    fn test_parse_github_owner_repo_invalid() {
        let result = parse_github_owner_repo("https://example.com/not-github");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_github_owner_repo_rejects_non_https_scheme() {
        let result = parse_github_owner_repo("http://github.com/owner/repo");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_github_owner_repo_rejects_javascript_scheme() {
        let result = parse_github_owner_repo("javascript:alert('xss')");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_github_owner_repo_rejects_lookalike_host() {
        let result = parse_github_owner_repo("https://evilgithub.com/owner/repo");
        assert_eq!(result, None);
    }

    #[test]
    fn test_canonicalize_github_repo_url() {
        let result = canonicalize_github_repo_url("https://www.github.com/owner/repo/tree/main");
        assert_eq!(result.as_deref(), Some("https://github.com/owner/repo"));
    }

    #[test]
    fn test_repo_ref_from_repo_url() {
        let result = repo_ref_from_repo_url("owner/repo");
        assert_eq!(result.as_deref(), Some("github:owner/repo"));
    }

    #[test]
    fn test_should_check_no_cache() {
        assert!(should_check(None));
    }

    #[test]
    fn test_should_check_fresh_cache() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let entry = UpdateCheckEntry {
            last_checked: now,
            update_available: false,
            remote_ref: None,
            repo_ref: None,
            last_error: None,
            last_error_kind: None,
        };
        assert!(!should_check(Some(&entry)));
    }

    #[test]
    fn test_should_check_stale_cache() {
        let old = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - CHECK_COOLDOWN_SECS
            - 1;
        let entry = UpdateCheckEntry {
            last_checked: old,
            update_available: false,
            remote_ref: None,
            repo_ref: None,
            last_error: None,
            last_error_kind: None,
        };
        assert!(should_check(Some(&entry)));
    }

    #[test]
    fn test_compare_refs_same() {
        assert!(matches!(
            compare_refs(Some("abc"), Some("abc")),
            UpdateComparison::Same
        ));
    }

    #[test]
    fn test_compare_refs_different() {
        assert!(matches!(
            compare_refs(Some("abc"), Some("def")),
            UpdateComparison::Different
        ));
    }

    #[test]
    fn test_compare_refs_unknown() {
        assert!(matches!(
            compare_refs(Some("abc"), None),
            UpdateComparison::Unknown
        ));
    }
}
