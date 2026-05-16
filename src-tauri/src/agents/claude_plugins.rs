// @agent-context: Claude Code plugin discovery — the canonical scanner for
// skills/commands/agents that arrive via `claude plugin install`.
//
// CC v2 plugins are stored under `$HOME/.claude/plugins/cache/<marketplace>/
// <plugin>/<version>/`. Each plugin can ship a `skills/`, `commands/`, and
// `agents/` directory, plus a `.claude-plugin/plugin.json` manifest. The set of
// active installs is recorded in `$HOME/.claude/plugins/installed_plugins.json`
// (canonical install paths) and the enabled/disabled flag is in
// `$HOME/.claude/settings.json` under `enabledPlugins`.
//
// This module reads both files, then walks only the enabled plugins. That avoids
// version-proliferation duplicates (multiple cached versions of the same plugin)
// and respects the user's disable choice without globbing the entire cache.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::models::{AgentInfo, ScanError, Skill, SkillFormat, SkillScope};

/// Filename segment we must skip while walking a plugin install — Git's
/// sample hook scripts live under `<plugin>/.git/hooks/` and would otherwise
/// pollute the command bucket.
const GIT_DIR_SEGMENT: &str = ".git";

/// One installed plugin record as it appears in `installed_plugins.json`.
/// `version` and `scope` are kept for round-trip fidelity and future surfacing
/// in the UI; they are not consumed by the scanner today.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct InstallRecord {
    install_path: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct InstalledPluginsFile {
    #[serde(default)]
    plugins: HashMap<String, Vec<InstallRecord>>,
}

/// A resolved plugin install — what we actually scan.
#[derive(Debug, Clone)]
pub struct PluginInstall {
    /// Marketplace-qualified key, e.g. `superpowers@superpowers-dev`.
    pub key: String,
    /// Short plugin name, e.g. `superpowers` (used for slash-command namespacing).
    pub name: String,
    /// Absolute filesystem path to the plugin's version directory.
    pub install_path: PathBuf,
    /// `true` when the user has the plugin enabled in `settings.json`.
    pub enabled: bool,
}

/// Public entry point: discover every skill/command/agent contributed by an
/// installed (and enabled) Claude Code plugin.
pub fn scan_claude_plugins(home: &Path, agent: &AgentInfo) -> (Vec<Skill>, Vec<ScanError>) {
    let mut skills: Vec<Skill> = Vec::new();
    let mut errors: Vec<ScanError> = Vec::new();

    let installs = match discover_plugin_installs(home) {
        Ok(list) => list,
        Err(e) => {
            errors.push(ScanError {
                file_path: home.join(".claude/plugins/installed_plugins.json").to_string_lossy().to_string(),
                message: format!("plugin registry unreadable: {e}"),
            });
            return (skills, errors);
        }
    };

    for install in installs.into_iter().filter(|i| i.enabled) {
        let (mut found, mut found_errors) = scan_single_plugin(&install, agent);
        skills.append(&mut found);
        errors.append(&mut found_errors);
    }

    (skills, errors)
}

/// Read `installed_plugins.json` and cross-reference with `enabledPlugins` in
/// `settings.json` to produce a resolved list of installs.
pub fn discover_plugin_installs(home: &Path) -> Result<Vec<PluginInstall>> {
    let registry_path = home.join(".claude/plugins/installed_plugins.json");
    if !registry_path.exists() {
        return Ok(Vec::new());
    }

    let raw = std::fs::read_to_string(&registry_path)
        .with_context(|| format!("read {}", registry_path.display()))?;
    let parsed: InstalledPluginsFile = serde_json::from_str(&raw)
        .with_context(|| format!("parse {}", registry_path.display()))?;

    let enabled_set = read_enabled_plugins(home).unwrap_or_default();

    let mut out: Vec<PluginInstall> = Vec::new();
    for (key, records) in parsed.plugins.into_iter() {
        let name = plugin_name_from_key(&key);
        // Default to enabled when the settings flag is missing — better to show
        // a freshly installed plugin than to silently hide it.
        let enabled = enabled_set
            .get(&key)
            .copied()
            .unwrap_or(true);

        for record in records {
            let install_path = PathBuf::from(record.install_path);
            if !install_path.exists() {
                continue;
            }
            out.push(PluginInstall {
                key: key.clone(),
                name: name.clone(),
                install_path,
                enabled,
            });
        }
    }

    // Stable ordering keeps diffs predictable for tests and the UI.
    out.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(out)
}

