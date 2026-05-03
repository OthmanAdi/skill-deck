// @agent-context: Detection module — platform-specific and content-analysis engines.
//
// MODULES:
// - repo_detector: GitHub/GitLab URL and npx install command detection
// - skill_discovery: discovery tags and use-case enrichment for faceted filtering
// - update_checker: HTTP-based version comparison via GitHub API
// - skill_history: local snapshots for restore workflows

pub mod repo_detector;
pub mod skill_discovery;
pub mod skill_history;
pub mod update_checker;
