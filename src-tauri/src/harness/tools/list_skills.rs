// @agent-context: list_skills — structured listing with rich filters.
// Combines: agent_id, scope, starred_only, tag, language, installed/updated time, limit.

use crate::harness::tools::{
    agent_id_str, merged_tags, parse_args_or_teach, search_skills::SkillSummary, HarnessContext,
    ToolError, ToolResult,
};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

const NAME: &str = "list_skills";
const DESCRIPTION: &str = "Structured listing with rich filters. Combine filters freely. Use this for questions like 'what skills did I install today', 'which Cursor skills do I have', 'show me starred skills with the rust tag'. NOTE: this tool has NO `query` field — use search_skills for natural-language queries. Time filters take unix seconds; ALWAYS use the named time constants provided in the runtime context, never invent timestamps.";
const EXAMPLE: &str = r#"{"agent_id":"claude-code","tag":"rust","limit":20}"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InnerArgs {
    #[serde(
        default,
        deserialize_with = "crate::harness::tools::serde_helpers::null_str_to_none"
    )]
    agent_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::harness::tools::serde_helpers::null_str_to_none"
    )]
    scope: Option<String>,
    #[serde(default)]
    starred_only: Option<bool>,
    #[serde(
        default,
        deserialize_with = "crate::harness::tools::serde_helpers::null_str_to_none"
    )]
    tag: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::harness::tools::serde_helpers::null_str_to_none"
    )]
    language: Option<String>,
    #[serde(default)]
    installed_since_unix: Option<u64>,
    #[serde(default)]
    updated_since_unix: Option<u64>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Clone)]
pub struct ListSkills {
    ctx: HarnessContext,
}

impl ListSkills {
    pub fn new(ctx: HarnessContext) -> Self {
        Self { ctx }
    }
    pub async fn call_with_json(&self, args_json: &str) -> Result<ToolResult, ToolError> {
        let value: Value = serde_json::from_str(args_json).map_err(|e| {
            ToolError::InvalidArgs(format!(
                "tool '{NAME}' got malformed JSON: {e}. Example: {EXAMPLE}"
            ))
        })?;
        let args: InnerArgs = parse_args_or_teach(&value, NAME, EXAMPLE)?;
        run(&self.ctx, args).await
    }
}

impl Tool for ListSkills {
    const NAME: &'static str = NAME;
    type Error = ToolError;
    type Args = Value;
    type Output = ToolResult;
    async fn definition(&self, _: String) -> ToolDefinition {
        definition()
    }
    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let parsed: InnerArgs = parse_args_or_teach(&args, NAME, EXAMPLE)?;
        run(&self.ctx, parsed).await
    }
}

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: NAME.into(),
        description: DESCRIPTION.into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "agent_id": { "type": "string" },
                "scope": { "type": "string", "enum": ["global", "project"] },
                "starred_only": { "type": "boolean" },
                "tag": { "type": "string" },
                "language": { "type": "string" },
                "installed_since_unix": { "type": "integer" },
                "updated_since_unix": { "type": "integer" },
                "limit": { "type": "integer", "description": "Default 40, hard cap 120." }
            }
        }),
    }
}

async fn run(ctx: &HarnessContext, args: InnerArgs) -> Result<ToolResult, ToolError> {
    let start = Instant::now();
    let starred_only = args.starred_only.unwrap_or(false);
    let tag = args.tag.as_ref().map(|s| s.to_lowercase());
    let language = args.language.as_ref().map(|s| s.to_lowercase());
    let limit = args.limit.unwrap_or(40).clamp(1, 120);

    let filtered: Vec<SkillSummary> = ctx
        .skills
        .iter()
        .filter(|s| {
            if let Some(target) = &args.agent_id {
                if !agent_id_str(s).eq_ignore_ascii_case(target) {
                    return false;
                }
            }
            if let Some(scope_filter) = &args.scope {
                let matches = matches!(
                    (scope_filter.as_str(), &s.scope),
                    ("global", crate::models::SkillScope::Global)
                        | ("project", crate::models::SkillScope::Project)
                );
                if !matches {
                    return false;
                }
            }
            if starred_only && !s.starred {
                return false;
            }
            if let Some(t) = &tag {
                if !merged_tags(s).iter().any(|x| x.eq_ignore_ascii_case(t)) {
                    return false;
                }
            }
            if let Some(lang) = &language {
                if !s
                    .metadata
                    .language
                    .as_ref()
                    .map(|l| l.eq_ignore_ascii_case(lang))
                    .unwrap_or(false)
                {
                    return false;
                }
            }
            if let Some(since) = args.installed_since_unix {
                if s.installed_at.unwrap_or(0) < since {
                    return false;
                }
            }
            if let Some(since) = args.updated_since_unix {
                if s.last_modified_at.unwrap_or(0) < since {
                    return false;
                }
            }
            true
        })
        .take(limit)
        .map(|s| SkillSummary::from_skill(s, None))
        .collect();

    let count = filtered.len();
    let mut filter_desc = Vec::new();
    if let Some(a) = &args.agent_id {
        filter_desc.push(format!("agent={a}"));
    }
    if let Some(sc) = &args.scope {
        filter_desc.push(format!("scope={sc}"));
    }
    if starred_only {
        filter_desc.push("starred".into());
    }
    if let Some(t) = &tag {
        filter_desc.push(format!("tag={t}"));
    }
    if let Some(l) = &language {
        filter_desc.push(format!("lang={l}"));
    }
    if args.installed_since_unix.is_some() {
        filter_desc.push("installed-since".into());
    }
    if args.updated_since_unix.is_some() {
        filter_desc.push("updated-since".into());
    }
    let label = if filter_desc.is_empty() {
        "all".into()
    } else {
        filter_desc.join(",")
    };

    let snippet = if count == 0 {
        format!("no skills matched filters [{label}]")
    } else {
        let names: Vec<&str> = filtered.iter().take(5).map(|s| s.name.as_str()).collect();
        format!("{count} matches; first: {}", names.join(", "))
    };

    Ok(ToolResult {
        title: format!("{NAME}({label}) → {count} result(s)"),
        kind: "skill-list".into(),
        snippet: ToolResult::snippet_from(&snippet),
        data: json!({ "count": count, "filters_applied": filter_desc, "skills": filtered }),
        items_count: Some(count),
        source: Some("local".into()),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}
