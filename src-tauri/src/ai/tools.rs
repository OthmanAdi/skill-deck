// @agent-context: Skill-aware tools exposed to the LLM via tool calls.
//
// Design intent: many small, composable tools that the model can chain.
// The system prompt forces the model into multiple rounds (search →
// inspect → combine → render) rather than answering from memory. Every
// metadata facet the user might ask about is searchable here: name,
// description, tags, use-cases, category, language, slash command, hook
// event, install/update date, agent, scope, starred flag.

use crate::models::Skill;
use chrono::{Datelike, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::provider::ToolDefinition;

pub fn all_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "search_skills".into(),
            description: "Wide-net search across every text field of the user's installed skills (name, description, tags, use_cases, category, language, slash_command). Use this FIRST for any natural-language question about which skills the user has. Returns ranked matches. Call again with a different `query` (synonyms, broader terms) if the first call returns fewer than 3 results — never answer 'I don't know' until you've broadened the search at least once.".into(),
            parameters: json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string", "description": "Keywords or natural-language phrase. Multiple terms ranked higher when present in name." },
                    "fields": {
                        "type": "array",
                        "items": { "type": "string", "enum": ["name", "description", "tags", "use_cases", "category", "language", "slash_command", "all"] },
                        "description": "Which fields to search. Default ['all']."
                    },
                    "limit": { "type": "integer", "description": "Max results, default 12, hard cap 50." }
                }
            }),
        },
        ToolDefinition {
            name: "list_skills".into(),
            description: "Structured listing with rich filters. Combine filters freely. Use this for questions like 'what skills did I install today', 'which Cursor skills do I have', 'show me starred skills with the rust tag'. Time filters take unix seconds; ALWAYS compute them from the current_time value the agent provides — never invent timestamps.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Filter to one agent id, e.g. claude-code, codex, cursor, gemini-cli." },
                    "scope": { "type": "string", "enum": ["global", "project"], "description": "Filter by scope." },
                    "starred_only": { "type": "boolean" },
                    "tag": { "type": "string", "description": "Skills whose discovery_tags or frontmatter tags include this exact tag (case-insensitive)." },
                    "language": { "type": "string", "description": "Filter by frontmatter language (rust, python, etc.)." },
                    "installed_since_unix": { "type": "integer", "description": "Only skills with installed_at >= this unix-seconds value." },
                    "updated_since_unix": { "type": "integer", "description": "Only skills with last_modified_at >= this unix-seconds value." },
                    "limit": { "type": "integer", "description": "Max results, default 40, hard cap 120." }
                }
            }),
        },
        ToolDefinition {
            name: "get_skill_detail".into(),
            description: "Fetch the full markdown body + parsed frontmatter for one skill. Call this after search_skills / list_skills when you need actual content to reason about how a skill works or to compose a prompt that uses it.".into(),
            parameters: json!({
                "type": "object",
                "required": ["skill_id"],
                "properties": {
                    "skill_id": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "get_skill_stats".into(),
            description: "Cheap analytical roll-ups over the skill registry. Use for 'how many skills do I have', 'which agent has the most skills', 'what tags appear most', 'how many skills did I install this month'. Returns counts grouped by the requested axis — does NOT return skill bodies.".into(),
            parameters: json!({
                "type": "object",
                "required": ["group_by"],
                "properties": {
                    "group_by": {
                        "type": "string",
                        "enum": ["agent_id", "tag", "scope", "language", "install_month", "update_month", "starred"],
                        "description": "What to group on."
                    },
                    "top_n": { "type": "integer", "description": "Return top N groups by count, default 20." }
                }
            }),
        },
        ToolDefinition {
            name: "combine_skills_workflow".into(),
            description: "Compose an ordered workflow from multiple skill_ids for a stated intent. Returns structured steps the agent can summarize or feed into render_prompt_for_coding_agent. Use this whenever the user asks to combine, chain, or compose two or more skills.".into(),
            parameters: json!({
                "type": "object",
                "required": ["skill_ids", "intent"],
                "properties": {
                    "skill_ids": { "type": "array", "items": { "type": "string" } },
                    "intent": { "type": "string", "description": "What the user is trying to accomplish in their own words." }
                }
            }),
        },
        ToolDefinition {
            name: "render_prompt_for_coding_agent".into(),
            description: "Translate the user's intent + selected skills into a final-form prompt the user can paste into their coding agent (Claude Code, Codex, Cursor, etc.). Returns the prompt text. Use this whenever the user asks 'make me a prompt', 'turn this into a prompt', 'build a prompt for X'.".into(),
            parameters: json!({
                "type": "object",
                "required": ["target_agent", "intent", "skill_ids"],
                "properties": {
                    "target_agent": { "type": "string", "description": "Target coding agent id." },
                    "intent": { "type": "string", "description": "Detailed description of what the user wants the coding agent to do." },
                    "skill_ids": { "type": "array", "items": { "type": "string" }, "description": "Ordered skill ids to reference." },
                    "extra_instructions": { "type": "string", "description": "Optional extra context for the target agent (constraints, file paths, success criteria)." }
                }
            }),
        },
        ToolDefinition {
            name: "search_marketplace".into(),
            description: "Search the public skill marketplaces (Skills.sh and ClawHub) — the SAME registries the user's Registry tab uses — for skills the user does NOT yet have installed. Use ONLY when the user explicitly asks to look on skills.sh / clawhub / marketplace / registry / hub, OR when search_skills + list_skills together returned ZERO matches for the user's intent and no local skill can fulfill the task. Each returned item carries an install_command the user can run to add the skill locally. Never use this for questions about what is already installed — those go through search_skills / list_skills.".into(),
            parameters: json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string", "description": "Keywords or natural-language phrase to look up." },
                    "provider": {
                        "type": "string",
                        "enum": ["all", "skills-sh", "claw-hub"],
                        "description": "Which marketplace to query. Default 'all' fans out in parallel."
                    },
                    "limit": { "type": "integer", "description": "Max results per provider, default 10, hard cap 25." }
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
    pub use_cases: Vec<String>,
    pub category: Option<String>,
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified_at: Option<u64>,
    pub starred: bool,
    pub slash_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
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
        let tags = merge_tags(s);
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            description: s.description.clone(),
            agent_id,
            scope,
            tags,
            use_cases: s.use_cases.clone(),
            category: s.metadata.category.clone(),
            language: s.metadata.language.clone(),
            installed_at: s.installed_at,
            last_modified_at: s.last_modified_at,
            starred: s.starred,
            slash_command: s.metadata.slash_command.clone(),
            version: s.metadata.version.clone(),
            author: s.metadata.author.clone(),
        }
    }
}

fn merge_tags(s: &Skill) -> Vec<String> {
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub label: String,
    pub data: Value,
}

pub async fn dispatch_tool(
    name: &str,
    arguments_json: &str,
    skills: &[Skill],
) -> Result<ToolResult, String> {
    let args: Value =
        serde_json::from_str(arguments_json).unwrap_or(Value::Object(Default::default()));
    log::info!(
        target: "skill_deck::agent::tools",
        "dispatch_tool name={} args={}",
        name,
        truncate_for_log(&arguments_json, 240)
    );
    let start = std::time::Instant::now();
    let result = match name {
        "search_skills" => search_skills(&args, skills),
        "list_skills" => list_skills(&args, skills),
        "get_skill_detail" => get_skill_detail(&args, skills),
        "get_skill_stats" => get_skill_stats(&args, skills),
        "combine_skills_workflow" => combine_skills_workflow(&args, skills),
        "render_prompt_for_coding_agent" => render_prompt_for_coding_agent(&args, skills),
        "search_marketplace" => search_marketplace(&args).await,
        other => Err(format!("unknown tool: {}", other)),
    };
    let ms = start.elapsed().as_millis();
    match &result {
        Ok(r) => log::info!(
            target: "skill_deck::agent::tools",
            "dispatch_tool name={} ok label=\"{}\" elapsed_ms={}",
            name,
            r.label,
            ms
        ),
        Err(e) => log::warn!(
            target: "skill_deck::agent::tools",
            "dispatch_tool name={} ERROR msg=\"{}\" elapsed_ms={}",
            name,
            e,
            ms
        ),
    }
    result
}

fn truncate_for_log(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push('…');
        out
    }
}

