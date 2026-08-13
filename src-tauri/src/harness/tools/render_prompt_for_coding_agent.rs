// @agent-context: render_prompt_for_coding_agent — final-form prompt builder.

use crate::harness::tools::{parse_args_or_teach, HarnessContext, ToolError, ToolResult};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

const NAME: &str = "render_prompt_for_coding_agent";
const DESCRIPTION: &str = "Translate the user's intent + selected skills into a final-form prompt the user can paste into their coding agent (Claude Code, Codex, Cursor, etc.). Returns the prompt text. Use whenever the user asks 'make me a prompt', 'turn this into a prompt', 'build a prompt for X'.";
const EXAMPLE: &str = r#"{"target_agent":"claude-code","intent":"refactor my rust api","skill_ids":["artifact:3fd95f3cd2b3"]}"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InnerArgs {
    target_agent: String,
    intent: String,
    skill_ids: Vec<String>,
    #[serde(
        default,
        deserialize_with = "crate::harness::tools::serde_helpers::null_str_to_none"
    )]
    extra_instructions: Option<String>,
}

#[derive(Clone)]
pub struct RenderPromptForCodingAgent {
    ctx: HarnessContext,
}

impl RenderPromptForCodingAgent {
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

impl Tool for RenderPromptForCodingAgent {
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
            "required": ["target_agent", "intent", "skill_ids"],
            "properties": {
                "target_agent": { "type": "string" },
                "intent": { "type": "string" },
                "skill_ids": { "type": "array", "items": { "type": "string" } },
                "extra_instructions": { "type": "string" }
            }
        }),
    }
}

async fn run(ctx: &HarnessContext, args: InnerArgs) -> Result<ToolResult, ToolError> {
    let start = Instant::now();
    if args.skill_ids.is_empty() {
        return Err(ToolError::InvalidArgs(
            "tool 'render_prompt_for_coding_agent' refused: `skill_ids` is empty. \
             You must FIRST call `search_skills` or `translate_to_skills` to get real skill ids, \
             THEN pass those ids (they start with `artifact:`) into this tool. \
             Rendering a prompt with zero skills is never useful — it would just echo the intent back. \
             Either gather skills first, or skip this tool entirely and answer the user directly."
                .to_string(),
        ));
    }
    let mut chosen: Vec<&crate::models::Skill> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    for id in &args.skill_ids {
        match ctx
            .skills
            .iter()
            .find(|sk| &sk.id == id || sk.legacy_ids.iter().any(|l| l == id))
        {
            Some(s) => chosen.push(s),
            None => unresolved.push(id.clone()),
        }
    }
    if chosen.is_empty() {
        return Err(ToolError::InvalidArgs(format!(
            "tool 'render_prompt_for_coding_agent' refused: none of the supplied `skill_ids` resolved \
             to a real installed skill. You passed [{}]. Skill ids look like `artifact:3fd95f3cd2b3`, \
             NOT tool names like `web_search` or `search_skills`. Call `search_skills` or \
             `translate_to_skills` first, take the `id` field off each returned skill, then call \
             this tool again with those exact strings.",
            unresolved.iter().take(5).map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(", ")
        )));
    }
    let invocation_line = chosen
        .iter()
        .map(|s| {
            s.metadata
                .slash_command
                .clone()
                .unwrap_or_else(|| format!("`{}`", s.name))
        })
        .collect::<Vec<_>>()
        .join(" + ");
    let skill_block = chosen
        .iter()
        .map(|s| {
            format!(
                "- **{}** ({}): {}",
                s.name,
                s.metadata
                    .slash_command
                    .clone()
                    .unwrap_or_else(|| format!("skill:{}", s.id)),
                s.description
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let extra_block = match args.extra_instructions.as_deref() {
        Some(extra) if !extra.trim().is_empty() => {
            format!("\nExtra constraints:\n{}\n", extra.trim())
        }
        _ => String::new(),
    };
    let lead = if chosen.len() > 1 { " (in order)" } else { "" };
    let skills_text = if skill_block.is_empty() {
        "(no matching skills found in your registry — proceed with general knowledge)".to_string()
    } else {
        skill_block
    };
    let invocation = if invocation_line.is_empty() {
        "n/a".to_string()
    } else {
        invocation_line
    };

    let prompt = format!(
        "Target agent: {agent}\n\nGoal:\n{intent}\n\nUse these skills{lead}:\n{skills}\n{extra}\nInstructions for the agent:\n1. Load each listed skill before starting.\n2. Apply them in the order given.\n3. Report results in the format the skills define.\n\nQuick invocation: {invocation}",
        agent = args.target_agent,
        intent = args.intent.trim(),
        lead = lead,
        skills = skills_text,
        extra = extra_block,
        invocation = invocation,
    );

    let snippet = format!(
        "prompt for {} using {} skill(s)",
        args.target_agent,
        chosen.len()
    );

    Ok(ToolResult {
        title: format!("{NAME} → {} ({} skill(s))", args.target_agent, chosen.len()),
        kind: "prompt-render".into(),
        snippet: ToolResult::snippet_from(&snippet),
        data: json!({
            "target_agent": args.target_agent,
            "prompt": prompt,
            "skill_count": chosen.len(),
        }),
        items_count: Some(1),
        source: Some("local".into()),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}
