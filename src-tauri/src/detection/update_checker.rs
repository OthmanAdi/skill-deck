// @agent-context: Update checker for skills with detected GitHub repository URLs.
//
// STRATEGY:
// 1. Parse the repo URL to extract owner/repo
// 2. Call GitHub API: GET /repos/{owner}/{repo}/commits?per_page=1&sha=HEAD
// 3. Compare the remote SHA against a local reference (file content hash)
// 4. Cache results to avoid hammering the API (max 1 check per skill per hour)
//
// RATE LIMITS:
// - GitHub unauthenticated: 60 requests/hour per IP
// - We limit to 1 check per skill per hour via UpdateCheckEntry cache

use regex::Regex;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

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

/// Compute a simple hash of a file's content for local reference
pub fn file_content_hash(file_path: &Path) -> Option<String> {
    let content = std::fs::read(file_path).ok()?;
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    Some(format!("{:016x}", hasher.finish()))
}

/// Check a single skill for updates by querying the GitHub API.
///
/// This is async because it makes HTTP requests.
/// Returns UpdateCheckResult with the check outcome.
pub async fn check_github_update(
    repo_url: &str,
    local_hash: Option<&str>,
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

    let client = reqwest::Client::new();
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

    let update_available = match (&remote_sha, local_hash) {
        (Some(remote), Some(local)) => remote != local,
        (Some(_), None) => false, // No local reference to compare — assume up to date
        _ => false,
    };

    UpdateCheckResult {
        update_available,
        remote_ref: remote_sha,
        error: None,
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
        };
        assert!(should_check(Some(&entry)));
    }

    #[test]
    fn test_file_content_hash_deterministic() {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"test content").unwrap();
        let h1 = file_content_hash(f.path());
        let h2 = file_content_hash(f.path());
        assert!(h1.is_some());
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_file_content_hash_different_content() {
        use std::io::Write;
        let mut f1 = tempfile::NamedTempFile::new().unwrap();
        f1.write_all(b"content A").unwrap();
        let mut f2 = tempfile::NamedTempFile::new().unwrap();
        f2.write_all(b"content B").unwrap();
        let h1 = file_content_hash(f1.path());
        let h2 = file_content_hash(f2.path());
        assert_ne!(h1, h2);
    }
}
