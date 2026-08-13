// @agent-context: Tool framework — uniform ToolResult envelope and shared context.
//
// Each concrete tool is a struct that implements rig's `Tool` trait so it can
// be attached to a rig::Agent via `.tool(...)`. The same struct also exposes
// `call_with_json(args_json) -> Result<ToolResult, ToolError>` so the CLI
// (and tests) can invoke tools by name without a full agent loop.

#![allow(dead_code)]

pub mod combine_skills_workflow;
/// Parse a JSON value into a strict args struct, returning a model-teaching
/// error string when the parse fails. This bridges between rig's "Args is a
/// typed struct" pattern (where deserialization errors surface as terse
/// `unknown field X, expected Y` messages and the LLM then hallucinates
/// results around them) and our want for a clear, actionable error that
/// instructs the model to retry with correct args.
pub fn parse_args_or_teach<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    tool_name: &str,
    example_json: &str,
) -> Result<T, ToolError> {
    serde_json::from_value::<T>(value.clone()).map_err(|e| {
        ToolError::InvalidArgs(format!(
            "tool '{tool_name}' received invalid arguments: {e}. \
             Retry this exact tool with the correct shape. \
             Valid example: {example_json}. \
             CRITICAL: do NOT fabricate results, do NOT pretend the call succeeded, do NOT invent items. \
             Either call the tool again with corrected args, or pick a different tool."
        ))
    })
}

/// Serde helpers used by every tool's `Args` to be hermes-grade resilient.
pub mod serde_helpers {
    use serde::{Deserialize, Deserializer};

    /// Deserialize `Option<String>`, mapping `""`, `"null"`, `"none"`, `"undefined"`,
    /// whitespace-only, etc. to `None`. Smaller models love passing literal
    /// `"null"` for optional fields; this turns that into the absence the tool
    /// expects.
    pub fn null_str_to_none<'de, D>(de: D) -> Result<Option<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<serde_json::Value>::deserialize(de)?;
        match opt {
            None => Ok(None),
            Some(serde_json::Value::Null) => Ok(None),
            Some(serde_json::Value::String(s)) => {
                let trimmed = s.trim();
                let lower = trimmed.to_lowercase();
                if trimmed.is_empty()
                    || lower == "null"
                    || lower == "none"
                    || lower == "undefined"
                    || lower == "nil"
                {
                    Ok(None)
                } else {
                    Ok(Some(s))
                }
            }
            Some(other) => {
                // Unexpected non-string non-null — best effort: try its string form.
                Ok(Some(other.to_string()))
            }
        }
    }
}

pub mod get_skill_detail;
pub mod get_skill_stats;
pub mod list_skills;
pub mod model_projection;
pub mod render_prompt_for_coding_agent;
pub mod search_marketplace;
pub mod search_skills;
pub mod translate_to_skills;
pub mod web_fetch;
pub mod web_search;

use crate::harness::config::HarnessSettings;
use crate::models::Skill;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// Uniform shape every tool returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub title: String,
    pub kind: String,
    pub snippet: String,
    pub data: Value,
    pub items_count: Option<usize>,
    pub source: Option<String>,
    pub elapsed_ms: u64,
}

impl ToolResult {
    pub fn snippet_from(text: &str) -> String {
        const MAX: usize = 280;
        let trimmed = text.trim();
        if trimmed.chars().count() <= MAX {
            trimmed.to_string()
        } else {
            let mut s: String = trimmed.chars().take(MAX - 1).collect();
            s.push('…');
            s
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("execution failed: {0}")]
    Execution(String),
    #[error("network error: {0}")]
    Network(String),
}

impl From<reqwest::Error> for ToolError {
    fn from(e: reqwest::Error) -> Self {
        ToolError::Network(e.to_string())
    }
}

/// Shared dependencies passed to every tool at construction.
#[derive(Clone)]
pub struct HarnessContext {
    pub skills: Arc<Vec<Skill>>,
    pub settings: Arc<HarnessSettings>,
    pub http: reqwest::Client,
}

impl HarnessContext {
    /// Fallible on purpose: building the reqwest client can fail when the OS
    /// TLS stack cannot initialize. That is an environment problem the caller
    /// must surface to the user — panicking here took the whole app down.
    pub fn new(skills: Vec<Skill>, settings: HarnessSettings) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent("skill-deck-harness/0.1")
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build harness http client (TLS init?): {e}"))?;
        Ok(Self {
            skills: Arc::new(skills),
            settings: Arc::new(settings),
            http,
        })
    }
}

/// List every tool definition the harness knows about. Used by `harness-cli
/// tools list` and by the orchestrator agent assembly to register the rig tools.
pub fn list_definitions() -> Vec<rig::completion::ToolDefinition> {
    vec![
        search_skills::definition(),
        list_skills::definition(),
        get_skill_detail::definition(),
        get_skill_stats::definition(),
        search_marketplace::definition(),
        web_search::definition(),
        web_fetch::definition(),
        translate_to_skills::definition(),
        combine_skills_workflow::definition(),
        render_prompt_for_coding_agent::definition(),
    ]
}

/// Look up a tool by name and dispatch a JSON args payload. Used by the CLI
/// and by tests; production agent flows use rig's typed `Tool` integration.
pub async fn dispatch(
    name: &str,
    args_json: &str,
    ctx: &HarnessContext,
) -> Result<ToolResult, ToolError> {
    match name {
        "search_skills" => {
            search_skills::SearchSkills::new(ctx.clone())
                .call_with_json(args_json)
                .await
        }
        "list_skills" => {
            list_skills::ListSkills::new(ctx.clone())
                .call_with_json(args_json)
                .await
        }
        "get_skill_detail" => {
            get_skill_detail::GetSkillDetail::new(ctx.clone())
                .call_with_json(args_json)
                .await
        }
        "get_skill_stats" => {
            get_skill_stats::GetSkillStats::new(ctx.clone())
                .call_with_json(args_json)
                .await
        }
        "search_marketplace" => {
            search_marketplace::SearchMarketplace::new(ctx.clone())
                .call_with_json(args_json)
                .await
        }
        "web_search" => {
            web_search::WebSearch::new(ctx.clone())
                .call_with_json(args_json)
                .await
        }
        "web_fetch" => {
            web_fetch::WebFetch::new(ctx.clone())
                .call_with_json(args_json)
                .await
        }
        "translate_to_skills" => {
            translate_to_skills::TranslateToSkills::new(ctx.clone())
                .call_with_json(args_json)
                .await
        }
        "combine_skills_workflow" => {
            combine_skills_workflow::CombineSkillsWorkflow::new(ctx.clone())
                .call_with_json(args_json)
                .await
        }
        "render_prompt_for_coding_agent" => {
            render_prompt_for_coding_agent::RenderPromptForCodingAgent::new(ctx.clone())
                .call_with_json(args_json)
                .await
        }
        other => Err(ToolError::InvalidArgs(format!("unknown tool: {other}"))),
    }
}

/// Convenience snapshot of merged tags (frontmatter + discovery).
pub fn merged_tags(s: &Skill) -> Vec<String> {
    let mut tags: Vec<String> = s.discovery_tags.clone();
    if let Some(meta_tags) = s.metadata.tags.as_ref() {
        for t in meta_tags {
            if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                tags.push(t.clone());
            }
        }
    }
    tags
}

/// Render an agent_id enum into its kebab string form for tool output.
pub fn agent_id_str(s: &Skill) -> String {
    serde_json::to_string(&s.agent_id)
        .unwrap_or_else(|_| "\"unknown\"".to_string())
        .trim_matches('"')
        .to_string()
}