/// Parse `settings.json`'s `enabledPlugins` map. Missing file or missing key
/// returns `None` so the caller can treat plugins as enabled by default.
fn read_enabled_plugins(home: &Path) -> Option<HashMap<String, bool>> {
    let settings_path = home.join(".claude/settings.json");
    let raw = std::fs::read_to_string(&settings_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let map = value.get("enabledPlugins")?.as_object()?;
    let mut out = HashMap::with_capacity(map.len());
    for (k, v) in map {
        if let Some(b) = v.as_bool() {
            out.insert(k.clone(), b);
        }
    }
    Some(out)
}

/// `superpowers@superpowers-dev` → `superpowers`.
fn plugin_name_from_key(key: &str) -> String {
    key.split('@').next().unwrap_or(key).to_string()
}

/// Walk one plugin's install directory and parse every supported artifact.
fn scan_single_plugin(
    install: &PluginInstall,
    agent: &AgentInfo,
) -> (Vec<Skill>, Vec<ScanError>) {
    let mut skills: Vec<Skill> = Vec::new();
    let mut errors: Vec<ScanError> = Vec::new();

    if agent.format != SkillFormat::SkillMd {
        return (skills, errors);
    }

    for entry in WalkDir::new(&install.install_path)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| !is_git_dir(e.path()))
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if !is_plugin_artifact(path) {
            continue;
        }

        match super::scanner::parse_file_for_agent(path, agent, SkillScope::Global) {
            Ok(mut parsed) => {
                for skill in &mut parsed {
                    annotate_plugin_origin(skill, install);
                }
                skills.extend(parsed);
            }
            Err(e) => errors.push(ScanError {
                file_path: path.to_string_lossy().to_string(),
                message: e.to_string(),
            }),
        }
    }

    (skills, errors)
}

/// `.git/` lives at the root of every plugin install when CC clones the source.
/// Skip the whole subtree to avoid Git's sample hook scripts.
fn is_git_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n == GIT_DIR_SEGMENT)
        .unwrap_or(false)
}

/// Decide whether `path` is something Skill Deck should surface from a plugin
/// install. We accept SKILL.md, command markdown files, agent markdown files.
fn is_plugin_artifact(path: &Path) -> bool {
    let lower = normalize_path_sep(&path.to_string_lossy()).to_lowercase();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_lowercase();

    if file_name == "skill.md" {
        return true;
    }

    if file_name.ends_with(".md") {
        // `commands/foo.md` and `agents/bar.md` are the conventional layouts.
        return lower.contains("/commands/") || lower.contains("/agents/");
    }

    false
}

/// Decorate the parsed skill with plugin metadata so the UI can show the
/// owning plugin and the user can search by `pluginname:command`.
fn annotate_plugin_origin(skill: &mut Skill, install: &PluginInstall) {
    // Push the plugin's marketplace key into the description tail when not
    // already mentioned. Cheap, lossless, and grepable.
    let tag = format!("[plugin: {}]", install.key);
    if !skill.description.contains(&tag) {
        if skill.description.is_empty() {
            skill.description = tag;
        } else {
            skill.description.push(' ');
            skill.description.push_str(&tag);
        }
    }

    // Plugin commands and skills are addressed as `/<plugin>:<name>` in Claude
    // Code. Rewrite a bare `/name` into the namespaced form so search matches
    // what the user actually types in the CLI.
    if let Some(existing) = skill.metadata.slash_command.clone() {
        if !existing.contains(':') {
            let stripped = existing.trim_start_matches('/');
            if !stripped.is_empty() {
                skill.metadata.slash_command =
                    Some(format!("/{}:{}", install.name, stripped));
            }
        }
    }

    // Record an extra tag the discovery layer can surface.
    if !skill.discovery_tags.iter().any(|t| t == "plugin") {
        skill.discovery_tags.push("plugin".to_string());
    }
}

