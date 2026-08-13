// @agent-context: combine_skills_workflow — render ordered steps for a list of skill_ids.

use crate::harness::tools::{
    merged_tags, parse_args_or_teach, HarnessContext, ToolError, ToolResult,
};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

const NAME: &str = "combine_skills_workflow";
const DESCRIPTION: &str = "Compose an ordered workflow from multiple skill_ids for a stated intent. Returns structured steps the agent can summarize or feed into render_prompt_for_coding_agent. Use whenever the user asks to combine, chain, or compose two or more skills.";
const EXAMPLE: &str = r#"{"skill_ids":["artifact:3fd95f3cd2b3","artifact:44739dd8439d"],"intent":"refactor my rust api"}"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InnerArgs {
    skill_ids: Vec<String>,
    intent: String,
}

#[derive(Clone)]
pub struct CombineSkillsWorkflow {
    ctx: HarnessContext,
}

impl CombineSkillsWorkflow {
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

impl Tool for CombineSkillsWorkflow {
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
            "required": ["skill_ids", "intent"],
            "properties": {
                "skill_ids": { "type": "array", "items": { "type": "string" } },
                "intent": { "type": "string" }
            }
        }),
    }
}

async fn run(ctx: &HarnessContext, args: InnerArgs) -> Result<ToolResult, ToolError> {
    let start = Instant::now();
    if args.skill_ids.is_empty() {
        return Err(ToolError::InvalidArgs("skill_ids must not be empty".into()));
    }
    let mut steps = Vec::new();
    for (i, id) in args.skill_ids.iter().enumerate() {
        let skill = ctx
            .skills
            .iter()
            .find(|s| &s.id == id || s.legacy_ids.iter().any(|l| l == id));
        steps.push(match skill {
            Some(s) => json!({
                "step": i + 1,
                "skill_id": s.id,
                "skill_name": s.name,
                "purpose": s.description,
                "invocation_hint": s.metadata.slash_command.clone()
                    .unwrap_or_else(|| format!("invoke skill: {}", s.name)),
                "tags": merged_tags(s),
            }),
            None => json!({
                "step": i + 1,
                "skill_id": id,
                "skill_name": null,
                "purpose": "skill not found in current registry",
                "invocation_hint": null,
            }),
        });
    }
    let count = steps.len();
    let snippet = format!(
        "{count} step(s) for intent: {}",
        args.intent.chars().take(60).collect::<String>()
    );

    Ok(ToolResult {
        title: format!("{NAME} ({count} step(s))"),
        kind: "workflow-combine".into(),
        snippet: ToolResult::snippet_from(&snippet),
        data: json!({ "intent": args.intent, "steps": steps }),
        items_count: Some(count),
        source: Some("local".into()),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}
