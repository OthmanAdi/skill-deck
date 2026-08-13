// @agent-context: Cross-platform agent location resolution.
//
// Registry data uses stable placeholders instead of baking the current
// machine into the catalog. This module resolves those placeholders locally;
// it never crawls the disk or performs network I/O.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_SETTINGS_BYTES: u64 = 1_048_576;
const MAX_PATH_LIST_BYTES: usize = 32_768;
const MAX_PATH_BYTES: usize = 4_096;
const MAX_PATH_LIST_ENTRIES: usize = 32;
const VSCODE_GLOBAL_STORAGE: &str = "$VSCODE_GLOBAL_STORAGE";
const ROO_CUSTOM_STORAGE: &str = "$ROO_CUSTOM_STORAGE";
const COPILOT_SKILLS_DIRS: &str = "$COPILOT_SKILLS_DIRS";
/// Deliberately narrow opt-in for synthetic screenshot/capture sessions.
///
/// This is not a general home-directory override. When present it replaces
/// *all* discovery roots with one verified fixture root, and every candidate
/// is checked again by the scanner before it is parsed.
pub(crate) const CAPTURE_ROOT_ENV: &str = "SKILL_DECK_CAPTURE_ROOT";
const CAPTURE_ROOT_SENTINEL: &str = ".skill-deck-capture-root";
const CAPTURE_ROOT_SENTINEL_CONTENT: &str = "skill-deck-capture-v1\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CaptureRootMode {
    Normal,
    Capture(CaptureRoot),
    Invalid(String),
}

/// Canonical, non-reparse-point fixture root used only by the screenshot
/// capture contract. The raw path is retained to reject a symlink/junction in
/// any candidate path before its contents are opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CaptureRoot {
    root: PathBuf,
    canonical_root: PathBuf,
}

impl CaptureRoot {
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// A candidate must resolve below the fixture root and must not cross any
    /// symlink or Windows junction on its lexical path. This makes capture
    /// scanning fail closed even when a fixture tree contains a reparse point.
    pub(crate) fn contains_file(&self, candidate: &Path) -> bool {
        let Ok(lexical_metadata) = fs::symlink_metadata(candidate) else {
            return false;
        };
        if lexical_metadata.file_type().is_symlink() {
            return false;
        }
        let Ok(canonical_candidate) = fs::canonicalize(candidate) else {
            return false;
        };
        let Ok(metadata) = fs::metadata(&canonical_candidate) else {
            return false;
        };
        if !metadata.is_file() || !canonical_candidate.starts_with(&self.canonical_root) {
            return false;
        }

        // Win32 globbing may return either the declared `C:\...` spelling or
        // canonical `\\?\C:\...` spelling. Both are safe only after the
        // canonical containment check above; use the matching spelling solely
        // to inspect every lexical descendant for a reparse point.
        let relative = candidate
            .strip_prefix(&self.root)
            .or_else(|_| canonical_candidate.strip_prefix(&self.canonical_root));
        let Ok(relative) = relative else {
            return false;
        };
        let mut current = if candidate.starts_with(&self.root) {
            self.root.clone()
        } else {
            self.canonical_root.clone()
        };
        for component in relative.components() {
            current.push(component.as_os_str());
            let Ok(metadata) = fs::symlink_metadata(&current) else {
                return false;
            };
            if metadata.file_type().is_symlink() {
                return false;
            }
        }
        true
    }
}

pub(crate) fn capture_root_mode_from_process() -> CaptureRootMode {
    let Some(raw_root) = env::var_os(CAPTURE_ROOT_ENV) else {
        return CaptureRootMode::Normal;
    };
    if raw_root.to_string_lossy().trim().is_empty() {
        return CaptureRootMode::Invalid(format!(
            "{CAPTURE_ROOT_ENV} must name an absolute fixture directory"
        ));
    }

    match CaptureRoot::from_declared_path(PathBuf::from(raw_root)) {
        Ok(root) => CaptureRootMode::Capture(root),
        Err(error) => CaptureRootMode::Invalid(error),
    }
}

pub(crate) fn capture_mode_requested() -> bool {
    env::var_os(CAPTURE_ROOT_ENV).is_some()
}

