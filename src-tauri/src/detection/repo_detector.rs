// @agent-context: Repo and install command detection engine.
// Scans skill files for GitHub repository URLs and npx install commands
// using a priority chain: frontmatter → body regex → .git/config → user override.
//
// This module is called after parsing to enrich SkillMetadata with:
// - repository_url: GitHub/GitLab/etc. repo URL
// - install_command: npx/npm install one-liner

use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

/// Result of repo/install detection for a single skill
#[derive(Debug, Clone, Default)]
pub struct DetectedSource {
    pub repository_url: Option<String>,
    pub install_command: Option<String>,
}

// ── Compiled regex patterns (compiled once, reused across calls) ─────────────

fn github_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"https?://github\.com/([\w.\-]+)/([\w.\-]+?)(?:\.git)?(?:[/#\s\)]|$)")
            .expect("github URL regex")
    })
}

fn gitlab_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"https?://gitlab\.com/([\w.\-]+(?:/[\w.\-]+)*)/([\w.\-]+?)(?:\.git)?(?:[/#\s\)]|$)",
        )
        .expect("gitlab URL regex")
    })
}

fn npx_skills_add_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"npx\s+skills\s+add\s+([\w.\-]+/[\w.\-]+)(?:\s+--skill\s+([\w.\-]+))?(?:\s+-g)?",
        )
        .expect("npx skills add regex")
    })
}

fn npm_install_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"npm\s+install\s+(?:-g\s+)?([\w.\-@/]+)").expect("npm install regex")
    })
}

fn git_remote_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"url\s*=\s*(.+)"#).expect("git remote url regex"))
}

// First path segments under github.com that are NOT a repository — these are
// site routes (sponsors, marketplace, org listings, search, etc.). We exclude
// them so a "sponsor me" link in the body of a SKILL.md doesn't get scraped
// as `github.com/sponsors/<user>` and then 404 on every update check.
const GITHUB_RESERVED_FIRST_SEGMENTS: &[&str] = &[
    "sponsors",
    "orgs",
    "organizations",
    "marketplace",
    "topics",
    "features",
    "pricing",
    "about",
    "login",
    "signup",
    "join",
    "settings",
    "explore",
    "trending",
    "collections",
    "events",
    "enterprise",
    "security",
    "pulls",
    "issues",
    "notifications",
    "search",
    "stars",
    "watching",
    "new",
    "site",
    "contact",
    "readme",
    "discussions",
    "advisories",
    "codespaces",
    "copilot",
    "apps",
    "github-copilot",
];

pub(crate) fn github_first_segment_is_repo(segment: &str) -> bool {
    let lower = segment.to_ascii_lowercase();
    !GITHUB_RESERVED_FIRST_SEGMENTS.contains(&lower.as_str())
}

// ── YAML frontmatter keys that may contain a repo URL ───────────────────────

const REPO_FRONTMATTER_KEYS: &[&str] = &[
    "repository",
    "repo",
    "source",
    "homepage",
    "url",
    "github",
    "git",
];

const INSTALL_FRONTMATTER_KEYS: &[&str] = &["install", "install-command", "installCommand"];

// ── Public API ──────────────────────────────────────────────────────────────

/// Run the full detection chain on a skill's content and file path.
///
/// Priority: frontmatter fields → body regex → .git/config in parent dir
pub fn detect_sources(
    frontmatter: Option<&serde_yaml::Value>,
    body: &str,
    file_path: &Path,
) -> DetectedSource {
    let mut result = DetectedSource::default();

    // 1. Check frontmatter for repo URL
    if let Some(fm) = frontmatter {
        result.repository_url = detect_repo_from_frontmatter(fm);
        if result.install_command.is_none() {
            result.install_command = detect_install_from_frontmatter(fm);
        }
    }

    // 2. Scan body for GitHub/GitLab URLs
    if result.repository_url.is_none() {
        result.repository_url = detect_repo_from_body(body);
    }

    // 3. Scan body for npx/npm install commands
    if result.install_command.is_none() {
        result.install_command = detect_install_from_body(body);
    }

    // 4. Check .git/config in skill's parent directory
    if result.repository_url.is_none() {
        result.repository_url = detect_repo_from_git_config(file_path);
    }

    result
}

/// Apply user overrides from AppConfig on top of detected sources.
pub fn apply_overrides(
    detected: &mut DetectedSource,
    skill_id: &str,
    repo_overrides: &std::collections::HashMap<String, String>,
    install_overrides: &std::collections::HashMap<String, String>,
) {
    if let Some(url) = repo_overrides.get(skill_id) {
        detected.repository_url = Some(url.clone());
    }
    if let Some(cmd) = install_overrides.get(skill_id) {
        detected.install_command = Some(cmd.clone());
    }
}