// -- search_skills ---------------------------------------------------------

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
        .min(50) as usize;
    let fields = args
        .get("fields")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["all".to_string()]);
    let search_all = fields.iter().any(|f| f == "all");
    let want = |f: &str| search_all || fields.iter().any(|x| x == f);

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
            let tags = merge_tags(s);
            let mut score: i64 = 0;
            for term in &terms {
                if want("name") && hay_name.contains(term) {
                    score += 14;
                }
                if want("description") && hay_desc.contains(term) {
                    score += 6;
                }
                if want("tags") && tags.iter().any(|t| t.to_lowercase().contains(term)) {
                    score += 8;
                }
                if want("use_cases")
                    && s.use_cases.iter().any(|u| u.to_lowercase().contains(term))
                {
                    score += 7;
                }
                if want("category")
                    && s.metadata
                        .category
                        .as_ref()
                        .map(|c| c.to_lowercase().contains(term))
                        .unwrap_or(false)
                {
                    score += 5;
                }
                if want("language")
                    && s.metadata
                        .language
                        .as_ref()
                        .map(|l| l.to_lowercase().contains(term))
                        .unwrap_or(false)
                {
                    score += 5;
                }
                if want("slash_command")
                    && s.metadata
                        .slash_command
                        .as_ref()
                        .map(|c| c.to_lowercase().contains(term))
                        .unwrap_or(false)
                {
                    score += 4;
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
            "fields_searched": fields,
            "skills": trimmed,
        }),
    })
}