impl CaptureRoot {
    pub(crate) fn from_declared_path(root: PathBuf) -> Result<Self, String> {
        if !root.is_absolute() {
            return Err(format!(
                "{CAPTURE_ROOT_ENV} must name an absolute fixture directory"
            ));
        }
        let metadata = fs::symlink_metadata(&root)
            .map_err(|_| format!("{CAPTURE_ROOT_ENV} fixture directory is unavailable"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(format!(
                "{CAPTURE_ROOT_ENV} must name a real directory, not a symlink or junction"
            ));
        }
        let canonical_root = fs::canonicalize(&root)
            .map_err(|_| format!("{CAPTURE_ROOT_ENV} fixture directory cannot be resolved"))?;

        let sentinel = root.join(CAPTURE_ROOT_SENTINEL);
        let sentinel_metadata = fs::symlink_metadata(&sentinel)
            .map_err(|_| format!("{CAPTURE_ROOT_ENV} requires {CAPTURE_ROOT_SENTINEL}"))?;
        if !sentinel_metadata.file_type().is_file() || sentinel_metadata.file_type().is_symlink() {
            return Err(format!(
                "{CAPTURE_ROOT_SENTINEL} must be a regular sentinel file"
            ));
        }
        let contents = fs::read_to_string(&sentinel)
            .map_err(|_| format!("{CAPTURE_ROOT_SENTINEL} cannot be read"))?;
        if contents != CAPTURE_ROOT_SENTINEL_CONTENT {
            return Err(format!(
                "{CAPTURE_ROOT_SENTINEL} does not declare the capture fixture contract"
            ));
        }
        let canonical_sentinel = fs::canonicalize(&sentinel)
            .map_err(|_| format!("{CAPTURE_ROOT_SENTINEL} cannot be resolved"))?;
        if canonical_sentinel.parent() != Some(canonical_root.as_path()) {
            return Err(format!(
                "{CAPTURE_ROOT_SENTINEL} must remain directly inside the fixture root"
            ));
        }

        Ok(Self {
            root,
            canonical_root,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Windows,
    Macos,
    Linux,
}

impl Platform {
    fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LocationEnvironment {
    platform: Platform,
    pub(crate) home: PathBuf,
    pub(crate) config_home: PathBuf,
    pub(crate) xdg_data_home: PathBuf,
    pub(crate) codex_home: PathBuf,
    pub(crate) claude_home: PathBuf,
    pub(crate) gemini_home: PathBuf,
    pub(crate) copilot_home: PathBuf,
    pub(crate) copilot_skill_dirs: Vec<PathBuf>,
    pub(crate) continue_home: PathBuf,
    pub(crate) cline_data_dir: Option<PathBuf>,
    pub(crate) vibe_home: PathBuf,
    pub(crate) hermes_home: PathBuf,
    pub(crate) autohand_home: PathBuf,
    pub(crate) grok_home: PathBuf,
    pub(crate) app_data: Option<PathBuf>,
    pub(crate) flatpak_config_home: Option<PathBuf>,
    pub(crate) mac_application_support: Option<PathBuf>,
    vscode_global_storage: Vec<PathBuf>,
    roo_custom_storage: Vec<PathBuf>,
}

impl LocationEnvironment {
    pub(crate) fn from_process() -> Self {
        let platform = Platform::current();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let mut environment = Self::from_reader(platform, home, env_path);
        environment.roo_custom_storage =
            discover_roo_custom_storage(&environment.vscode_settings_files(), &environment.home);
        environment
    }

    /// Capture mode deliberately ignores every normal agent-path environment
    /// variable. A synthetic root can only narrow discovery, never redirect a
    /// single adapter back into the real user profile.
    pub(crate) fn for_capture(root: &CaptureRoot) -> Self {
        Self::from_reader(Platform::current(), root.root().to_path_buf(), |_| None)
    }

    #[cfg(test)]
    pub(crate) fn fixture(platform: Platform, home: impl Into<PathBuf>) -> Self {
        let mut environment = Self::from_reader(platform, home.into(), |_| None);
        if platform == Platform::Linux {
            environment.flatpak_config_home = Some(environment.home.join(".var/app/config"));
        }
        environment
    }

    fn from_reader<F>(platform: Platform, home: PathBuf, read_env: F) -> Self
    where
        F: Fn(&str) -> Option<PathBuf>,
    {
        let config_home = read_env("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        // OpenCode and Kilo use the XDG data root even on Windows/macOS.
        // Do not substitute LOCALAPPDATA or Application Support for it.
        let xdg_data_home =
            read_env("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share"));
        let app_data = (platform == Platform::Windows)
            .then(|| read_env("APPDATA").unwrap_or_else(|| home.join("AppData").join("Roaming")));
        let mac_application_support =
            (platform == Platform::Macos).then(|| home.join("Library").join("Application Support"));
        let vscode_global_storage = vscode_global_storage_roots(
            platform,
            &config_home,
            app_data.as_deref(),
            mac_application_support.as_deref(),
        );
        let copilot_skill_dirs = read_env("COPILOT_SKILLS_DIRS")
            .map(|value| parse_copilot_skill_dirs(platform, value.as_os_str()))
            .unwrap_or_default();

        Self {
            platform,
            codex_home: read_env("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            claude_home: read_env("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude")),
            // GEMINI_CLI_HOME is the parent of `.gemini`, not a replacement
            // for the `.gemini` directory itself.
            gemini_home: read_env("GEMINI_CLI_HOME")
                .unwrap_or_else(|| home.clone())
                .join(".gemini"),
            // These variables replace their complete default directories.
            copilot_home: read_env("COPILOT_HOME").unwrap_or_else(|| home.join(".copilot")),
            copilot_skill_dirs,
            continue_home: read_env("CONTINUE_GLOBAL_DIR")
                .unwrap_or_else(|| home.join(".continue")),
            // Cline documents CLINE_DATA_DIR. There is no invented fallback
            // variable: absent an explicit override, the normal `.cline`
            // detection probe remains authoritative.
            cline_data_dir: read_env("CLINE_DATA_DIR"),
            vibe_home: read_env("VIBE_HOME").unwrap_or_else(|| home.join(".vibe")),
            hermes_home: read_env("HERMES_HOME").unwrap_or_else(|| home.join(".hermes")),
            autohand_home: read_env("AUTOHAND_HOME").unwrap_or_else(|| home.join(".autohand")),
            grok_home: read_env("GROK_HOME").unwrap_or_else(|| home.join(".grok")),
            flatpak_config_home: (platform == Platform::Linux)
                .then(|| read_env("FLATPAK_XDG_CONFIG_HOME"))
                .flatten(),
            mac_application_support,
            vscode_global_storage,
            roo_custom_storage: Vec::new(),
            home,
            config_home,
            xdg_data_home,
            app_data,
        }
    }

    /// Resolve one registry template. Platform-only placeholders are omitted
    /// when they do not apply, which prevents literal `$APPDATA` paths from
    /// leaking into scans on macOS/Linux.
    pub(crate) fn resolve(&self, template: &str) -> Option<String> {
        const PROJECT: &str = "$PROJECT";
        if template.starts_with(PROJECT) {
            return Some(template.to_string());
        }

        let bases: [(&str, Option<&Path>); 16] = [
            ("$CONFIG_HOME", Some(self.config_home.as_path())),
            ("$XDG_DATA_HOME", Some(self.xdg_data_home.as_path())),
            ("$CODEX_HOME", Some(self.codex_home.as_path())),
            ("$CLAUDE_HOME", Some(self.claude_home.as_path())),
            ("$GEMINI_HOME", Some(self.gemini_home.as_path())),
            ("$COPILOT_HOME", Some(self.copilot_home.as_path())),
            ("$CONTINUE_HOME", Some(self.continue_home.as_path())),
            ("$CLINE_DATA_DIR", self.cline_data_dir.as_deref()),
            ("$VIBE_HOME", Some(self.vibe_home.as_path())),
            ("$HERMES_HOME", Some(self.hermes_home.as_path())),
            ("$AUTOHAND_HOME", Some(self.autohand_home.as_path())),
            ("$GROK_HOME", Some(self.grok_home.as_path())),
            ("$APPDATA", self.app_data.as_deref()),
            (
                "$FLATPAK_XDG_CONFIG_HOME",
                self.flatpak_config_home.as_deref(),
            ),
            ("$MAC_APP_SUPPORT", self.mac_application_support.as_deref()),
            ("$HOME", Some(self.home.as_path())),
        ];

        for (token, base) in bases {
            if let Some(suffix) = template.strip_prefix(token) {
                let base = base?;
                return Some(join_template(base, suffix));
            }
        }

        // Refuse unknown placeholders instead of turning them into relative
        // paths. In particular, there is no ROO_CODE_STORAGE_DIR contract.
        if template.starts_with('$') {
            return None;
        }

        Some(template.replace('\\', "/"))
    }

    /// Resolve and deduplicate registry templates while preserving catalog
    /// order. Multi-root editor placeholders expand only to exact known roots.
    pub(crate) fn resolve_all(&self, templates: &[&str]) -> Vec<String> {
        let mut paths = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for template in templates {
            if let Some(suffix) = template.strip_prefix(VSCODE_GLOBAL_STORAGE) {
                self.extend_roots(&mut paths, &mut seen, &self.vscode_global_storage, suffix);
            } else if let Some(suffix) = template.strip_prefix(COPILOT_SKILLS_DIRS) {
                self.extend_roots(&mut paths, &mut seen, &self.copilot_skill_dirs, suffix);
            } else if let Some(suffix) = template.strip_prefix(ROO_CUSTOM_STORAGE) {
                self.extend_roots(&mut paths, &mut seen, &self.roo_custom_storage, suffix);
            } else if let Some(path) = self.resolve(template) {
                push_unique_path(self.platform, &mut paths, &mut seen, path);
            }
        }

        paths
    }

    fn extend_roots(
        &self,
        paths: &mut Vec<String>,
        seen: &mut std::collections::HashSet<String>,
        roots: &[PathBuf],
        suffix: &str,
    ) {
        for root in roots {
            push_unique_path(self.platform, paths, seen, join_template(root, suffix));
        }
    }

    fn vscode_settings_files(&self) -> Vec<PathBuf> {
        self.vscode_global_storage
            .iter()
            .filter_map(|root| root.parent())
            .map(|user| user.join("settings.json"))
            .collect()
    }
}

/// Copilot defines commas as the delimiter on every supported platform.
/// Using the OS path-list separator here would split Windows drive letters or
/// accept a format that Copilot itself does not honor.
fn parse_copilot_skill_dirs(platform: Platform, value: &OsStr) -> Vec<PathBuf> {
    let value = value.to_string_lossy();
    if value.contains('\u{fffd}') || value.len() > MAX_PATH_LIST_BYTES {
        return Vec::new();
    }

    let mut paths = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in value.split(',').take(MAX_PATH_LIST_ENTRIES) {
        let entry = entry.trim();
        if !is_safe_custom_skill_root(platform, entry) {
            continue;
        }

        let path = PathBuf::from(entry);
        let normalized = entry.replace('\\', "/");
        let key = if platform == Platform::Windows {
            normalized.to_lowercase()
        } else {
            normalized
        };
        if seen.insert(key) {
            paths.push(path);
        }
    }
    paths
}

fn is_safe_custom_skill_root(platform: Platform, value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_PATH_BYTES
        || value.contains(['*', '?', '[', ']'])
        || value.chars().any(char::is_control)
    {
        return false;
    }

    let normalized = value.replace('\\', "/");
    let path_parts: Vec<_> = normalized
        .split('/')
        .filter(|component| !component.is_empty())
        .collect();
    if path_parts
        .iter()
        .any(|component| matches!(*component, "." | ".."))
    {
        return false;
    }

    match platform {
        Platform::Windows => {
            let drive_absolute = normalized.as_bytes().get(1) == Some(&b':')
                && normalized.as_bytes().get(2) == Some(&b'/')
                && normalized
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphabetic);

            if drive_absolute {
                // A drive root alone is too broad.
                path_parts.len() >= 2
            } else {
                // Local-only discovery rejects all UNC, extended UNC, and
                // device namespace forms, even when they name a subdirectory.
                false
            }
        }
        Platform::Macos | Platform::Linux => normalized.starts_with('/') && !path_parts.is_empty(),
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
}

fn vscode_global_storage_roots(
    platform: Platform,
    config_home: &Path,
    app_data: Option<&Path>,
    mac_application_support: Option<&Path>,
) -> Vec<PathBuf> {
    const CHANNELS: [&str; 2] = ["Code", "Code - Insiders"];
    let base = match platform {
        Platform::Windows => app_data,
        Platform::Macos => mac_application_support,
        Platform::Linux => Some(config_home),
    };

    base.map(|base| {
        CHANNELS
            .iter()
            .map(|channel| base.join(channel).join("User").join("globalStorage"))
            .collect()
    })
    .unwrap_or_default()
}

fn discover_roo_custom_storage(settings_files: &[PathBuf], home: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for settings_file in settings_files {
        let Ok(metadata) = fs::symlink_metadata(settings_file) else {
            continue;
        };
        if !metadata.file_type().is_file() || metadata.len() > MAX_SETTINGS_BYTES {
            continue;
        }
        let Ok(contents) = fs::read_to_string(settings_file) else {
            continue;
        };
        let Some(value) = parse_jsonc_object(&contents)
            .and_then(|settings| settings.get("roo-cline.customStoragePath").cloned())
            .and_then(|value| value.as_str().map(str::to_owned))
        else {
            continue;
        };
        let Some(path) = explicit_path(&value, home) else {
            continue;
        };
        let normalized = path.to_string_lossy().replace('\\', "/");
        let key = normalized.to_lowercase();
        if seen.insert(key) {
            roots.push(path);
        }
    }

    roots
}

fn parse_jsonc_object(contents: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let without_comments = strip_jsonc_comments(contents)?;
    let without_trailing_commas = strip_trailing_commas(&without_comments);
    serde_json::from_str::<serde_json::Value>(&without_trailing_commas)
        .ok()?
        .as_object()
        .cloned()
}

fn strip_jsonc_comments(contents: &str) -> Option<String> {
    let chars: Vec<char> = contents.chars().collect();
    let mut output = String::with_capacity(contents.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;

    while index < chars.len() {
        let current = chars[index];
        if in_string {
            output.push(current);
            if escaped {
                escaped = false;
            } else if current == '\\' {
                escaped = true;
            } else if current == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if current == '"' {
            in_string = true;
            output.push(current);
            index += 1;
        } else if current == '/' && chars.get(index + 1) == Some(&'/') {
            index += 2;
            while index < chars.len() && !matches!(chars[index], '\n' | '\r') {
                index += 1;
            }
        } else if current == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            let mut closed = false;
            while index + 1 < chars.len() {
                if chars[index] == '*' && chars[index + 1] == '/' {
                    index += 2;
                    closed = true;
                    break;
                }
                index += 1;
            }
            if !closed {
                return None;
            }
        } else {
            output.push(current);
            index += 1;
        }
    }

    (!in_string).then_some(output)
}

fn strip_trailing_commas(contents: &str) -> String {
    let chars: Vec<char> = contents.chars().collect();
    let mut output = String::with_capacity(contents.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;

    while index < chars.len() {
        let current = chars[index];
        if in_string {
            output.push(current);
            if escaped {
                escaped = false;
            } else if current == '\\' {
                escaped = true;
            } else if current == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if current == '"' {
            in_string = true;
            output.push(current);
        } else if current == ',' {
            let next_non_whitespace = chars[index + 1..]
                .iter()
                .find(|candidate| !candidate.is_whitespace());
            if !matches!(next_non_whitespace, Some('}') | Some(']')) {
                output.push(current);
            }
        } else {
            output.push(current);
        }
        index += 1;
    }

    output
}

fn explicit_path(value: &str, home: &Path) -> Option<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(relative) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        return Some(home.join(relative));
    }

    let path = PathBuf::from(trimmed);
    path.is_absolute().then_some(path)
}

fn push_unique_path(
    platform: Platform,
    paths: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
    path: String,
) {
    let key = if platform == Platform::Windows {
        path.to_lowercase()
    } else {
        path.clone()
    };
    if seen.insert(key) {
        paths.push(path);
    }
}

fn join_template(base: &Path, suffix: &str) -> String {
    let relative = suffix.trim_start_matches(['/', '\\']);
    let path = if relative.is_empty() {
        base.to_path_buf()
    } else {
        base.join(relative)
    };
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn capture_root_fixture() -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("capture tempdir");
        fs::write(
            temp.path().join(CAPTURE_ROOT_SENTINEL),
            CAPTURE_ROOT_SENTINEL_CONTENT,
        )
        .expect("capture sentinel");
        temp
    }

    #[test]
    fn capture_root_requires_the_exact_sentinel_contract() {
        let temp = tempfile::tempdir().expect("capture tempdir");
        let missing = CaptureRoot::from_declared_path(temp.path().to_path_buf())
            .expect_err("missing sentinel must fail closed");
        assert!(missing.contains(CAPTURE_ROOT_SENTINEL));

        fs::write(temp.path().join(CAPTURE_ROOT_SENTINEL), "wrong contract\n")
            .expect("wrong sentinel");
        let invalid = CaptureRoot::from_declared_path(temp.path().to_path_buf())
            .expect_err("wrong sentinel must fail closed");
        assert!(invalid.contains("does not declare"));
    }

    #[test]
    fn capture_root_allows_only_regular_files_below_its_real_root() {
        let temp = capture_root_fixture();
        let inside = temp.path().join("fixture").join("SKILL.md");
        let outside = temp.path().with_extension("outside.md");
        fs::create_dir_all(inside.parent().expect("inside parent")).expect("inside directory");
        fs::write(&inside, "fixture").expect("inside file");
        fs::write(&outside, "outside").expect("outside file");
        let root =
            CaptureRoot::from_declared_path(temp.path().to_path_buf()).expect("valid capture root");

        assert!(root.contains_file(&inside));
        assert!(!root.contains_file(&outside));
    }

    #[test]
    fn capture_root_rejects_symlink_or_junction_escape() {
        let temp = capture_root_fixture();
        let outside = temp.path().with_extension("outside.md");
        let link = temp.path().join("linked.md");
        fs::write(&outside, "outside").expect("outside file");

        #[cfg(unix)]
        let link_result = std::os::unix::fs::symlink(&outside, &link);
        #[cfg(windows)]
        let link_result = std::os::windows::fs::symlink_file(&outside, &link);
        if link_result.is_err() {
            return;
        }

        let root =
            CaptureRoot::from_declared_path(temp.path().to_path_buf()).expect("valid capture root");
        assert!(!root.contains_file(&link));
    }

    #[test]
    fn windows_fixture_uses_appdata_with_native_case() {
        let env = LocationEnvironment::fixture(Platform::Windows, "C:/Users/Test");
        assert_eq!(
            env.resolve("$APPDATA/Zed").as_deref(),
            Some("C:/Users/Test/AppData/Roaming/Zed")
        );
        assert!(env.resolve("$MAC_APP_SUPPORT/Zed").is_none());
    }

    #[test]
    fn macos_fixture_uses_application_support() {
        let env = LocationEnvironment::fixture(Platform::Macos, "/Users/test");
        assert_eq!(
            env.resolve("$MAC_APP_SUPPORT/Zed").as_deref(),
            Some("/Users/test/Library/Application Support/Zed")
        );
        assert!(env.resolve("$APPDATA/Zed").is_none());
    }

    #[test]
    fn linux_fixture_uses_xdg_and_flatpak_roots() {
        let env = LocationEnvironment::fixture(Platform::Linux, "/home/test");
        assert_eq!(
            env.resolve("$CONFIG_HOME/opencode/skills/**/SKILL.md")
                .as_deref(),
            Some("/home/test/.config/opencode/skills/**/SKILL.md")
        );
        assert_eq!(
            env.resolve("$FLATPAK_XDG_CONFIG_HOME/zed").as_deref(),
            Some("/home/test/.var/app/config/zed")
        );
    }

    #[test]
    fn environment_specific_agent_homes_are_first_class() {
        let mut env = LocationEnvironment::fixture(Platform::Linux, "/home/test");
        env.codex_home = PathBuf::from("/opt/codex");
        env.claude_home = PathBuf::from("/opt/claude");
        env.vibe_home = PathBuf::from("/opt/vibe");
        env.hermes_home = PathBuf::from("/opt/hermes");
        env.autohand_home = PathBuf::from("/opt/autohand");
        env.grok_home = PathBuf::from("/opt/grok");

        assert_eq!(
            env.resolve("$CODEX_HOME/skills/**/SKILL.md").as_deref(),
            Some("/opt/codex/skills/**/SKILL.md")
        );
        assert_eq!(
            env.resolve("$CLAUDE_HOME/skills/**/SKILL.md").as_deref(),
            Some("/opt/claude/skills/**/SKILL.md")
        );
        assert_eq!(
            env.resolve("$VIBE_HOME/skills/**/SKILL.md").as_deref(),
            Some("/opt/vibe/skills/**/SKILL.md")
        );
        assert_eq!(
            env.resolve("$HERMES_HOME/skills/**/SKILL.md").as_deref(),
            Some("/opt/hermes/skills/**/SKILL.md")
        );
        assert_eq!(
            env.resolve("$AUTOHAND_HOME/skills/**/SKILL.md").as_deref(),
            Some("/opt/autohand/skills/**/SKILL.md")
        );
        assert_eq!(
            env.resolve("$GROK_HOME/skills/**/SKILL.md").as_deref(),
            Some("/opt/grok/skills/**/SKILL.md")
        );
    }

    #[test]
    fn override_semantics_match_each_upstream_contract() {
        let variables = HashMap::from([
            ("CODEX_HOME", PathBuf::from("/opt/codex")),
            ("CLAUDE_CONFIG_DIR", PathBuf::from("/opt/claude")),
            ("GEMINI_CLI_HOME", PathBuf::from("/opt/gemini-parent")),
            ("COPILOT_HOME", PathBuf::from("/opt/copilot")),
            (
                "COPILOT_SKILLS_DIRS",
                PathBuf::from("/opt/team-skills,/opt/team-skills"),
            ),
            ("CONTINUE_GLOBAL_DIR", PathBuf::from("/opt/continue")),
            ("CLINE_DATA_DIR", PathBuf::from("/opt/cline-data")),
            ("XDG_DATA_HOME", PathBuf::from("/opt/xdg-data")),
        ]);
        let env = LocationEnvironment::from_reader(
            Platform::Linux,
            PathBuf::from("/home/test"),
            |name| variables.get(name).cloned(),
        );

        assert_eq!(env.codex_home, PathBuf::from("/opt/codex"));
        assert_eq!(env.claude_home, PathBuf::from("/opt/claude"));
        assert_eq!(env.gemini_home, PathBuf::from("/opt/gemini-parent/.gemini"));
        assert_eq!(env.copilot_home, PathBuf::from("/opt/copilot"));
        assert_eq!(
            env.copilot_skill_dirs,
            vec![PathBuf::from("/opt/team-skills")]
        );
        assert_eq!(env.continue_home, PathBuf::from("/opt/continue"));
        assert_eq!(env.cline_data_dir, Some(PathBuf::from("/opt/cline-data")));
        assert_eq!(env.xdg_data_home, PathBuf::from("/opt/xdg-data"));
    }

    #[test]
    fn windows_and_macos_include_stable_and_insiders_extension_roots() {
        let windows = LocationEnvironment::fixture(Platform::Windows, "C:/Users/Test");
        assert_eq!(
            windows.resolve_all(&["$VSCODE_GLOBAL_STORAGE/saoudrizwan.claude-dev"]),
            vec![
                "C:/Users/Test/AppData/Roaming/Code/User/globalStorage/saoudrizwan.claude-dev",
                "C:/Users/Test/AppData/Roaming/Code - Insiders/User/globalStorage/saoudrizwan.claude-dev",
            ]
        );

        let macos = LocationEnvironment::fixture(Platform::Macos, "/Users/test");
        assert_eq!(
            macos.resolve_all(&["$VSCODE_GLOBAL_STORAGE/rooveterinaryinc.roo-cline"]),
            vec![
                "/Users/test/Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline",
                "/Users/test/Library/Application Support/Code - Insiders/User/globalStorage/rooveterinaryinc.roo-cline",
            ]
        );
    }

    #[test]
    fn roo_custom_storage_is_read_only_from_bounded_vscode_settings() {
        let temp = tempfile::tempdir().expect("tempdir");
        let stable = temp.path().join("stable-settings.json");
        let insiders = temp.path().join("insiders-settings.json");
        fs::write(
            &stable,
            r#"{
                // JSONC is the normal VS Code settings format.
                "roo-cline.customStoragePath": "~/Roo State",
            }"#,
        )
        .expect("stable settings");
        fs::write(
            &insiders,
            r#"{"roo-cline.customStoragePath":"relative/is/rejected"}"#,
        )
        .expect("insiders settings");

        assert_eq!(
            discover_roo_custom_storage(&[stable, insiders], Path::new("C:/Users/Test")),
            vec![PathBuf::from("C:/Users/Test/Roo State")]
        );
    }

    #[test]
    fn roo_settings_reject_malformed_and_oversized_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let malformed = temp.path().join("malformed.json");
        let oversized = temp.path().join("oversized.json");
        fs::write(
            &malformed,
            r#"{"roo-cline.customStoragePath":"C:/Roo", /* open"#,
        )
        .expect("malformed settings");
        let file = fs::File::create(&oversized).expect("oversized settings");
        file.set_len(MAX_SETTINGS_BYTES + 1)
            .expect("extend oversized settings");

        assert!(
            discover_roo_custom_storage(&[malformed, oversized], Path::new("C:/Users/Test"))
                .is_empty()
        );
    }

    #[test]
    fn path_dedupe_is_case_insensitive_only_on_windows() {
        let windows = LocationEnvironment::fixture(Platform::Windows, "C:/Users/Test");
        assert_eq!(
            windows.resolve_all(&["$HOME/.codex", "$HOME/.CODEX"]),
            vec!["C:/Users/Test/.codex"]
        );

        let linux = LocationEnvironment::fixture(Platform::Linux, "/home/test");
        assert_eq!(
            linux.resolve_all(&["$HOME/.codex", "$HOME/.CODEX"]),
            vec!["/home/test/.codex", "/home/test/.CODEX"]
        );
    }

    #[test]
    fn xdg_data_is_not_rewritten_to_platform_application_data() {
        let windows = LocationEnvironment::fixture(Platform::Windows, "C:/Users/Test");
        let macos = LocationEnvironment::fixture(Platform::Macos, "/Users/test");

        assert_eq!(
            windows.resolve("$XDG_DATA_HOME/opencode").as_deref(),
            Some("C:/Users/Test/.local/share/opencode")
        );
        assert_eq!(
            macos.resolve("$XDG_DATA_HOME/kilo").as_deref(),
            Some("/Users/test/.local/share/kilo")
        );
    }

    #[test]
    fn unknown_environment_placeholders_are_excluded() {
        let env = LocationEnvironment::fixture(Platform::Windows, "C:/Users/Test");
        assert!(env.resolve("$ROO_CODE_STORAGE_DIR").is_none());
        assert!(env
            .resolve_all(&["$ROO_CODE_STORAGE_DIR", "$HOME/.roo"])
            .iter()
            .all(|path| !path.contains("ROO_CODE_STORAGE_DIR")));
    }

    #[test]
    fn copilot_skill_dirs_use_commas_on_windows_without_splitting_drive_letters() {
        let variables = HashMap::from([(
            "COPILOT_SKILLS_DIRS",
            PathBuf::from(r" C:\Agent Skills , D:\Shared\skills,C:\AGENT SKILLS "),
        )]);
        let env = LocationEnvironment::from_reader(
            Platform::Windows,
            PathBuf::from("C:/Users/Test"),
            |name| variables.get(name).cloned(),
        );

        assert_eq!(
            env.resolve_all(&["$COPILOT_SKILLS_DIRS/**/SKILL.md"]),
            vec![
                "C:/Agent Skills/**/SKILL.md",
                "D:/Shared/skills/**/SKILL.md",
            ]
        );
    }

    #[test]
    fn copilot_skill_dirs_use_the_same_comma_contract_on_macos_and_linux() {
        let macos = parse_copilot_skill_dirs(
            Platform::Macos,
            OsStr::new("/Users/test/a:/still-one,/Users/test/b"),
        );
        assert_eq!(
            macos,
            vec![
                PathBuf::from("/Users/test/a:/still-one"),
                PathBuf::from("/Users/test/b")
            ]
        );

        let linux = parse_copilot_skill_dirs(
            Platform::Linux,
            OsStr::new("/opt/skills;/srv/not-a-separator,/srv/skills"),
        );
        assert_eq!(
            linux,
            vec![
                PathBuf::from("/opt/skills;/srv/not-a-separator"),
                PathBuf::from("/srv/skills")
            ]
        );
    }

    #[test]
    fn copilot_skill_dirs_reject_malformed_and_unsafe_entries() {
        let parsed = parse_copilot_skill_dirs(
            Platform::Linux,
            OsStr::new(
                ",relative,../escape,/safe/../escape,/skills/*,/,/valid/skills,/valid/skills",
            ),
        );
        assert_eq!(parsed, vec![PathBuf::from("/valid/skills")]);

        assert_eq!(
            parse_copilot_skill_dirs(
                Platform::Windows,
                OsStr::new(
                    r"C:\,C:relative,C:\local\skills,\\server\share,\\server\share\skills,\\?\UNC\server\share\skills,\\.\C:\skills"
                )
            ),
            vec![PathBuf::from(r"C:\local\skills")]
        );
    }

    #[test]
    fn copilot_skill_dirs_reject_every_windows_network_and_device_namespace() {
        for value in [
            r"\\server\share\skills",
            r"//server/share/skills",
            r"\\?\UNC\server\share\skills",
            r"//?/UNC/server/share/skills",
            r"\\.\C:\skills",
            r"//./C:/skills",
        ] {
            assert!(
                parse_copilot_skill_dirs(Platform::Windows, OsStr::new(value)).is_empty(),
                "{value} must remain outside local-only discovery"
            );
        }
    }

    #[test]
    fn copilot_skill_dirs_are_bounded_by_total_entry_and_item_size() {
        let oversized = "x".repeat(MAX_PATH_LIST_BYTES + 1);
        assert!(parse_copilot_skill_dirs(Platform::Linux, OsStr::new(&oversized)).is_empty());

        let long_entry = format!("/{}", "x".repeat(MAX_PATH_BYTES));
        assert!(parse_copilot_skill_dirs(Platform::Linux, OsStr::new(&long_entry)).is_empty());

        let many = (0..(MAX_PATH_LIST_ENTRIES + 5))
            .map(|index| format!("/skills/{index}"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            parse_copilot_skill_dirs(Platform::Linux, OsStr::new(&many)).len(),
            MAX_PATH_LIST_ENTRIES
        );
    }
}
