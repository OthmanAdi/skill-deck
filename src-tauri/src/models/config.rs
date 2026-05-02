// @agent-context: App configuration model.
// Stored as JSON in the OS app-data directory (e.g., %APPDATA%/skill-deck/config.json).
// Contains user preferences: starred skills, custom hotkey, theme, scan paths.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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

    /// User-overridden repository URLs per skill ID
    pub skill_repo_overrides: std::collections::HashMap<String, String>,

    /// User-overridden install commands per skill ID
    pub skill_install_overrides: std::collections::HashMap<String, String>,

    /// Cache: last update check timestamps per skill ID (unix epoch seconds)
    pub update_check_cache: std::collections::HashMap<String, UpdateCheckEntry>,
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
            skill_repo_overrides: std::collections::HashMap::new(),
            skill_install_overrides: std::collections::HashMap::new(),
            update_check_cache: std::collections::HashMap::new(),
        }
    }
}

fn default_overlay_width() -> u32 {
    460
}

fn default_overlay_height() -> u32 {
    640
}
