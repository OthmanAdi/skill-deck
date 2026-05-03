use crate::models::Skill;
use std::collections::HashSet;

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
            "pr",
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
        .replace('_', " ")
        .replace('-', " ")
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

pub fn enrich_skill_discovery(skills: &mut [Skill]) {
    for skill in skills.iter_mut() {
        let mut tags: Vec<String> = Vec::new();
        let mut use_cases: Vec<String> = Vec::new();
        let mut hints: Vec<String> = Vec::new();

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

        let mut text_parts: Vec<String> = vec![skill.name.clone(), skill.description.clone()];
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
        if let Some(version) = &skill.metadata.version {
            text_parts.push(version.clone());
        }

        let searchable = text_parts.join(" ").to_lowercase();
        for rule in RULES {
            if rule
                .keywords
                .iter()
                .any(|keyword| searchable.contains(keyword))
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
            agent_id: AgentId::ClaudeCode,
            file_path: format!("C:/tmp/{}/SKILL.md", name),
            scope: SkillScope::Global,
            project_path: None,
            metadata: SkillMetadata::default(),
            discovery_tags: Vec::new(),
            use_cases: Vec::new(),
            discovery_hints: Vec::new(),
            icon: None,
            starred: false,
            update_available: false,
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
}
