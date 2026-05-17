// @agent-context: Skill-aware tools exposed to the LLM via tool calls.
//
// Each tool is a pure function over the current skill snapshot + arguments.
// We deliberately keep tools small and composable — the model is responsible
// for chaining list -> get_detail -> combine -> render_prompt rather than
// asking us to provide one mega-tool. Smaller tools also produce cleaner
// tool cards in the UI.

use crate::models::Skill;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::provider::ToolDefinition;

pub fn all_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "list_skills".into(),
            description: "List the user's installed skills. Use filters to narrow results. Always prefer search_skills for keyword queries; use this when you need a structured slice (e.g., all skills for a specific agent, or all global scope).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Optional agent id filter (e.g. claude-code, codex, cursor)." },
                    "scope": { "type": "string", "enum": ["global", "project"], "description": "Optional scope filter." },
                    "starred_only": { "type": "boolean", "description": "Only return user-starred skills." },
                    "limit": { "type": "integer", "description": "Max results, default 30, hard cap 80." }
                }
            }),
        },
        ToolDefinition {
            name: "search_skills".into(),
            description: "Keyword search over skill name + description + tags. Returns ranked matches. Use this for any natural-language query about which skills exist.".into(),
            parameters: json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string", "description": "Keywords or natural-language phrase." },
                    "limit": { "type": "integer", "description": "Max results, default 12, hard cap 40." }
                }
            }),
        },
        ToolDefinition {
            name: "get_skill_detail".into(),
            description: "Fetch the full content and frontmatter of one skill by its id. Use after list_skills/search_skills when you need the body to reason about how to use it.".into(),
            parameters: json!({
                "type": "object",
                "required": ["skill_id"],
                "properties": {
                    "skill_id": { "type": "string", "description": "Skill id as returned by list_skills/search_skills." }
                }
            }),
        },
        ToolDefinition {
            name: "combine_skills_workflow".into(),
            description: "Compose multiple skills into an ordered workflow for an intent. Returns a structured plan listing each step + which skill drives it. Use BEFORE render_prompt_for_coding_agent when more than one skill is involved.".into(),
            parameters: json!({
                "type": "object",
                "required": ["skill_ids", "intent"],
                "properties": {
                    "skill_ids": { "type": "array", "items": { "type": "string" }, "description": "Ordered list of skill ids." },
                    "intent": { "type": "string", "description": "What the user is trying to accomplish in their own words." }
                }
            }),
        },
        ToolDefinition {
            name: "render_prompt_for_coding_agent".into(),
            description: "Translate the user's intent + selected skills into a final-form prompt the user can paste into their coding agent. Returns the prompt text.".into(),
            parameters: json!({
                "type": "object",
                "required": ["target_agent", "intent", "skill_ids"],
                "properties": {
                    "target_agent": {
                        "type": "string",
                        "description": "Target coding agent id (claude-code, codex, cursor, etc.). Determines invocation syntax (e.g. /skill-name)."
                    },
                    "intent": { "type": "string", "description": "What the user wants the coding agent to do, in detail." },
                    "skill_ids": { "type": "array", "items": { "type": "string" }, "description": "Ordered skill ids to reference." }
                }
            }),
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub agent_id: String,
    pub scope: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified_at: Option<u64>,
    pub starred: bool,
    pub slash_command: Option<String>,
}

impl SkillSummary {
    pub fn from_skill(s: &Skill) -> Self {
        let agent_id = serde_json::to_string(&s.agent_id)
            .unwrap_or_else(|_| "\"unknown\"".to_string())
            .trim_matches('"')
            .to_string();
        let scope = match s.scope {
            crate::models::SkillScope::Global => "global".to_string(),
            crate::models::SkillScope::Project => "project".to_string(),
        };
        let mut tags: Vec<String> = s.discovery_tags.clone();
        if let Some(meta_tags) = s.metadata.tags.as_ref() {
            for t in meta_tags {
                if !tags.contains(t) {
                    tags.push(t.clone());
                }
            }
        }
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            description: s.description.clone(),
            agent_id,
            scope,
            tags,
            installed_at: s.installed_at,
            last_modified_at: s.last_modified_at,
            starred: s.starred,
            slash_command: s.metadata.slash_command.clone(),
        }
    }
}

