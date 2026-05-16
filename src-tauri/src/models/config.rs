// @agent-context: App configuration model.
// Stored as JSON in the OS app-data directory (e.g., %APPDATA%/skill-deck/config.json).
// Contains user preferences: starred skills, custom hotkey, theme, scan paths.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Structured update check error categories for frontend UX and caching.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UpdateErrorKind {
    InvalidRepoUrl,
    RepoNotFound,
    RateLimited,
    AccessDenied,
    Network,
    InvalidResponse,
    ProviderError,
}

/// Persisted install/discovery timestamp for a skill identity key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstallEntry {
    /// Best-effort unix timestamp for when the skill first appeared locally.
    pub installed_at: u64,
}

/// Persisted app configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// Global keyboard shortcut to toggle the overlay
    /// Default: "CommandOrControl+Shift+K"
    pub hotkey: String,

    /// Set of skill IDs the user has starred
    pub starred_skills: HashSet<String>,

    /// Custom icon assignments: skill_id -> emoji or image path
    pub skill_icons: std::collections::HashMap<String, String>,

    /// Additional directories to scan for skills (beyond auto-detected agent paths)
    pub custom_scan_paths: Vec<String>,

    /// Whether to check for skill updates in the background
    pub check_updates: bool,

    /// Theme mode: "system" (default), "dark", or "light"
    pub theme: String,

    /// Overlay position preference
    pub overlay_position: OverlayPosition,

    /// Persisted overlay width in logical pixels
    #[serde(default = "default_overlay_width")]
    pub overlay_width: u32,

    /// Persisted overlay height in logical pixels
    #[serde(default = "default_overlay_height")]
    pub overlay_height: u32,

    /// Overlay interaction mode: "pinned" or "auto-hide"
    #[serde(default = "default_overlay_mode")]
    pub overlay_mode: String,

    /// Persisted state for Finder panel visibility
    #[serde(default)]
    pub finder_open: bool,

    /// Sort mode for skill lists.
    #[serde(default = "default_skill_sort_mode")]
    pub skill_sort_mode: String,

    /// User-overridden repository URLs per skill ID
    pub skill_repo_overrides: std::collections::HashMap<String, String>,

    /// User-overridden install commands per skill ID
    pub skill_install_overrides: std::collections::HashMap<String, String>,

    /// Cache: last update check timestamps per skill ID (unix epoch seconds)
    pub update_check_cache: std::collections::HashMap<String, UpdateCheckEntry>,

    /// Archived local version history entries per skill ID
    #[serde(default)]
    pub skill_version_history: std::collections::HashMap<String, Vec<SkillVersionEntry>>,

    /// Stable install/discovery timestamps keyed by skill identity fingerprint.
    #[serde(default)]
    pub skill_install_index: std::collections::HashMap<String, SkillInstallEntry>,

    /// Maximum number of history entries to retain per skill
    #[serde(default = "default_max_skill_history_entries")]
    pub max_skill_history_entries: usize,

    /// Collapsed agent group ids in grouped list view
    #[serde(default)]
    pub collapsed_agents: HashSet<String>,

    /// Collapsed skill node ids in tree view
    #[serde(default)]
    pub collapsed_tree_nodes: HashSet<String>,

    /// Global UI font scale (CSS `zoom` applied to the document root).
    /// Clamped to [1.0, 2.0] on persist.
    #[serde(default = "default_font_scale")]
    pub font_scale: f64,
}

/// Cached result of an update check for a single skill
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckEntry {
    /// Unix timestamp of last check
    pub last_checked: u64,
    /// Whether an update was available at last check
    pub update_available: bool,
    /// Remote commit SHA or version string at last check
    #[serde(default)]
    pub remote_ref: Option<String>,
    /// Repo identity used for cache validity (e.g., github:owner/repo)
    #[serde(default)]
    pub repo_ref: Option<String>,

    /// Last check error text, if any
    #[serde(default)]
    pub last_error: Option<String>,

    /// Last check error category, if any
    #[serde(default)]
    pub last_error_kind: Option<UpdateErrorKind>,
}

/// One archived skill content version stored by the application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillVersionEntry {
    /// Unique version identifier for this snapshot
    pub version_id: String,

    /// Unix timestamp when this snapshot was created
    pub created_at: u64,

    /// Human readable reason, e.g. before-update, after-update, before-restore
    pub reason: String,

    /// Optional canonical source repository URL
    #[serde(default)]
    pub source_repo_url: Option<String>,

    /// Optional remote commit SHA or ref associated with this snapshot
    #[serde(default)]
    pub remote_ref: Option<String>,

    /// SHA-256 hash of the snapshot content
    pub content_hash: String,

    /// Byte count of the snapshot content
    pub content_bytes: u64,

    /// Relative path to snapshot JSON under history storage root
    pub snapshot_path: String,
}

/// Where the overlay appears on screen
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayPosition {
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            hotkey: "CommandOrControl+Shift+K".to_string(),
            starred_skills: HashSet::new(),
            skill_icons: std::collections::HashMap::new(),
            custom_scan_paths: Vec::new(),
            check_updates: true,
            theme: "system".to_string(),
            overlay_position: OverlayPosition::BottomRight,
            overlay_width: default_overlay_width(),
            overlay_height: default_overlay_height(),
            overlay_mode: default_overlay_mode(),
            finder_open: false,
            skill_sort_mode: default_skill_sort_mode(),
            skill_repo_overrides: std::collections::HashMap::new(),
            skill_install_overrides: std::collections::HashMap::new(),
            update_check_cache: std::collections::HashMap::new(),
            skill_version_history: std::collections::HashMap::new(),
            skill_install_index: std::collections::HashMap::new(),
            max_skill_history_entries: default_max_skill_history_entries(),
            collapsed_agents: HashSet::new(),
            collapsed_tree_nodes: HashSet::new(),
            font_scale: default_font_scale(),
        }
    }
}

fn default_font_scale() -> f64 {
    1.0
}

fn default_overlay_width() -> u32 {
    460
}

fn default_overlay_height() -> u32 {
    640
}

fn default_overlay_mode() -> String {
    "pinned".to_string()
}

fn default_skill_sort_mode() -> String {
    "default".to_string()
}

fn default_max_skill_history_entries() -> usize {
    40
}
