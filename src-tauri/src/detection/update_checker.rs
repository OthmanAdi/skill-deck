// @agent-context: Update checker for skills with detected GitHub repository URLs.
//
// STRATEGY:
// 1. Parse the repo URL to extract owner/repo
// 2. Call GitHub API: GET /repos/{owner}/{repo}/commits?per_page=1&sha=HEAD
// 3. Compare the remote SHA against a cached previous remote reference
// 4. Cache results to avoid hammering the API (max 1 check per skill per hour)
//
// RATE LIMITS:
// - GitHub unauthenticated: 60 requests/hour per IP
// - We limit to 1 check per skill per hour via UpdateCheckEntry cache

use regex::Regex;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::UpdateCheckEntry;

/// Minimum seconds between update checks for the same skill
const CHECK_COOLDOWN_SECS: u64 = 3600; // 1 hour

/// Result of an update check
#[derive(Debug, Clone)]
pub struct UpdateCheckResult {
    pub update_available: bool,
    pub remote_ref: Option<String>,
    pub error: Option<String>,
}

pub enum UpdateComparison {
    Same,
    Different,
    Unknown,
}

fn github_owner_repo_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"github\.com/([\w.\-]+)/([\w.\-]+?)(?:\.git)?(?:[/#]|$)")
            .expect("github owner/repo regex")
    })
}

/// Extract owner/repo from a GitHub URL
pub fn parse_github_owner_repo(url: &str) -> Option<(String, String)> {
    github_owner_repo_re().captures(url).and_then(|caps| {
        let owner = caps.get(1)?.as_str().to_string();
        let repo = caps.get(2)?.as_str().to_string();
        Some((owner, repo))
    })
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
            now - entry.last_checked > CHECK_COOLDOWN_SECS
        }
    }
}

/// Check a single skill for updates by querying the GitHub API.
///
/// This is async because it makes HTTP requests.
/// Returns UpdateCheckResult with the check outcome.
pub async fn check_github_update(
    repo_url: &str,
    local_ref: Option<&str>,
) -> UpdateCheckResult {
    let (owner, repo) = match parse_github_owner_repo(repo_url) {
        Some(pair) => pair,
        None => return UpdateCheckResult {
            update_available: false,
            remote_ref: None,
            error: Some(format!("Cannot parse GitHub URL: {}", repo_url)),
        },
    };

    // GitHub API: get latest commit on default branch
    let api_url = format!(
        "https://api.github.com/repos/{}/{}/commits?per_page=1",
        owner, repo
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
            };
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
        Err(e) => return UpdateCheckResult {
            update_available: false,
            remote_ref: None,
            error: Some(format!("HTTP request failed: {}", e)),
        },
    };

    if !response.status().is_success() {
        return UpdateCheckResult {
            update_available: false,
            remote_ref: None,
            error: Some(format!("GitHub API returned {}", response.status())),
        };
    }

    let body: serde_json::Value = match response.json().await {
        Ok(json) => json,
        Err(e) => return UpdateCheckResult {
            update_available: false,
            remote_ref: None,
            error: Some(format!("Failed to parse GitHub response: {}", e)),
        },
    };

    // Extract the latest commit SHA
    let remote_sha = body
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|commit| commit.get("sha"))
        .and_then(|sha| sha.as_str())
        .map(|s| s.to_string());

    let update_available = match compare_refs(remote_sha.as_deref(), local_ref) {
        UpdateComparison::Different => true,
        UpdateComparison::Same | UpdateComparison::Unknown => false,
    };

    UpdateCheckResult {
        update_available,
        remote_ref: remote_sha,
        error: None,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_owner_repo_https() {
        let result = parse_github_owner_repo("https://github.com/OthmanAdi/vibe-skills");
        assert_eq!(result, Some(("OthmanAdi".to_string(), "vibe-skills".to_string())));
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
    fn test_parse_github_owner_repo_invalid() {
        let result = parse_github_owner_repo("https://example.com/not-github");
        assert_eq!(result, None);
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
        };
        assert!(!should_check(Some(&entry)));
    }

    #[test]
    fn test_should_check_stale_cache() {
        let old = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() - CHECK_COOLDOWN_SECS - 1;
        let entry = UpdateCheckEntry {
            last_checked: old,
            update_available: false,
            remote_ref: None,
            repo_ref: None,
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