/// Result returned from a tool execution — JSON value plus a short label
/// the UI uses on the tool card.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub label: String,
    pub data: Value,
}

/// Dispatch one tool call by name. Returns an error string if the tool is
/// unknown or arguments are invalid — that string is passed back to the
/// model as the tool response so it can recover.
pub fn dispatch_tool(
    name: &str,
    arguments_json: &str,
    skills: &[Skill],
) -> Result<ToolResult, String> {
    let args: Value = serde_json::from_str(arguments_json).unwrap_or(Value::Object(Default::default()));
    match name {
        "list_skills" => list_skills(&args, skills),
        "search_skills" => search_skills(&args, skills),
        "get_skill_detail" => get_skill_detail(&args, skills),
        "combine_skills_workflow" => combine_skills_workflow(&args, skills),
        "render_prompt_for_coding_agent" => render_prompt_for_coding_agent(&args, skills),
        other => Err(format!("unknown tool: {}", other)),
    }
}

fn list_skills(args: &Value, skills: &[Skill]) -> Result<ToolResult, String> {
    let agent_id = args.get("agent_id").and_then(|v| v.as_str());
    let scope = args.get("scope").and_then(|v| v.as_str());
    let starred_only = args.get("starred_only").and_then(|v| v.as_bool()).unwrap_or(false);
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(30)
        .min(80) as usize;

    let filtered: Vec<SkillSummary> = skills
        .iter()
        .filter(|s| {
            if let Some(target) = agent_id {
                let aid = serde_json::to_string(&s.agent_id)
                    .unwrap_or_else(|_| "\"unknown\"".to_string());
                if !aid.trim_matches('"').eq_ignore_ascii_case(target) {
                    return false;
                }
            }
            if let Some(scope_filter) = scope {
                let matches = match (scope_filter, &s.scope) {
                    ("global", crate::models::SkillScope::Global) => true,
                    ("project", crate::models::SkillScope::Project) => true,
                    _ => false,
                };
                if !matches {
                    return false;
                }
            }
            if starred_only && !s.starred {
                return false;
            }
            true
        })
        .take(limit)
        .map(SkillSummary::from_skill)
        .collect();

    let count = filtered.len();
    Ok(ToolResult {
        label: format!("list_skills → {} result(s)", count),
        data: json!({
            "count": count,
            "skills": filtered,
        }),
    })
}

fn search_skills(args: &Value, skills: &[Skill]) -> Result<ToolResult, String> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "search_skills: 'query' is required".to_string())?
        .to_lowercase();
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(12)
        .min(40) as usize;

    if query.trim().is_empty() {
        return Ok(ToolResult {
            label: "search_skills → empty query".into(),
            data: json!({ "count": 0, "skills": [] }),
        });
    }

    let terms: Vec<String> = query
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect();

    let mut scored: Vec<(i64, &Skill)> = skills
        .iter()
        .filter_map(|s| {
            let hay_name = s.name.to_lowercase();
            let hay_desc = s.description.to_lowercase();
            let mut score: i64 = 0;
            for term in &terms {
                if hay_name.contains(term) {
                    score += 12;
                }
                if hay_desc.contains(term) {
                    score += 5;
                }
                if s.discovery_tags.iter().any(|t| t.to_lowercase().contains(term)) {
                    score += 6;
                }
                if let Some(tags) = s.metadata.tags.as_ref() {
                    if tags.iter().any(|t| t.to_lowercase().contains(term)) {
                        score += 6;
                    }
                }
            }
            if score == 0 {
                None
            } else {
                Some((score, s))
            }
        })
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let trimmed: Vec<SkillSummary> = scored
        .into_iter()
        .take(limit)
        .map(|(_, s)| SkillSummary::from_skill(s))
        .collect();

    let count = trimmed.len();
    Ok(ToolResult {
        label: format!("search_skills(\"{}\") → {} result(s)", query, count),
        data: json!({
            "count": count,
            "skills": trimmed,
        }),
    })
}

