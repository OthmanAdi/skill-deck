// @agent-context: Detection module — platform-specific and content-analysis engines.
//
// MODULES:
// - repo_detector: GitHub/GitLab URL and npx install command detection
// - update_checker: HTTP-based version comparison via GitHub API
// - window_at_point: Native OS window-under-cursor detection
// - terminal_inject: Inject content into terminal windows via clipboard+paste

pub mod repo_detector;
pub mod update_checker;
pub mod window_at_point;
pub mod terminal_inject;
