// @agent-context: get_skill_detail — full body + frontmatter for one skill.

use crate::harness::tools::{
    parse_args_or_teach, search_skills::SkillSummary, HarnessContext, ToolError, ToolResult,
};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

const NAME: &str = "get_skill_detail";
const DESCRIPTION: &str = "Fetch the full markdown body + parsed frontmatter for one skill. Call this after search_skills / list_skills when you need actual content to reason about how a skill works or to compose a prompt that uses it.";
const EXAMPLE: &str = r#"{"skill_id":"artifact:3fd95f3cd2b3"}"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InnerArgs {
    skill_id: String,
    #[serde(default)]
    body_char_cap: Option<usize>,
}

#[derive(Clone)]
pub struct GetSkillDetail {
    ctx: HarnessContext,
}

impl GetSkillDetail {
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

impl Tool for GetSkillDetail {
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
            "required": ["skill_id"],
            "properties": {
                "skill_id": { "type": "string" },
                "body_char_cap": { "type": "integer", "description": "Optional cap on body chars; default 16000." }
            }
        }),
    }
}

async fn run(ctx: &HarnessContext, args: InnerArgs) -> Result<ToolResult, ToolError> {
    let start = Instant::now();
    let skill = ctx
        .skills
        .iter()
        .find(|s| s.id == args.skill_id || s.legacy_ids.iter().any(|l| l == &args.skill_id))
        .ok_or_else(|| ToolError::Execution(format!("skill not found: {}", args.skill_id)))?;

    let cap = args.body_char_cap.unwrap_or(16000);
    let body = std::fs::read_to_string(&skill.file_path)
        .map_err(|_| ToolError::Execution("failed to read skill body".into()))?;
    let body_capped = if body.chars().count() > cap {
        let prefix: String = body.chars().take(cap).collect();
        format!("{prefix}\n\n…[truncated]")
    } else {
        body.clone()
    };

    let snippet = ToolResult::snippet_from(&skill.description);
    Ok(ToolResult {
        title: format!("{NAME}({})", skill.name),
        kind: "skill-detail".into(),
        snippet,
        data: json!({
            "skill": SkillSummary::from_skill(skill, None),
            "file_path": skill.file_path,
            "metadata": skill.metadata,
            "body": body_capped,
            "body_char_count": body.chars().count(),
        }),
        items_count: Some(1),
        source: Some("local".into()),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}