// ── Frontmatter detection ───────────────────────────────────────────────────

fn detect_repo_from_frontmatter(fm: &serde_yaml::Value) -> Option<String> {
    for key in REPO_FRONTMATTER_KEYS {
        if let Some(val) = fm.get(*key).and_then(|v| v.as_str()) {
            let trimmed = val.trim();
            // Accept if it looks like a URL
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                return Some(trimmed.to_string());
            }
            // Accept GitHub shorthand: "owner/repo"
            if is_github_shorthand(trimmed) {
                return Some(format!("https://github.com/{}", trimmed));
            }
        }
    }
    None
}

fn detect_install_from_frontmatter(fm: &serde_yaml::Value) -> Option<String> {
    for key in INSTALL_FRONTMATTER_KEYS {
        if let Some(val) = fm.get(*key).and_then(|v| v.as_str()) {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

// ── Body content detection ──────────────────────────────────────────────────

fn detect_repo_from_body(body: &str) -> Option<String> {
    // Try GitHub first (most common). Skip matches whose first path segment is
    // a known site route (sponsors / marketplace / etc.) so they don't get
    // saved as a fake repo URL.
    for caps in github_url_re().captures_iter(body) {
        let owner = caps.get(1)?.as_str();
        let repo = caps.get(2)?.as_str();
        if !github_first_segment_is_repo(owner) {
            continue;
        }
        return Some(format!("https://github.com/{}/{}", owner, repo));
    }
    // Try GitLab
    if let Some(caps) = gitlab_url_re().captures(body) {
        let owner = caps.get(1)?.as_str();
        let repo = caps.get(2)?.as_str();
        return Some(format!("https://gitlab.com/{}/{}", owner, repo));
    }
    None
}

fn detect_install_from_body(body: &str) -> Option<String> {
    // Try `npx skills add` first (most specific)
    if let Some(caps) = npx_skills_add_re().captures(body) {
        return Some(caps.get(0)?.as_str().trim().to_string());
    }
    // Try `npm install`
    if let Some(caps) = npm_install_re().captures(body) {
        return Some(caps.get(0)?.as_str().trim().to_string());
    }
    None
}

// ── .git/config detection ───────────────────────────────────────────────────

fn detect_repo_from_git_config(file_path: &Path) -> Option<String> {
    // Walk up from the skill file to find a .git/config
    let mut dir = file_path.parent()?;

    // Check up to 3 levels (skill file → skill dir → skills dir → agent dir)
    for _ in 0..3 {
        let git_config = dir.join(".git").join("config");
        if git_config.exists() {
            if let Ok(content) = std::fs::read_to_string(&git_config) {
                return parse_git_remote_url(&content);
            }
        }
        dir = dir.parent()?;
    }
    None
}

fn parse_git_remote_url(git_config_content: &str) -> Option<String> {
    let mut in_origin = false;
    for line in git_config_content.lines() {
        let trimmed = line.trim();
        if trimmed == "[remote \"origin\"]" {
            in_origin = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_origin = false;
            continue;
        }
        if in_origin {
            if let Some(caps) = git_remote_url_re().captures(trimmed) {
                let url = caps.get(1)?.as_str().trim();
                // Convert SSH URLs to HTTPS
                if url.starts_with("git@github.com:") {
                    let path = url
                        .strip_prefix("git@github.com:")?
                        .trim_end_matches(".git");
                    return Some(format!("https://github.com/{}", path));
                }
                if url.starts_with("http://") || url.starts_with("https://") {
                    return Some(url.trim_end_matches(".git").to_string());
                }
            }
        }
    }
    None
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Check if a string looks like a GitHub shorthand: "owner/repo"
fn is_github_shorthand(s: &str) -> bool {
    let parts: Vec<&str> = s.split('/').collect();
    parts.len() == 2
        && !parts[0].is_empty()
        && !parts[1].is_empty()
        && !s.contains(' ')
        && !s.contains(':')
        && !s.starts_with('/')
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── GitHub URL regex ────────────────────────────────────────────────

    #[test]
    fn test_github_url_https() {
        let body = "Check https://github.com/OthmanAdi/vibe-skills for details";
        let result = detect_repo_from_body(body);
        assert_eq!(
            result,
            Some("https://github.com/OthmanAdi/vibe-skills".to_string())
        );
    }

    #[test]
    fn test_github_url_with_git_suffix() {
        let body = "Clone from https://github.com/user/repo.git to get started";
        let result = detect_repo_from_body(body);
        assert_eq!(result, Some("https://github.com/user/repo".to_string()));
    }

    #[test]
    fn test_github_url_with_subpath_ignored() {
        let body = "See https://github.com/owner/repo/tree/main/docs for docs";
        let result = detect_repo_from_body(body);
        assert_eq!(result, Some("https://github.com/owner/repo".to_string()));
    }

    #[test]
    fn test_github_url_in_parentheses() {
        let body = "Source: (https://github.com/foo/bar)";
        let result = detect_repo_from_body(body);
        assert_eq!(result, Some("https://github.com/foo/bar".to_string()));
    }

    #[test]
    fn test_no_github_url() {
        let body = "This is a plain skill with no links.";
        let result = detect_repo_from_body(body);
        assert_eq!(result, None);
    }

    #[test]
    fn test_github_sponsor_url_is_ignored() {
        let body = "Support me at https://github.com/sponsors/safishamsi";
        let result = detect_repo_from_body(body);
        assert_eq!(result, None);
    }

    #[test]
    fn test_github_marketplace_url_is_ignored() {
        let body = "See https://github.com/marketplace/actions/foo";
        let result = detect_repo_from_body(body);
        assert_eq!(result, None);
    }

    #[test]
    fn test_github_repo_after_sponsor_link_wins() {
        let body = "Sponsor at https://github.com/sponsors/me. Repo at https://github.com/me/cool-skill";
        let result = detect_repo_from_body(body);
        assert_eq!(result, Some("https://github.com/me/cool-skill".to_string()));
    }

    #[test]
    fn test_gitlab_url() {
        let body = "Hosted at https://gitlab.com/group/project for CI";
        let result = detect_repo_from_body(body);
        assert_eq!(result, Some("https://gitlab.com/group/project".to_string()));
    }

    // ── npx skills add regex ────────────────────────────────────────────

    #[test]
    fn test_npx_skills_add_basic() {
        let body = "Install: `npx skills add OthmanAdi/vibe-skills --skill vibe -g`";
        let result = detect_install_from_body(body);
        assert_eq!(
            result,
            Some("npx skills add OthmanAdi/vibe-skills --skill vibe -g".to_string())
        );
    }

    #[test]
    fn test_npx_skills_add_no_skill_flag() {
        let body = "Run `npx skills add owner/repo` to install";
        let result = detect_install_from_body(body);
        assert_eq!(result, Some("npx skills add owner/repo".to_string()));
    }

    #[test]
    fn test_npm_install_global() {
        let body = "Install globally: `npm install -g @scope/package`";
        let result = detect_install_from_body(body);
        assert_eq!(result, Some("npm install -g @scope/package".to_string()));
    }

    #[test]
    fn test_no_install_command() {
        let body = "Just a plain markdown file.";
        let result = detect_install_from_body(body);
        assert_eq!(result, None);
    }

    // ── Frontmatter detection ───────────────────────────────────────────

    #[test]
    fn test_frontmatter_repository_url() {
        let fm: serde_yaml::Value =
            serde_yaml::from_str("repository: https://github.com/owner/repo").unwrap();
        let result = detect_repo_from_frontmatter(&fm);
        assert_eq!(result, Some("https://github.com/owner/repo".to_string()));
    }

    #[test]
    fn test_frontmatter_github_shorthand() {
        let fm: serde_yaml::Value = serde_yaml::from_str("repo: OthmanAdi/vibe-skills").unwrap();
        let result = detect_repo_from_frontmatter(&fm);
        assert_eq!(
            result,
            Some("https://github.com/OthmanAdi/vibe-skills".to_string())
        );
    }

    #[test]
    fn test_frontmatter_install_command() {
        let fm: serde_yaml::Value =
            serde_yaml::from_str("install: npx skills add owner/repo --skill test -g").unwrap();
        let result = detect_install_from_frontmatter(&fm);
        assert_eq!(
            result,
            Some("npx skills add owner/repo --skill test -g".to_string())
        );
    }

    #[test]
    fn test_frontmatter_no_repo() {
        let fm: serde_yaml::Value =
            serde_yaml::from_str("name: test-skill\ndescription: just a test").unwrap();
        let result = detect_repo_from_frontmatter(&fm);
        assert_eq!(result, None);
    }

    // ── .git/config parsing ─────────────────────────────────────────────

    #[test]
    fn test_git_config_https_origin() {
        let config = r#"[core]
    repositoryformatversion = 0
[remote "origin"]
    url = https://github.com/OthmanAdi/skill-deck.git
    fetch = +refs/heads/*:refs/remotes/origin/*
[branch "master"]
    remote = origin
"#;
        let result = parse_git_remote_url(config);
        assert_eq!(
            result,
            Some("https://github.com/OthmanAdi/skill-deck".to_string())
        );
    }

    #[test]
    fn test_git_config_ssh_origin() {
        let config = r#"[remote "origin"]
    url = git@github.com:OthmanAdi/skill-deck.git
    fetch = +refs/heads/*:refs/remotes/origin/*
"#;
        let result = parse_git_remote_url(config);
        assert_eq!(
            result,
            Some("https://github.com/OthmanAdi/skill-deck".to_string())
        );
    }

    #[test]
    fn test_git_config_no_origin() {
        let config = r#"[core]
    repositoryformatversion = 0
"#;
        let result = parse_git_remote_url(config);
        assert_eq!(result, None);
    }

    // ── Full detection chain ────────────────────────────────────────────

    #[test]
    fn test_detect_sources_frontmatter_wins() {
        let fm: serde_yaml::Value =
            serde_yaml::from_str("repository: https://github.com/owner/from-frontmatter").unwrap();
        let body = "Also see https://github.com/owner/from-body for reference";
        let result = detect_sources(Some(&fm), body, Path::new("/tmp/fake/SKILL.md"));
        assert_eq!(
            result.repository_url,
            Some("https://github.com/owner/from-frontmatter".to_string())
        );
    }

    #[test]
    fn test_detect_sources_body_fallback() {
        let fm: serde_yaml::Value = serde_yaml::from_str("name: no-repo-here").unwrap();
        let body = "Install: npx skills add owner/cool-skill --skill test -g\nRepo: https://github.com/owner/cool-skill";
        let result = detect_sources(Some(&fm), body, Path::new("/tmp/fake/SKILL.md"));
        assert_eq!(
            result.repository_url,
            Some("https://github.com/owner/cool-skill".to_string())
        );
        assert_eq!(
            result.install_command,
            Some("npx skills add owner/cool-skill --skill test -g".to_string())
        );
    }

    // ── GitHub shorthand ────────────────────────────────────────────────

    #[test]
    fn test_is_github_shorthand_valid() {
        assert!(is_github_shorthand("owner/repo"));
        assert!(is_github_shorthand("OthmanAdi/vibe-skills"));
        assert!(is_github_shorthand("foo-bar/baz.js"));
    }

    #[test]
    fn test_is_github_shorthand_invalid() {
        assert!(!is_github_shorthand("https://github.com/owner/repo"));
        assert!(!is_github_shorthand("just-a-name"));
        assert!(!is_github_shorthand("/leading/slash"));
        assert!(!is_github_shorthand("has space/repo"));
        assert!(!is_github_shorthand(""));
    }

    // ── User overrides ──────────────────────────────────────────────────

    #[test]
    fn test_apply_overrides() {
        let mut detected = DetectedSource {
            repository_url: Some("https://github.com/auto/detected".to_string()),
            install_command: None,
        };
        let mut repo_overrides = std::collections::HashMap::new();
        repo_overrides.insert(
            "skill-1".to_string(),
            "https://github.com/user/override".to_string(),
        );
        let mut install_overrides = std::collections::HashMap::new();
        install_overrides.insert(
            "skill-1".to_string(),
            "npx skills add user/override -g".to_string(),
        );

        apply_overrides(
            &mut detected,
            "skill-1",
            &repo_overrides,
            &install_overrides,
        );
        assert_eq!(
            detected.repository_url,
            Some("https://github.com/user/override".to_string())
        );
        assert_eq!(
            detected.install_command,
            Some("npx skills add user/override -g".to_string())
        );
    }

    #[test]
    fn test_apply_overrides_no_match() {
        let mut detected = DetectedSource {
            repository_url: Some("https://github.com/auto/detected".to_string()),
            install_command: None,
        };
        let repo_overrides = std::collections::HashMap::new();
        let install_overrides = std::collections::HashMap::new();

        apply_overrides(
            &mut detected,
            "skill-1",
            &repo_overrides,
            &install_overrides,
        );
        assert_eq!(
            detected.repository_url,
            Some("https://github.com/auto/detected".to_string())
        );
        assert_eq!(detected.install_command, None);
    }
}