fn get_skill_detail(args: &Value, skills: &[Skill]) -> Result<ToolResult, String> {
    let skill_id = args
        .get("skill_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "get_skill_detail: 'skill_id' is required".to_string())?;
    let skill = skills
        .iter()
        .find(|s| s.id == skill_id || s.legacy_ids.iter().any(|l| l == skill_id))
        .ok_or_else(|| format!("skill not found: {}", skill_id))?;

    let body = std::fs::read_to_string(&skill.file_path)
        .unwrap_or_else(|e| format!("<failed to read skill body: {}>", e));
    // Cap body size to keep tool responses cheap on the model.
    let body_capped = if body.len() > 16_000 {
        format!("{}\n\n…[truncated {} bytes]", &body[..16_000], body.len() - 16_000)
    } else {
        body
    };

    Ok(ToolResult {
        label: format!("get_skill_detail({})", skill.name),
        data: json!({
            "skill": SkillSummary::from_skill(skill),
            "file_path": skill.file_path,
            "metadata": skill.metadata,
            "body": body_capped,
        }),
    })
}

fn combine_skills_workflow(args: &Value, skills: &[Skill]) -> Result<ToolResult, String> {
    let ids: Vec<String> = args
        .get("skill_ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let intent = args
        .get("intent")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if ids.is_empty() {
        return Err("combine_skills_workflow: 'skill_ids' must not be empty".into());
    }

    let mut steps = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let skill = skills
            .iter()
            .find(|s| &s.id == id || s.legacy_ids.iter().any(|l| l == id));
        let entry = match skill {
            Some(s) => json!({
                "step": i + 1,
                "skill_id": s.id,
                "skill_name": s.name,
                "purpose": s.description,
                "invocation_hint": s.metadata.slash_command.clone()
                    .unwrap_or_else(|| format!("invoke skill: {}", s.name)),
            }),
            None => json!({
                "step": i + 1,
                "skill_id": id,
                "skill_name": "(unknown)",
                "purpose": "skill not found in current registry",
                "invocation_hint": null,
            }),
        };
        steps.push(entry);
    }

    Ok(ToolResult {
        label: format!("combine_skills_workflow ({} step(s))", ids.len()),
        data: json!({
            "intent": intent,
            "steps": steps,
        }),
    })
}

fn render_prompt_for_coding_agent(args: &Value, skills: &[Skill]) -> Result<ToolResult, String> {
    let target_agent = args
        .get("target_agent")
        .and_then(|v| v.as_str())
        .unwrap_or("claude-code")
        .to_string();
    let intent = args
        .get("intent")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let ids: Vec<String> = args
        .get("skill_ids")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut chosen = Vec::new();
    for id in &ids {
        if let Some(s) = skills
            .iter()
            .find(|sk| &sk.id == id || sk.legacy_ids.iter().any(|l| l == id))
        {
            chosen.push(s);
        }
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

    let prompt = format!(
        "Target agent: {agent}\n\nGoal:\n{intent}\n\nUse these skills{lead}:\n{skills}\n\nInstructions for the agent:\n1. Load each listed skill before starting.\n2. Apply them in the order given.\n3. Report the result back in the format the skills define.\n\nQuick invocation: {invocation}",
        agent = target_agent,
        intent = intent.trim(),
        lead = if chosen.len() > 1 { " (in order)" } else { "" },
        skills = if skill_block.is_empty() {
            "(no matching skills found in your registry — proceed with general knowledge)".to_string()
        } else {
            skill_block
        },
        invocation = if invocation_line.is_empty() {
            "n/a".to_string()
        } else {
            invocation_line
        }
    );

    Ok(ToolResult {
        label: format!(
            "render_prompt_for_coding_agent → {} ({} skills)",
            target_agent,
            chosen.len()
        ),
        data: json!({
            "target_agent": target_agent,
            "prompt": prompt,
            "skill_count": chosen.len(),
        }),
    })
}
