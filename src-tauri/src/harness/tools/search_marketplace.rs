// @agent-context: search_marketplace — wraps existing detection::marketplaces with
// parallel / sequential mode selection. Public marketplaces only (skills.sh + clawhub).

use crate::detection::marketplaces::{self, normalize_limit, types::ProviderId};
use crate::harness::tools::{parse_args_or_teach, HarnessContext, ToolError, ToolResult};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

const NAME: &str = "search_marketplace";
const DESCRIPTION: &str = "Search the public skill marketplaces (Skills.sh and ClawHub) for skills the user does NOT yet have installed. Use ONLY when the user explicitly asks about marketplace/registry/hub OR when local search returned zero results. Modes: 'parallel' fans both out concurrently, 'sequential' awaits them in order. Each item carries an install_command.";
const EXAMPLE: &str =
    r#"{"query":"prompt engineering","provider":"all","limit":10,"mode":"parallel"}"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InnerArgs {
    query: String,
    #[serde(
        default,
        deserialize_with = "crate::harness::tools::serde_helpers::null_str_to_none"
    )]
    provider: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(
        default,
        deserialize_with = "crate::harness::tools::serde_helpers::null_str_to_none"
    )]
    mode: Option<String>,
}

#[derive(Clone)]
pub struct SearchMarketplace {
    ctx: HarnessContext,
}

impl SearchMarketplace {
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

impl Tool for SearchMarketplace {
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
                "provider": { "type": "string", "enum": ["all", "skills-sh", "claw-hub"] },
                "limit": { "type": "integer", "description": "Per provider, default 10, cap 25." },
                "mode": { "type": "string", "enum": ["parallel", "sequential"] }
            }
        }),
    }
}

async fn run(_ctx: &HarnessContext, args: InnerArgs) -> Result<ToolResult, ToolError> {
    let start = Instant::now();
    let provider_filter = args
        .provider
        .unwrap_or_else(|| "all".to_string())
        .to_lowercase();
    let limit = normalize_limit(args.limit.unwrap_or(10).min(25));
    let mode = args
        .mode
        .unwrap_or_else(|| "parallel".to_string())
        .to_lowercase();

    let mut items_out: Vec<Value> = Vec::new();
    let mut per_provider: Vec<Value> = Vec::new();
    let mut total_items = 0usize;

    let targets: Vec<ProviderId> = match provider_filter.as_str() {
        "skills-sh" | "skillssh" => vec![ProviderId::SkillsSh],
        "claw-hub" | "clawhub" => vec![ProviderId::ClawHub],
        _ => vec![ProviderId::SkillsSh, ProviderId::ClawHub],
    };

    let outcomes = if mode == "sequential" {
        let mut acc = Vec::new();
        for p in &targets {
            let r = marketplaces::search(*p, &args.query, limit).await;
            acc.push((*p, r));
        }
        acc
    } else {
        // Parallel fan-out with concurrent awaits, then collect.
        let futures: Vec<_> = targets
            .iter()
            .map(|p| {
                let pp = *p;
                let q = args.query.clone();
                async move { (pp, marketplaces::search(pp, &q, limit).await) }
            })
            .collect();
        futures_util::future::join_all(futures).await
    };

    for (provider_id, result) in &outcomes {
        let provider_str = serde_json::to_string(provider_id)
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        match result {
            Ok(resp) => {
                per_provider.push(json!({
                    "provider": provider_str,
                    "count": resp.items.len(),
                    "duration_ms": resp.duration_ms,
                }));
                for item in &resp.items {
                    total_items += 1;
                    items_out.push(json!({
                        "id": item.id,
                        "provider": serde_json::to_string(&item.provider).unwrap_or_default().trim_matches('"').to_string(),
                        "kind": serde_json::to_string(&item.kind).unwrap_or_default().trim_matches('"').to_string(),
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
            Err(e) => {
                per_provider.push(json!({ "provider": provider_str, "error": e.to_string() }));
            }
        }
    }

    let snippet = if total_items == 0 {
        format!("0 results across {} provider(s)", per_provider.len())
    } else {
        let first_name = items_out
            .first()
            .and_then(|v| v.get("name").and_then(|n| n.as_str()))
            .unwrap_or("?");
        format!(
            "{total_items} item(s) across {} provider(s); top: {first_name}",
            per_provider.len()
        )
    };

    Ok(ToolResult {
        title: format!(
            "{NAME}(\"{}\", mode={}) → {} item(s)",
            args.query, mode, total_items
        ),
        kind: "marketplace-search".into(),
        snippet: ToolResult::snippet_from(&snippet),
        data: json!({
            "query": args.query,
            "mode": mode,
            "provider_filter": provider_filter,
            "total": total_items,
            "providers": per_provider,
            "items": items_out,
        }),
        items_count: Some(total_items),
        source: Some("marketplace".into()),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}
