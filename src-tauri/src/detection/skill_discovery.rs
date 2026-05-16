use crate::models::Skill;
use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

#[derive(Debug, Clone)]
struct Rule {
    tag: &'static str,
    use_case: &'static str,
    keywords: &'static [&'static str],
}

const RULES: &[Rule] = &[
    Rule {
        tag: "debugging",
        use_case: "debug",
        keywords: &[
            "debug",
            "bug",
            "trace",
            "stack trace",
            "diagnose",
            "incident",
            "failure",
            "broken",
        ],
    },
    Rule {
        tag: "testing",
        use_case: "test",
        keywords: &[
            "test",
            "unit test",
            "integration test",
            "coverage",
            "assert",
            "regression",
            "spec",
        ],
    },
    Rule {
        tag: "security",
        use_case: "secure",
        keywords: &[
            "security",
            "secure",
            "vulnerability",
            "owasp",
            "xss",
            "sqli",
            "csrf",
            "jwt",
            "auth",
            "injection",
        ],
    },
    Rule {
        tag: "documentation",
        use_case: "document",
        keywords: &[
            "readme",
            "docs",
            "documentation",
            "changelog",
            "release notes",
            "guide",
            "handbook",
        ],
    },
    Rule {
        tag: "release",
        use_case: "ship",
        keywords: &[
            "release",
            "publish",
            "deploy",
            "ship",
            "version",
            "tag",
            "rollout",
            "production",
        ],
    },
    Rule {
        tag: "git",
        use_case: "collaborate",
        keywords: &[
            "git",
            "commit",
            "branch",
            "pull request",
            // Removed bare "pr" — two letters, lit up "prompt"/"process"/"production".
            // The full phrase "pull request" stays as a multi-word keyword.
            "merge",
            "rebase",
            "cherry-pick",
        ],
    },
    Rule {
        tag: "frontend",
        use_case: "build-ui",
        keywords: &[
            "frontend",
            "ui",
            "ux",
            "css",
            "tailwind",
            "svelte",
            "react",
            "component",
            "a11y",
            "accessibility",
        ],
    },
    Rule {
        tag: "backend",
        use_case: "build-backend",
        keywords: &[
            "backend",
            "api",
            "endpoint",
            "server",
            "service",
            "database",
            "sql",
            "migration",
            "orm",
        ],
    },
    Rule {
        tag: "performance",
        use_case: "optimize",
        keywords: &[
            "performance",
            "optimize",
            "profiling",
            "latency",
            "throughput",
            "memory",
            "cpu",
            "benchmark",
        ],
    },
    Rule {
        tag: "ai-agent",
        use_case: "build-agent",
        keywords: &[
            "agent",
            "langchain",
            "langgraph",
            "prompt",
            "mcp",
            "tool calling",
            "reasoning",
            "rag",
            "llm",
        ],
    },
    Rule {
        tag: "refactoring",
        use_case: "refactor",
        keywords: &[
            "refactor",
            "cleanup",
            "deduplicate",
            "dry",
            "modular",
            "simplify",
            "maintainability",
        ],
    },
    Rule {
        tag: "career",
        use_case: "job-search",
        keywords: &[
            "resume",
            "cv",
            "cover letter",
            "job",
            "interview",
            "application",
            "linkedin",
            "salary",
        ],
    },
];

fn normalize_token(input: &str) -> Option<String> {
    let normalized = input
        .trim()
        .to_lowercase()
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn canonicalize_tag(tag: &str) -> String {
    match tag {
        "agent" | "ai" | "llm" | "prompt engineering" | "langchain" | "langgraph" => {
            "ai-agent".to_string()
        }
        "release management" | "deployment" | "deploy" => "release".to_string(),
        "docs" => "documentation".to_string(),
        "frontend ui" | "ui" | "ux" => "frontend".to_string(),
        "backend api" | "api" | "database" => "backend".to_string(),
        "qa" | "quality assurance" => "testing".to_string(),
        "devops" | "cicd" | "ci cd" => "release".to_string(),
        _ => tag.to_string(),
    }
}

fn canonicalize_use_case(use_case: &str) -> String {
    match use_case {
        "release" | "deploy" | "production" => "ship".to_string(),
        "documentation" | "docs" | "readme" => "document".to_string(),
        "frontend" | "ui" => "build-ui".to_string(),
        "backend" | "api" => "build-backend".to_string(),
        "agent" | "llm" | "prompt" => "build-agent".to_string(),
        _ => use_case.to_string(),
    }
}

fn push_unique_sorted(target: &mut Vec<String>, value: String) {
    if target.iter().any(|existing| existing == &value) {
        return;
    }
    target.push(value);
    target.sort();
}

fn map_trigger_use_case(trigger: Option<&str>) -> Option<&'static str> {
    match trigger {
        Some("manual") => Some("on-demand"),
        Some("auto") => Some("auto-run"),
        Some("always") => Some("always-on"),
        Some("agent-decision") => Some("agent-driven"),
        _ => None,
    }
}

