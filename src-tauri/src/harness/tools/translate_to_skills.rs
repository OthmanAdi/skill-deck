// @agent-context: translate_to_skills — the killer flow's worker.
//
// Takes the user's natural-language intent and returns N candidate skill workflows
// the user can actually run on their machine. Phase 4 ships a flat tool version
// (search-rank-score the local registry). Phase 7 promotes this into a rig
// sub-agent attached to the orchestrator via `.tool(translator_agent)`, where
// it gets its own ReAct loop and a marketplace fallback when local thin.

use crate::harness::tools::{
    agent_id_str, merged_tags, parse_args_or_teach, search_skills::SkillSummary, HarnessContext,
    ToolError, ToolResult,
};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;

const NAME: &str = "translate_to_skills";
const DESCRIPTION: &str = "Take the user's natural-language intent and propose exactly `limit` candidate skill workflows from their installed skills. Each candidate is a ranked, scored match with a justification and a paste-ready invocation hint. Use this whenever the user asks 'what skill should I use for X', 'translate my idea into skills', 'give me N candidates for Y'. If the user specifies a number, you MUST return that number — never fewer.";
const EXAMPLE: &str = r#"{"intent":"refactor my rust api with code review","limit":3}"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InnerArgs {
    intent: String,
    limit: usize,
    #[serde(default)]
    prefer_tags: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "crate::harness::tools::serde_helpers::null_str_to_none"
    )]
    target_agent: Option<String>,
}

#[derive(Clone)]
pub struct TranslateToSkills {
    ctx: HarnessContext,
}

impl TranslateToSkills {
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

impl Tool for TranslateToSkills {
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
            "required": ["intent", "limit"],
            "properties": {
                "intent": { "type": "string" },
                "limit": { "type": "integer", "description": "Exact number of candidates to return. Honor the user's stated count." },
                "prefer_tags": { "type": "array", "items": { "type": "string" } },
                "target_agent": { "type": "string" }
            }
        }),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    skill: SkillSummary,
    score: f64,
    why: String,
    invocation: String,
    source: String, // "local" | "marketplace" (marketplace later in Phase 7 sub-agent)
}