// -- list_skills -----------------------------------------------------------

fn list_skills(args: &Value, skills: &[Skill]) -> Result<ToolResult, String> {
    let agent_id = args.get("agent_id").and_then(|v| v.as_str());
    let scope = args.get("scope").and_then(|v| v.as_str());
    let starred_only = args
        .get("starred_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let tag = args.get("tag").and_then(|v| v.as_str()).map(|s| s.to_lowercase());
    let language = args
        .get("language")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());
    let installed_since = args.get("installed_since_unix").and_then(|v| v.as_u64());
    let updated_since = args.get("updated_since_unix").and_then(|v| v.as_u64());
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(40)
        .min(120) as usize;

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
                let matches = matches!(
                    (scope_filter, &s.scope),
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
            if let Some(t) = tag.as_ref() {
                let tags = merge_tags(s);
                if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                    return false;
                }
            }
            if let Some(lang) = language.as_ref() {
                let matches = s
                    .metadata
                    .language
                    .as_ref()
                    .map(|l| l.eq_ignore_ascii_case(lang))
                    .unwrap_or(false);
                if !matches {
                    return false;
                }
            }
            if let Some(since) = installed_since {
                if s.installed_at.unwrap_or(0) < since {
                    return false;
                }
            }
            if let Some(since) = updated_since {
                if s.last_modified_at.unwrap_or(0) < since {
                    return false;
                }
            }
            true
        })
        .take(limit)
        .map(SkillSummary::from_skill)
        .collect();

    let count = filtered.len();
    let mut filter_desc = Vec::new();
    if let Some(a) = agent_id {
        filter_desc.push(format!("agent={}", a));
    }
    if let Some(sc) = scope {
        filter_desc.push(format!("scope={}", sc));
    }
    if starred_only {
        filter_desc.push("starred".into());
    }
    if let Some(t) = tag.as_ref() {
        filter_desc.push(format!("tag={}", t));
    }
    if let Some(l) = language.as_ref() {
        filter_desc.push(format!("lang={}", l));
    }
    if installed_since.is_some() {
        filter_desc.push("installed-since".into());
    }
    if updated_since.is_some() {
        filter_desc.push("updated-since".into());
    }
    let label_filters = if filter_desc.is_empty() {
        "all".to_string()
    } else {
        filter_desc.join(",")
    };

    Ok(ToolResult {
        label: format!("list_skills({}) → {} result(s)", label_filters, count),
        data: json!({
            "count": count,
            "filters_applied": filter_desc,
            "skills": filtered,
        }),
    })
}

