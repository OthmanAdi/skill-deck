// @agent-context: search_skills — fuzzy keyword search over every text field
// of the user's installed skills. The model's first move on any "what skills do
// I have for X" question.
//
// Scoring: term hits in `name` rank highest, then tags / use_cases, then
// description, then category / language / slash_command. Multiple terms
// each contribute independently so two-word queries lift truly relevant skills.

use crate::harness::tools::{
    agent_id_str, merged_tags, parse_args_or_teach, HarnessContext, ToolError, ToolResult,
};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;

const NAME: &str = "search_skills";
const DESCRIPTION: &str = "Wide-net search across every text field of the user's installed skills (name, description, tags, use_cases, category, language, slash_command). Use this FIRST for any natural-language question about which skills the user has. Returns ranked matches. Call again with a different `query` (synonyms, broader terms) if the first call returns fewer than 3 results — never answer 'I don't know' until you've broadened the search at least once.";
const EXAMPLE: &str = r#"{"query":"rust testing","limit":5}"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InnerArgs {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    fields: Option<Vec<String>>,
}

#[derive(Clone)]
pub struct SearchSkills {
    ctx: HarnessContext,
}

impl SearchSkills {
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

impl Tool for SearchSkills {
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
            "required": ["query"],
            "properties": {
                "query": { "type": "string" },
                "limit": { "type": "integer", "description": "Default 12, hard cap 50." },
                "fields": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["name", "description", "tags", "use_cases", "category", "language", "slash_command", "all"] }
                }
            }
        }),
    }
}

async fn run(ctx: &HarnessContext, args: InnerArgs) -> Result<ToolResult, ToolError> {
    let start = Instant::now();
    let query = args.query.to_lowercase();
    let limit = args
        .limit
        .unwrap_or(ctx.settings.item_count_default)
        .clamp(1, 50);
    let fields = args.fields.unwrap_or_else(|| vec!["all".to_string()]);
    let search_all = fields.iter().any(|f| f == "all");
    let want = |f: &str| search_all || fields.iter().any(|x| x == f);

    // Drop short noise terms ("a", "i", "is"). Anything < 3 chars matches
    // too aggressively under substring search and caused the
    // "start a business" → SessionStart-hook false-positive cascade.
    let terms: Vec<String> = query
        .split_whitespace()
        .filter(|t| !t.is_empty() && t.chars().count() >= 3)
        .map(|t| t.to_string())
        .collect();

    if terms.is_empty() {
        return Ok(ToolResult {
            title: format!("{NAME}(\"{}\") → 0 results", args.query),
            kind: "skill-search".into(),
            snippet: "empty query (no terms of length >= 3 after filtering)".into(),
            data: json!({ "count": 0, "skills": [] }),
            items_count: Some(0),
            source: Some("local".into()),
            elapsed_ms: start.elapsed().as_millis() as u64,
        });
    }

    // Precompile word-boundary regexes once per query, not per skill. We use
    // \b{term}\b on high-signal fields (name, tags, slash_command, category,
    // language). Substring stays on description / use_cases for recall on
    // longer prose — those weights are lower so the noise is bounded.
    let term_patterns: Vec<(String, regex::Regex)> = terms
        .iter()
        .filter_map(|t| {
            let pat = format!(r"\b{}\b", regex::escape(t));
            regex::RegexBuilder::new(&pat)
                .case_insensitive(true)
                .build()
                .ok()
                .map(|re| (t.clone(), re))
        })
        .collect();

    let mut scored: Vec<(i64, &crate::models::Skill)> = ctx
        .skills
        .iter()
        .filter_map(|s| {
            let hay_name = s.name.to_lowercase();
            let hay_desc = s.description.to_lowercase();
            let tags = merged_tags(s);
            let mut score: i64 = 0;
            // Track how many distinct query terms hit ANY field. We later
            // require >= ceil(terms/2) matches so "start a business" stops
            // matching things that only share the word "start".
            let mut hit_terms: std::collections::HashSet<String> = std::collections::HashSet::new();

            for (term, re) in &term_patterns {
                let mut hit = false;
                if want("name") && re.is_match(&hay_name) {
                    score += 14;
                    hit = true;
                }
                if want("tags") && tags.iter().any(|t| re.is_match(&t.to_lowercase())) {
                    score += 8;
                    hit = true;
                }
                if want("slash_command")
                    && s.metadata
                        .slash_command
                        .as_ref()
                        .map(|c| re.is_match(&c.to_lowercase()))
                        .unwrap_or(false)
                {
                    score += 4;
                    hit = true;
                }
                if want("category")
                    && s.metadata
                        .category
                        .as_ref()
                        .map(|c| re.is_match(&c.to_lowercase()))
                        .unwrap_or(false)
                {
                    score += 5;
                    hit = true;
                }
                if want("language")
                    && s.metadata
                        .language
                        .as_ref()
                        .map(|l| re.is_match(&l.to_lowercase()))
                        .unwrap_or(false)
                {
                    score += 5;
                    hit = true;
                }
                // Description + use_cases stay on substring — longer prose,
                // we want recall. Lower weights bound the false-positive cost.
                if want("description") && hay_desc.contains(term) {
                    score += 6;
                    hit = true;
                }
                if want("use_cases") && s.use_cases.iter().any(|u| u.to_lowercase().contains(term))
                {
                    score += 7;
                    hit = true;
                }
                if hit {
                    hit_terms.insert(term.clone());
                }
            }
            // Coverage gate: enforce a minimum number of DISTINCT query
            // terms that touched the skill. Single-term queries pass with
            // one hit; multi-term queries require at least 2 hits (with a
            // ceil(n/2) floor for longer queries). The load-bearing case:
            // "start a business" → terms ["start", "business"] after the
            // len>=3 filter. SessionStart hooks match only "start", so they
            // get 1 hit < required 2 and drop out.
            let n = term_patterns.len();
            let required = if n == 1 { 1 } else { n.div_ceil(2).max(2) };
            if score == 0 || hit_terms.len() < required {
                None
            } else {
                Some((score, s))
            }
        })
        .collect();
    scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    let trimmed: Vec<SkillSummary> = scored
        .into_iter()
        .take(limit)
        .map(|(score, s)| SkillSummary::from_skill(s, Some(score)))
        .collect();

    let count = trimmed.len();
    let snippet = if let Some(first) = trimmed.first() {
        if count == 1 {
            format!("top match: {} — {}", first.name, first.description)
        } else {
            format!(
                "{} matches; top: {} ({}…)",
                count,
                first.name,
                first.description.chars().take(60).collect::<String>()
            )
        }
    } else {
        "no matches; broaden the query".to_string()
    };

    Ok(ToolResult {
        title: format!("{NAME}(\"{}\") → {} result(s)", args.query, count),
        kind: "skill-search".into(),
        snippet: ToolResult::snippet_from(&snippet),
        data: json!({
            "count": count,
            "fields_searched": fields,
            "skills": trimmed,
        }),
        items_count: Some(count),
        source: Some("local".into()),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub agent_id: String,
    pub tags: Vec<String>,
    pub slash_command: Option<String>,
    pub installed_at: Option<u64>,
    pub last_modified_at: Option<u64>,
    pub starred: bool,
    pub score: Option<i64>,
}

impl SkillSummary {
    pub fn from_skill(s: &crate::models::Skill, score: Option<i64>) -> Self {
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            description: s.description.clone(),
            agent_id: agent_id_str(s),
            tags: merged_tags(s),
            slash_command: s.metadata.slash_command.clone(),
            installed_at: s.installed_at,
            last_modified_at: s.last_modified_at,
            starred: s.starred,
            score,
        }
    }
}
