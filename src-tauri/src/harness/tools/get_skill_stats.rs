// @agent-context: get_skill_stats — analytical roll-ups (counts grouped by agent, tag, scope, language, etc.).

use crate::harness::tools::{
    agent_id_str, merged_tags, parse_args_or_teach, HarnessContext, ToolError, ToolResult,
};
use chrono::{Datelike, TimeZone};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Instant;

const NAME: &str = "get_skill_stats";
const DESCRIPTION: &str = "Cheap analytical roll-ups over the skill registry. Use for 'how many skills do I have', 'which agent has the most skills', 'what tags appear most', 'how many skills did I install this month'. Returns counts grouped by the requested axis — does NOT return skill bodies. NOTE: this tool only accepts `group_by` + optional `top_n`. No `limit`, no `query`, no `tag`.";
const EXAMPLE: &str = r#"{"group_by":"agent_id","top_n":10}"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InnerArgs {
    group_by: String,
    #[serde(default)]
    top_n: Option<usize>,
}

#[derive(Clone)]
pub struct GetSkillStats {
    ctx: HarnessContext,
}

impl GetSkillStats {
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

impl Tool for GetSkillStats {
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
            "required": ["group_by"],
            "properties": {
                "group_by": {
                    "type": "string",
                    "enum": ["agent_id", "tag", "scope", "language", "install_month", "update_month", "starred"]
                },
                "top_n": { "type": "integer", "description": "Default 20." }
            }
        }),
    }
}

async fn run(ctx: &HarnessContext, args: InnerArgs) -> Result<ToolResult, ToolError> {
    let start = Instant::now();
    let top_n = args.top_n.unwrap_or(20).clamp(1, 200);
    let mut counts: HashMap<String, u32> = HashMap::new();
    for s in ctx.skills.iter() {
        match args.group_by.as_str() {
            "agent_id" => {
                *counts.entry(agent_id_str(s)).or_insert(0) += 1;
            }
            "tag" => {
                for t in merged_tags(s) {
                    *counts.entry(t.to_lowercase()).or_insert(0) += 1;
                }
            }
            "scope" => {
                let key = match s.scope {
                    crate::models::SkillScope::Global => "global",
                    crate::models::SkillScope::Project => "project",
                }
                .to_string();
                *counts.entry(key).or_insert(0) += 1;
            }
            "language" => {
                if let Some(l) = s.metadata.language.as_ref() {
                    *counts.entry(l.to_lowercase()).or_insert(0) += 1;
                }
            }
            "install_month" => {
                if let Some(ts) = s.installed_at {
                    if let Some(key) = month_key(ts) {
                        *counts.entry(key).or_insert(0) += 1;
                    }
                }
            }
            "update_month" => {
                if let Some(ts) = s.last_modified_at {
                    if let Some(key) = month_key(ts) {
                        *counts.entry(key).or_insert(0) += 1;
                    }
                }
            }
            "starred" => {
                let key = if s.starred { "starred" } else { "unstarred" }.to_string();
                *counts.entry(key).or_insert(0) += 1;
            }
            other => return Err(ToolError::InvalidArgs(format!("unknown group_by: {other}"))),
        }
    }
    let mut ranked: Vec<(String, u32)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(top_n);
    let total: u32 = ranked.iter().map(|(_, c)| *c).sum();
    let snippet = if let Some((k, c)) = ranked.first() {
        format!("{} group(s); top: {} ({})", ranked.len(), k, c)
    } else {
        "no data".to_string()
    };
    Ok(ToolResult {
        title: format!(
            "{NAME}(group_by={}) → {} group(s)",
            args.group_by,
            ranked.len()
        ),
        kind: "skill-stats".into(),
        snippet,
        data: json!({
            "group_by": args.group_by,
            "total_count": total,
            "groups": ranked.iter().map(|(k, v)| json!({ "key": k, "count": v })).collect::<Vec<_>>(),
        }),
        items_count: Some(ranked.len()),
        source: Some("local".into()),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

fn month_key(unix_seconds: u64) -> Option<String> {
    let dt = chrono::Utc.timestamp_opt(unix_seconds as i64, 0).single()?;
    Some(format!("{:04}-{:02}", dt.year(), dt.month()))
}