// -- get_skill_detail ------------------------------------------------------

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
    let body_capped = if body.len() > 16_000 {
        format!(
            "{}\n\n…[truncated {} bytes]",
            &body[..16_000],
            body.len() - 16_000
        )
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

// -- get_skill_stats -------------------------------------------------------

fn get_skill_stats(args: &Value, skills: &[Skill]) -> Result<ToolResult, String> {
    let group_by = args
        .get("group_by")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "get_skill_stats: 'group_by' is required".to_string())?;
    let top_n = args
        .get("top_n")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    let mut counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    for s in skills {
        match group_by {
            "agent_id" => {
                let aid = serde_json::to_string(&s.agent_id)
                    .unwrap_or_else(|_| "\"unknown\"".to_string())
                    .trim_matches('"')
                    .to_string();
                *counts.entry(aid).or_insert(0) += 1;
            }
            "tag" => {
                for t in merge_tags(s) {
                    *counts.entry(t.to_lowercase()).or_insert(0) += 1;
                }
            }
            "scope" => {
                let scope = match s.scope {
                    crate::models::SkillScope::Global => "global".to_string(),
                    crate::models::SkillScope::Project => "project".to_string(),
                };
                *counts.entry(scope).or_insert(0) += 1;
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
                let key = if s.starred { "starred" } else { "unstarred" };
                *counts.entry(key.to_string()).or_insert(0) += 1;
            }
            other => {
                return Err(format!("unknown group_by: {}", other));
            }
        }
    }

    let mut ranked: Vec<(String, u32)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.truncate(top_n);
    let total: u32 = ranked.iter().map(|(_, c)| *c).sum();

    Ok(ToolResult {
        label: format!(
            "get_skill_stats(group_by={}) → {} group(s)",
            group_by,
            ranked.len()
        ),
        data: json!({
            "group_by": group_by,
            "total_count": total,
            "groups": ranked.iter().map(|(k, v)| json!({ "key": k, "count": v })).collect::<Vec<_>>(),
        }),
    })
}

fn month_key(unix_seconds: u64) -> Option<String> {
    let dt = chrono::Utc.timestamp_opt(unix_seconds as i64, 0).single()?;
    Some(format!("{:04}-{:02}", dt.year(), dt.month()))
}

// -- combine_skills_workflow ----------------------------------------------

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
                "tags": merge_tags(s),
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