fn normalize_path_sep(s: &str) -> String {
    s.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AgentId;
    use std::collections::HashSet;
    use std::fs;

    fn make_agent_info() -> AgentInfo {
        AgentInfo {
            id: AgentId::ClaudeCode,
            display_name: "Claude Code".to_string(),
            description: "test".to_string(),
            color: "#000000".to_string(),
            installed: true,
            skill_count: 0,
            global_paths: vec![],
            global_detection_paths: vec![],
            project_paths: vec![],
            format: SkillFormat::SkillMd,
        }
    }

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn discover_returns_empty_when_registry_missing() {
        let temp = tempfile::tempdir().unwrap();
        let installs = discover_plugin_installs(temp.path()).unwrap();
        assert!(installs.is_empty());
    }

    #[test]
    fn discover_reads_install_paths_and_enabled_state() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();

        let install_dir = home.join(".claude/plugins/cache/mkt/foo/1.0.0");
        fs::create_dir_all(&install_dir).unwrap();
        let disabled_dir = home.join(".claude/plugins/cache/mkt/bar/1.0.0");
        fs::create_dir_all(&disabled_dir).unwrap();

        write(
            &home.join(".claude/plugins/installed_plugins.json"),
            &serde_json::json!({
                "version": 2,
                "plugins": {
                    "foo@mkt": [{
                        "scope": "user",
                        "installPath": install_dir.to_string_lossy(),
                        "version": "1.0.0",
                    }],
                    "bar@mkt": [{
                        "scope": "user",
                        "installPath": disabled_dir.to_string_lossy(),
                        "version": "1.0.0",
                    }],
                }
            })
            .to_string(),
        );
        write(
            &home.join(".claude/settings.json"),
            &serde_json::json!({
                "enabledPlugins": {
                    "foo@mkt": true,
                    "bar@mkt": false,
                }
            })
            .to_string(),
        );

        let installs = discover_plugin_installs(home).unwrap();
        assert_eq!(installs.len(), 2);
        let foo = installs.iter().find(|i| i.key == "foo@mkt").unwrap();
        assert!(foo.enabled);
        assert_eq!(foo.name, "foo");
        let bar = installs.iter().find(|i| i.key == "bar@mkt").unwrap();
        assert!(!bar.enabled);
    }

    #[test]
    fn discover_treats_missing_enabled_entry_as_enabled() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let install_dir = home.join(".claude/plugins/cache/mkt/foo/1.0.0");
        fs::create_dir_all(&install_dir).unwrap();
        write(
            &home.join(".claude/plugins/installed_plugins.json"),
            &serde_json::json!({
                "version": 2,
                "plugins": {
                    "foo@mkt": [{ "installPath": install_dir.to_string_lossy() }]
                }
            })
            .to_string(),
        );
        // No settings.json at all.
        let installs = discover_plugin_installs(home).unwrap();
        assert_eq!(installs.len(), 1);
        assert!(installs[0].enabled);
    }

    #[test]
    fn discover_skips_records_whose_install_path_is_gone() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        write(
            &home.join(".claude/plugins/installed_plugins.json"),
            &serde_json::json!({
                "version": 2,
                "plugins": {
                    "ghost@mkt": [{ "installPath": "C:/nope/does/not/exist" }]
                }
            })
            .to_string(),
        );
        let installs = discover_plugin_installs(home).unwrap();
        assert!(installs.is_empty());
    }

    #[test]
    fn scan_picks_up_skills_commands_agents_and_skips_git() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let install_dir = home.join(".claude/plugins/cache/mkt/plug/1.0.0");

        write(
            &install_dir.join("skills/alpha/SKILL.md"),
            "---\nname: alpha\ndescription: alpha skill\n---\nbody",
        );
        write(
            &install_dir.join("commands/beta.md"),
            "---\nname: beta\ndescription: beta command\n---\nbody",
        );
        write(
            &install_dir.join("agents/gamma.md"),
            "---\nname: gamma\ndescription: gamma agent\n---\nbody",
        );
        write(
            &install_dir.join(".git/hooks/pre-commit.sample"),
            "#!/bin/sh\nexit 0",
        );
        write(
            &install_dir.join(".git/hooks/post-commit.md"),
            "# fake",
        );

        write(
            &home.join(".claude/plugins/installed_plugins.json"),
            &serde_json::json!({
                "version": 2,
                "plugins": {
                    "plug@mkt": [{ "installPath": install_dir.to_string_lossy() }]
                }
            })
            .to_string(),
        );
        write(
            &home.join(".claude/settings.json"),
            &serde_json::json!({"enabledPlugins": {"plug@mkt": true}}).to_string(),
        );

        let agent = make_agent_info();
        let (skills, errors) = scan_claude_plugins(home, &agent);

        assert!(errors.is_empty(), "errors: {errors:?}");
        let names: HashSet<_> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains("alpha"), "missing alpha: {names:?}");
        assert!(names.contains("beta"), "missing beta: {names:?}");
        assert!(names.contains("gamma"), "missing gamma: {names:?}");
        // Git's sample hook .md should be excluded.
        assert!(!names.contains("post-commit"));

        // Plugin origin tag added.
        for skill in &skills {
            assert!(skill.description.contains("[plugin: plug@mkt]"));
            assert!(skill.discovery_tags.contains(&"plugin".to_string()));
        }
    }

    #[test]
    fn scan_skips_disabled_plugin() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let install_dir = home.join(".claude/plugins/cache/mkt/off/1.0.0");
        write(
            &install_dir.join("skills/x/SKILL.md"),
            "---\nname: x\ndescription: should not appear\n---\n",
        );
        write(
            &home.join(".claude/plugins/installed_plugins.json"),
            &serde_json::json!({
                "version": 2,
                "plugins": {
                    "off@mkt": [{ "installPath": install_dir.to_string_lossy() }]
                }
            })
            .to_string(),
        );
        write(
            &home.join(".claude/settings.json"),
            &serde_json::json!({"enabledPlugins": {"off@mkt": false}}).to_string(),
        );

        let agent = make_agent_info();
        let (skills, _) = scan_claude_plugins(home, &agent);
        assert!(skills.is_empty());
    }
}