async fn run(ctx: &HarnessContext, args: InnerArgs) -> Result<ToolResult, ToolError> {
    let start = Instant::now();
    if args.limit == 0 {
        return Err(ToolError::InvalidArgs("limit must be >= 1".into()));
    }
    let intent_l = args.intent.to_lowercase();
    let intent_terms: Vec<String> = intent_l
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .map(|t| t.to_string())
        .collect();
    let prefer_tags_l: Vec<String> = args
        .prefer_tags
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.to_lowercase())
        .collect();
    let target_agent_l = args.target_agent.as_ref().map(|s| s.to_lowercase());

    // Word-boundary regexes for the high-signal fields (name, tags), mirroring
    // the search_skills fix (TQ-1). Naive `.contains()` let a single common word
    // ("start") match unrelated skills ("SessionStart") and surface them with a
    // confident score. Description + use_cases keep substring matching for recall
    // on longer prose, where the lower weight bounds the noise.
    let term_patterns: Vec<(String, regex::Regex)> = intent_terms
        .iter()
        .filter_map(|t| {
            regex::RegexBuilder::new(&format!(r"\b{}\b", regex::escape(t)))
                .case_insensitive(true)
                .build()
                .ok()
                .map(|re| (t.clone(), re))
        })
        .collect();

    let mut scored: Vec<(f64, &crate::models::Skill, Vec<String>)> = ctx
        .skills
        .iter()
        .filter_map(|s| {
            let mut score = 0.0_f64;
            let name = s.name.to_lowercase();
            let desc = s.description.to_lowercase();
            let tags: Vec<String> = merged_tags(s)
                .into_iter()
                .map(|t| t.to_lowercase())
                .collect();
            let mut reasons: Vec<String> = Vec::new();
            let mut hit_terms: std::collections::HashSet<&str> = std::collections::HashSet::new();

            for (term, re) in &term_patterns {
                let mut hit = false;
                if re.is_match(&name) {
                    score += 5.0;
                    reasons.push(format!("name match '{term}'"));
                    hit = true;
                }
                if desc.contains(term) {
                    score += 2.5;
                    reasons.push(format!("description match '{term}'"));
                    hit = true;
                }
                if tags.iter().any(|t| re.is_match(t)) {
                    score += 3.0;
                    reasons.push(format!("tag match '{term}'"));
                    hit = true;
                }
                if s.use_cases.iter().any(|u| u.to_lowercase().contains(term)) {
                    score += 3.0;
                    reasons.push(format!("use-case match '{term}'"));
                    hit = true;
                }
                if hit {
                    hit_terms.insert(term.as_str());
                }
            }
            for pt in &prefer_tags_l {
                if tags.iter().any(|t| t == pt) {
                    score += 2.0;
                    reasons.push(format!("preferred tag '{pt}'"));
                }
            }
            if let Some(target) = &target_agent_l {
                if agent_id_str(s).to_lowercase() == *target {
                    score += 1.5;
                    reasons.push(format!("target agent '{target}'"));
                }
            }
            if s.starred {
                score += 0.5;
                reasons.push("starred".into());
            }

            // Coverage gate (TQ-1): require a minimum number of DISTINCT intent
            // terms to actually hit the skill. Single-term intents pass on one
            // hit; multi-term intents need at least 2 (ceil(n/2) floor), so
            // "start a business" no longer surfaces a skill that only shares
            // "start". When the intent has no usable terms (n == 0) the gate is
            // inert, so prefer_tags / target_agent only queries still work.
            let n = term_patterns.len();
            let required = if n <= 1 { 1 } else { n.div_ceil(2).max(2) };
            if score <= 0.0 || (n > 0 && hit_terms.len() < required) {
                None
            } else {
                Some((score, s, reasons))
            }
        })
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let had_any_direct_match = !scored.is_empty();
    let mut candidates: Vec<Candidate> = scored
        .iter()
        .take(args.limit)
        .map(|(score, s, reasons)| build_candidate(*score, s, reasons.clone()))
        .collect();

    // Only pad with recency/starred fallbacks when we already have AT LEAST
    // ONE real direct match. Padding from zero is hallucination: an off-topic
    // intent ("buy a chair on Amazon") would otherwise get matched to random
    // starred skills with score 0 and the user would see a confident-looking
    // list of irrelevant garbage.
    if had_any_direct_match && candidates.len() < args.limit {
        let already: std::collections::HashSet<String> =
            candidates.iter().map(|c| c.skill.id.clone()).collect();
        let fallback: Vec<&crate::models::Skill> = ctx
            .skills
            .iter()
            .filter(|s| !already.contains(&s.id))
            .collect();
        let mut fb_ranked: Vec<&crate::models::Skill> = fallback;
        fb_ranked.sort_by(|a, b| {
            b.starred
                .cmp(&a.starred)
                .then(b.installed_at.cmp(&a.installed_at))
        });
        let needed = args.limit - candidates.len();
        for s in fb_ranked.into_iter().take(needed) {
            candidates.push(build_candidate(
                0.0,
                s,
                vec!["fallback (soft match — surfaced by recency/starred)".into()],
            ));
        }
    }

    let count = candidates.len();
    let snippet = if let Some(first) = candidates.first() {
        format!(
            "{} candidate(s); top: {} (score {:.1})",
            count, first.skill.name, first.score
        )
    } else {
        format!(
            "no relevant local skills for '{}'. The intent may be off-topic for skill-deck \
             (e.g. shopping, weather, news), or no installed skill matches. \
             Try search_marketplace, or answer the user directly without skills.",
            args.intent.chars().take(80).collect::<String>()
        )
    };

    Ok(ToolResult {
        title: format!("{NAME}(\"{}\") → {} candidate(s)", args.intent, count),
        kind: "translate-skills".into(),
        snippet: ToolResult::snippet_from(&snippet),
        data: json!({
            "intent": args.intent,
            "limit": args.limit,
            "returned": count,
            "candidates": candidates,
        }),
        items_count: Some(count),
        source: Some("local".into()),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

fn build_candidate(score: f64, s: &crate::models::Skill, reasons: Vec<String>) -> Candidate {
    let invocation = s
        .metadata
        .slash_command
        .clone()
        .map(|c| c.to_string())
        .unwrap_or_else(|| format!("Invoke skill: {}", s.name));
    let why = if reasons.is_empty() {
        "match".to_string()
    } else {
        reasons.join("; ")
    };
    Candidate {
        skill: SkillSummary::from_skill(s, Some(score as i64)),
        score,
        why,
        invocation,
        source: "local".into(),
    }
}