// -- render_prompt_for_coding_agent ---------------------------------------

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
    let extra = args
        .get("extra_instructions")
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

    let extra_block = if extra.trim().is_empty() {
        String::new()
    } else {
        format!("\nExtra constraints:\n{}\n", extra.trim())
    };

    let prompt = format!(
        "Target agent: {agent}\n\nGoal:\n{intent}\n\nUse these skills{lead}:\n{skills}\n{extra}\nInstructions for the agent:\n1. Load each listed skill before starting.\n2. Apply them in the order given.\n3. Report results in the format the skills define.\n\nQuick invocation: {invocation}",
        agent = target_agent,
        intent = intent.trim(),
        lead = if chosen.len() > 1 { " (in order)" } else { "" },
        skills = if skill_block.is_empty() {
            "(no matching skills found in your registry — proceed with general knowledge)".to_string()
        } else {
            skill_block
        },
        extra = extra_block,
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

// -- search_marketplace ---------------------------------------------------

async fn search_marketplace(args: &Value) -> Result<ToolResult, String> {
    use crate::detection::marketplaces::{self, types::ProviderId, normalize_limit};

    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "search_marketplace: 'query' is required".to_string())?
        .to_string();
    let provider_filter = args
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("all")
        .to_lowercase();
    let limit = normalize_limit(
        args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10).min(25) as usize,
    );

    // Local lightweight outcome to avoid touching the private `from_result`
    // constructor on marketplaces::ProviderSearchOutcome.
    struct LocalOutcome {
        provider: ProviderId,
        response: Option<marketplaces::MarketplaceSearchResponse>,
        error: Option<String>,
    }

    let outcomes: Vec<LocalOutcome> = match provider_filter.as_str() {
        "skills-sh" | "skillssh" => {
            let result = marketplaces::search(ProviderId::SkillsSh, &query, limit).await;
            vec![match result {
                Ok(response) => LocalOutcome {
                    provider: ProviderId::SkillsSh,
                    response: Some(response),
                    error: None,
                },
                Err(e) => LocalOutcome {
                    provider: ProviderId::SkillsSh,
                    response: None,
                    error: Some(e.to_string()),
                },
            }]
        }
        "claw-hub" | "clawhub" => {
            let result = marketplaces::search(ProviderId::ClawHub, &query, limit).await;
            vec![match result {
                Ok(response) => LocalOutcome {
                    provider: ProviderId::ClawHub,
                    response: Some(response),
                    error: None,
                },
                Err(e) => LocalOutcome {
                    provider: ProviderId::ClawHub,
                    response: None,
                    error: Some(e.to_string()),
                },
            }]
        }
        _ => marketplaces::search_all(&query, limit)
            .await
            .into_iter()
            .map(|o| LocalOutcome {
                provider: o.provider,
                response: o.response,
                error: o.error,
            })
            .collect(),
    };

    let mut items_out: Vec<Value> = Vec::new();
    let mut per_provider: Vec<Value> = Vec::new();
    let mut total_items = 0usize;
    let mut any_error: Option<String> = None;

    for outcome in &outcomes {
        let provider_str = serde_json::to_string(&outcome.provider)
            .unwrap_or_else(|_| "\"unknown\"".into())
            .trim_matches('"')
            .to_string();
        match (&outcome.response, &outcome.error) {
            (Some(resp), _) => {
                per_provider.push(json!({
                    "provider": provider_str,
                    "count": resp.items.len(),
                    "duration_ms": resp.duration_ms,
                }));
                for item in &resp.items {
                    total_items += 1;
                    items_out.push(json!({
                        "id": item.id,
                        "provider": serde_json::to_string(&item.provider)
                            .unwrap_or_default()
                            .trim_matches('"')
                            .to_string(),
                        "kind": serde_json::to_string(&item.kind)
                            .unwrap_or_default()
                            .trim_matches('"')
                            .to_string(),
                        "name": item.name,
                        "description": item.description,
                        "install_command": item.install_command,
                        "source_url": item.source_url,
                        "homepage_url": item.homepage_url,
                        "installs": item.installs,
                        "version": item.version,
                        "author": item.author,
                        "updated_at": item.updated_at,
                    }));
                }
            }
            (None, Some(err)) => {
                per_provider.push(json!({
                    "provider": provider_str,
                    "error": err,
                }));
                if any_error.is_none() {
                    any_error = Some(format!("{}: {}", provider_str, err));
                }
            }
            (None, None) => {
                per_provider.push(json!({
                    "provider": provider_str,
                    "count": 0,
                }));
            }
        }
    }

    let label = format!(
        "search_marketplace(\"{}\") → {} item(s) across {} provider(s)",
        query,
        total_items,
        per_provider.len()
    );

    Ok(ToolResult {
        label,
        data: json!({
            "query": query,
            "provider_filter": provider_filter,
            "total": total_items,
            "providers": per_provider,
            "items": items_out,
            "warning": any_error,
        }),
    })
}