/// Strip the `[plugin: name@marketplace]` annotation the plugin scanner adds
/// to descriptions. Without this, the plugin's *name* gets fed to keyword
/// heuristics — so a plugin called `modularity` would mark every one of its
/// skills as "refactoring" because the rule includes the keyword `modular`.
fn strip_plugin_annotation(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"\[plugin:[^\]]*\]").expect("static regex must compile")
    });
    re.replace_all(text, "").to_string()
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Word-boundary-aware keyword match. The keyword must START at a word
/// boundary in `haystack`; the right-hand side is free to extend further, so
/// "debug" still matches "debugging" but no longer matches "rebugthing".
///
/// Multi-word keywords ("pull request", "stack trace") are specific enough
/// to be safe with plain substring matching, so they bypass the boundary
/// check.
fn keyword_matches_text(keyword: &str, haystack: &str) -> bool {
    if keyword.contains(' ') {
        return haystack.contains(keyword);
    }

    let kw_bytes = keyword.as_bytes();
    let hay_bytes = haystack.as_bytes();
    let kw_len = kw_bytes.len();

    if kw_len == 0 || kw_len > hay_bytes.len() {
        return false;
    }

    let mut i = 0;
    while i + kw_len <= hay_bytes.len() {
        if &hay_bytes[i..i + kw_len] == kw_bytes {
            let left_ok = i == 0 || !is_word_byte(hay_bytes[i - 1]);
            if left_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

pub fn enrich_skill_discovery(skills: &mut [Skill]) {
    for skill in skills.iter_mut() {
        // Seed from anything an earlier pass attached (e.g. the plugin scanner
        // stamps `plugin` here). Without this seed the assignment at the bottom
        // of the loop would erase those upstream signals.
        let mut tags: Vec<String> = skill.discovery_tags.clone();
        tags.sort();
        tags.dedup();
        let mut use_cases: Vec<String> = skill.use_cases.clone();
        use_cases.sort();
        use_cases.dedup();
        let mut hints: Vec<String> = skill.discovery_hints.clone();
        hints.sort();
        hints.dedup();

        match skill.artifact_type {
            crate::models::ArtifactType::Skill => {}
            crate::models::ArtifactType::Command => {
                push_unique_sorted(&mut tags, "commands".to_string());
                push_unique_sorted(&mut use_cases, "on-demand".to_string());
                push_unique_sorted(&mut hints, "artifact-type:command".to_string());
            }
            crate::models::ArtifactType::Hook => {
                push_unique_sorted(&mut tags, "hooks".to_string());
                push_unique_sorted(&mut use_cases, "auto-run".to_string());
                push_unique_sorted(&mut hints, "artifact-type:hook".to_string());
            }
            crate::models::ArtifactType::Rule => {
                push_unique_sorted(&mut tags, "rules".to_string());
                push_unique_sorted(&mut use_cases, "govern".to_string());
                push_unique_sorted(&mut hints, "artifact-type:rule".to_string());
            }
            crate::models::ArtifactType::Workflow => {
                push_unique_sorted(&mut tags, "workflow".to_string());
                push_unique_sorted(&mut use_cases, "automate".to_string());
                push_unique_sorted(&mut hints, "artifact-type:workflow".to_string());
            }
            crate::models::ArtifactType::Prompt => {
                push_unique_sorted(&mut tags, "prompt".to_string());
                push_unique_sorted(&mut use_cases, "assist".to_string());
                push_unique_sorted(&mut hints, "artifact-type:prompt".to_string());
            }
            crate::models::ArtifactType::Config => {
                push_unique_sorted(&mut tags, "config".to_string());
                push_unique_sorted(&mut use_cases, "configure".to_string());
                push_unique_sorted(&mut hints, "artifact-type:config".to_string());
            }
            crate::models::ArtifactType::Other => {
                push_unique_sorted(&mut tags, "other".to_string());
                push_unique_sorted(&mut hints, "artifact-type:other".to_string());
            }
        }

        if let Some(metadata_tags) = &skill.metadata.tags {
            for raw_tag in metadata_tags {
                if let Some(normalized) = normalize_token(raw_tag) {
                    let tag = canonicalize_tag(&normalized);
                    push_unique_sorted(&mut tags, tag.clone());
                    push_unique_sorted(&mut hints, format!("metadata.tags:{}", tag));
                }
            }
        }

        if let Some(metadata_use_cases) = &skill.metadata.use_cases {
            for raw in metadata_use_cases {
                if let Some(normalized) = normalize_token(raw) {
                    let use_case = canonicalize_use_case(&normalized);
                    push_unique_sorted(&mut use_cases, use_case.clone());
                    push_unique_sorted(&mut hints, format!("metadata.use_cases:{}", use_case));
                }
            }
        }

        if let Some(category) = &skill.metadata.category {
            if let Some(normalized) = normalize_token(category) {
                let tag = canonicalize_tag(&normalized);
                push_unique_sorted(&mut tags, tag.clone());
                push_unique_sorted(&mut hints, format!("metadata.category:{}", tag));
            }
        }

        if let Some(trigger) = skill
            .metadata
            .trigger
            .as_ref()
            .and_then(|v| normalize_token(v).map(|n| canonicalize_use_case(&n)))
        {
            if let Some(mapped) = map_trigger_use_case(Some(trigger.as_str())) {
                push_unique_sorted(&mut use_cases, mapped.to_string());
                push_unique_sorted(&mut hints, format!("metadata.trigger:{}", mapped));
            }
        } else if let Some(trigger) = map_trigger_use_case(skill.metadata.trigger.as_deref()) {
            push_unique_sorted(&mut use_cases, trigger.to_string());
            push_unique_sorted(&mut hints, format!("metadata.trigger:{}", trigger));
        }

        let clean_description = strip_plugin_annotation(&skill.description);
        let mut text_parts: Vec<String> = vec![skill.name.clone(), clean_description];
        if let Some(category) = &skill.metadata.category {
            text_parts.push(category.clone());
        }
        if let Some(allowed_tools) = &skill.metadata.allowed_tools {
            text_parts.push(allowed_tools.clone());
        }
        if let Some(globs) = &skill.metadata.globs {
            text_parts.push(globs.join(" "));
        }
        if let Some(language) = &skill.metadata.language {
            text_parts.push(language.clone());
        }
        // `version` deliberately excluded — version strings ("1.0.0", "tag-v2")
        // contributed false matches without ever helping classification.

        let searchable = text_parts.join(" ").to_lowercase();
        for rule in RULES {
            if rule
                .keywords
                .iter()
                .any(|keyword| keyword_matches_text(keyword, &searchable))
            {
                push_unique_sorted(&mut tags, rule.tag.to_string());
                push_unique_sorted(&mut use_cases, rule.use_case.to_string());
                push_unique_sorted(
                    &mut hints,
                    format!("heuristic:{}", rule.keywords[0].replace(' ', "_")),
                );
            }
        }

        if tags.is_empty() {
            push_unique_sorted(&mut tags, "general".to_string());
            push_unique_sorted(&mut hints, "fallback:general".to_string());
        }

        if use_cases.is_empty() {
            push_unique_sorted(&mut use_cases, "explore".to_string());
            push_unique_sorted(&mut hints, "fallback:explore".to_string());
        }

        let canonical_tag_set: HashSet<String> = tags.iter().cloned().collect();
        if canonical_tag_set.contains("testing") && !use_cases.iter().any(|v| v == "verify") {
            push_unique_sorted(&mut use_cases, "verify".to_string());
        }
        if canonical_tag_set.contains("release") && !use_cases.iter().any(|v| v == "ship") {
            push_unique_sorted(&mut use_cases, "ship".to_string());
        }

        skill.discovery_tags = tags;
        skill.use_cases = use_cases;
        skill.discovery_hints = hints;
    }
}

#[cfg(test)]
mod tests {
    use super::enrich_skill_discovery;
    use crate::models::{AgentId, Skill, SkillMetadata, SkillScope};

    fn base_skill(name: &str, description: &str) -> Skill {
        Skill {
            id: format!("test:{}", name),
            name: name.to_string(),
            description: description.to_string(),
            artifact_type: crate::models::ArtifactType::Skill,
            agent_id: AgentId::ClaudeCode,
            source_agents: vec![AgentId::ClaudeCode],
            file_path: format!("C:/tmp/{}/SKILL.md", name),
            source_paths: vec![format!("C:/tmp/{}/SKILL.md", name)],
            legacy_ids: vec![],
            scope: SkillScope::Global,
            project_path: None,
            metadata: SkillMetadata::default(),
            discovery_tags: Vec::new(),
            use_cases: Vec::new(),
            discovery_hints: Vec::new(),
            icon: None,
            starred: false,
            update_available: false,
            installed_at: None,
            parent_id: None,
            children: vec![],
        }
    }

    #[test]
    fn enriches_with_metadata_tags_and_use_cases() {
        let mut skill = base_skill("debug-tool", "Finds hard bugs");
        skill.metadata.tags = Some(vec!["debugging".to_string(), "testing".to_string()]);
        skill.metadata.use_cases = Some(vec!["verify".to_string()]);

        let mut items = vec![skill];
        enrich_skill_discovery(&mut items);

        assert!(items[0].discovery_tags.iter().any(|v| v == "debugging"));
        assert!(items[0].discovery_tags.iter().any(|v| v == "testing"));
        assert!(items[0].use_cases.iter().any(|v| v == "verify"));
    }

    #[test]
    fn enriches_from_heuristics() {
        let skill = base_skill("ship-fast", "Deploy and release production changes safely");
        let mut items = vec![skill];
        enrich_skill_discovery(&mut items);

        assert!(items[0].discovery_tags.iter().any(|v| v == "release"));
        assert!(items[0].use_cases.iter().any(|v| v == "ship"));
    }

    #[test]
    fn adds_fallback_when_no_signal_exists() {
        let skill = base_skill("misc", "Utility helper");
        let mut items = vec![skill];
        enrich_skill_discovery(&mut items);

        assert!(items[0].discovery_tags.iter().any(|v| v == "general"));
        assert!(items[0].use_cases.iter().any(|v| v == "explore"));
    }

    #[test]
    fn keyword_does_not_match_inside_unrelated_word() {
        // Each description is deliberately void of real rule keywords. The
        // substrings ("dry" inside "laundry", "tag" inside "vintage",
        // "ai" inside "claim") used to false-trigger the old contains() check.
        let cases: &[(&str, &str)] = &[
            ("misc-laundry", "Manages laundry within an industry workflow helper"),
            ("misc-vintage", "A vintage furniture marketplace organizer"),
            ("misc-fail", "Helps you claim every available widget edge"),
            ("misc-rapid", "Rapid widget arranger for warehouse rooms"),
            // Old bare-"pr" rule matched "previous"/"preference" via substring.
            ("misc-preview", "Preview the previous preferences sidebar"),
        ];

        for (name, description) in cases {
            let skill = base_skill(name, description);
            let mut items = vec![skill];
            enrich_skill_discovery(&mut items);
            assert_eq!(
                items[0].discovery_tags,
                vec!["general"],
                "leaked tag for `{description}`: {:?}",
                items[0].discovery_tags
            );
        }
    }

    #[test]
    fn keyword_still_matches_inflected_word_at_boundary() {
        // "debug" -> "debugging", "test" -> "testing", "refactor" -> "refactoring"
        let pairs: &[(&str, &str, &str)] = &[
            ("dbg", "Debugging gnarly stack traces", "debugging"),
            ("tst", "Testing happens on every push", "testing"),
            ("rfc", "Refactoring shared helpers", "refactoring"),
        ];

        for (name, description, expected_tag) in pairs {
            let skill = base_skill(name, description);
            let mut items = vec![skill];
            enrich_skill_discovery(&mut items);
            assert!(
                items[0].discovery_tags.iter().any(|t| t == expected_tag),
                "expected `{expected_tag}` in {:?} for {description}",
                items[0].discovery_tags
            );
        }
    }

    #[test]
    fn plugin_annotation_is_stripped_before_heuristics() {
        // Plugin annotation contains "modular" via the marketplace name. Without
        // stripping, every plugin skill would inherit the `refactoring` tag.
        let mut skill = base_skill("status", "Show status [plugin: modularity@vladikk-modularity]");
        skill.metadata.tags = None;
        let mut items = vec![skill];
        enrich_skill_discovery(&mut items);

        assert!(
            !items[0].discovery_tags.iter().any(|t| t == "refactoring"),
            "plugin annotation leaked through: {:?}",
            items[0].discovery_tags
        );
    }

    #[test]
    fn preserves_upstream_tags_use_cases_and_hints() {
        // The plugin scanner stamps "plugin" on every plugin-sourced skill
        // before enrichment runs. Enrichment must keep that signal alive.
        let mut skill = base_skill("plug-skill", "A skill from a plugin");
        skill.discovery_tags = vec!["plugin".to_string()];
        skill.use_cases = vec!["seed-use-case".to_string()];
        skill.discovery_hints = vec!["seed-hint".to_string()];

        let mut items = vec![skill];
        enrich_skill_discovery(&mut items);

        assert!(items[0].discovery_tags.iter().any(|t| t == "plugin"));
        assert!(items[0].use_cases.iter().any(|t| t == "seed-use-case"));
        assert!(items[0].discovery_hints.iter().any(|t| t == "seed-hint"));
    }
}
