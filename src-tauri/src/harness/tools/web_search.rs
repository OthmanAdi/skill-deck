// @agent-context: web_search — DuckDuckGo HTML scrape, no API key required.
//
// DuckDuckGo serves a public HTML SERP at https://html.duckduckgo.com/html/?q=...
// that returns server-rendered results. We extract the first N items by regex.
// Fragile by design — but the user explicitly chose "no API key for v1 happy path".
// Brave or Tavily can be wired later via Settings.web_search_provider.

use crate::harness::tools::{parse_args_or_teach, HarnessContext, ToolError, ToolResult};
use regex::Regex;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;

/// Maximum HTML response size (2 MB) to prevent ReDoS on malformed pages.
const MAX_HTML_BYTES: usize = 2_000_000;

const NAME: &str = "web_search";
const DESCRIPTION: &str = "Search the public web. Returns ranked results with title + url + snippet. Use this when the user asks for information that is NOT about their installed skills and NOT in the marketplace registries. Default search engine is DuckDuckGo (no API key required); user can configure Brave in Settings.";
const DDG_URL: &str = "https://html.duckduckgo.com/html/";
const EXAMPLE: &str = r#"{"query":"rig rust agent","limit":6}"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InnerArgs {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Clone)]
pub struct WebSearch {
    ctx: HarnessContext,
}

impl WebSearch {
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

impl Tool for WebSearch {
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
                "limit": { "type": "integer", "description": "Default 6, cap 20." }
            }
        }),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebResult {
    title: String,
    url: String,
    snippet: String,
}

async fn run(ctx: &HarnessContext, args: InnerArgs) -> Result<ToolResult, ToolError> {
    let start = Instant::now();
    let limit = args.limit.unwrap_or(6).clamp(1, 20);

    let res = ctx
        .http
        .post(DDG_URL)
        .form(&[("q", args.query.as_str())])
        .header(
            "User-Agent",
            "Mozilla/5.0 (compatible; SkillDeckHarness/0.1)",
        )
        .send()
        .await?;
    let status = res.status();
    if !status.is_success() {
        return Err(ToolError::Network(format!("duckduckgo HTTP {status}")));
    }
    let html_raw = res.bytes().await?;
    // Cap HTML size to prevent ReDoS from malformed giant pages.
    let html = if html_raw.len() > MAX_HTML_BYTES {
        String::from_utf8_lossy(&html_raw[..MAX_HTML_BYTES]).into_owned()
    } else {
        String::from_utf8_lossy(&html_raw).into_owned()
    };
    let results = parse_ddg_html(&html, limit);
    let count = results.len();

    let snippet = if let Some(first) = results.first() {
        format!("{count} result(s); top: {} ({})", first.title, first.url)
    } else {
        "no results from duckduckgo".into()
    };

    Ok(ToolResult {
        title: format!("{NAME}(\"{}\") → {} result(s)", args.query, count),
        kind: "web-search".into(),
        snippet: ToolResult::snippet_from(&snippet),
        data: json!({ "query": args.query, "engine": "duckduckgo", "results": results }),
        items_count: Some(count),
        source: Some("duckduckgo".into()),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

fn parse_ddg_html(html: &str, limit: usize) -> Vec<WebResult> {
    use std::sync::OnceLock;
    // Compiled once per process. expect() is justified: compile-time literal
    // patterns, exercised by tests — Regex::new on them is infallible.
    static TITLE_RE: OnceLock<Regex> = OnceLock::new();
    static SNIPPET_RE: OnceLock<Regex> = OnceLock::new();
    // DDG HTML SERP item shape (approx):
    //   <a class="result__a" href="..."> TITLE </a>
    //   <a class="result__snippet" href="..."> SNIPPET </a>
    // We bridge anchor pairs by scanning sequentially.
    let title_re = TITLE_RE.get_or_init(|| {
        Regex::new(
            r#"(?s)<a[^>]*class="[^"]*result__a[^"]*"[^>]*href="(?P<url>[^"]+)"[^>]*>(?P<title>.*?)</a>"#,
        )
        .expect("static regex")
    });
    let snippet_re = SNIPPET_RE.get_or_init(|| {
        Regex::new(r#"(?s)<a[^>]*class="[^"]*result__snippet[^"]*"[^>]*>(?P<text>.*?)</a>"#)
            .expect("static regex")
    });

    let titles: Vec<(String, String)> = title_re
        .captures_iter(html)
        .map(|c| {
            (
                decode_html(&c["url"]),
                strip_tags(&decode_html(&c["title"])),
            )
        })
        .collect();
    let snippets: Vec<String> = snippet_re
        .captures_iter(html)
        .map(|c| strip_tags(&decode_html(&c["text"])))
        .collect();

    let mut out = Vec::with_capacity(limit);
    for (i, (url, title)) in titles.into_iter().enumerate() {
        if out.len() >= limit {
            break;
        }
        let snippet = snippets.get(i).cloned().unwrap_or_default();
        out.push(WebResult {
            title,
            url,
            snippet,
        });
    }
    out
}

fn decode_html(s: &str) -> String {
    // Minimal entity decode for SERP — full decode is over-engineered here.
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

fn strip_tags(s: &str) -> String {
    use std::sync::OnceLock;
    // Compiled once; compile-time literal pattern → infallible.
    static TAG_RE: OnceLock<Regex> = OnceLock::new();
    let re = TAG_RE.get_or_init(|| Regex::new(r"<[^>]+>").expect("static regex"));
    re.replace_all(s, "").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ddg_serp_pairs_and_respects_limit() {
        let html = r##"
            <a class="result__a" href="https://one.example/a">First &amp; Best</a>
            <a class="result__snippet" href="#">Snippet <b>one</b></a>
            <a class="result__a" href="https://two.example/b">Second</a>
            <a class="result__snippet" href="#">Snippet two</a>
            <a class="result__a" href="https://three.example/c">Third</a>
        "##;
        let results = parse_ddg_html(html, 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://one.example/a");
        assert_eq!(results[0].title, "First & Best");
        assert_eq!(results[0].snippet, "Snippet one");
        assert_eq!(results[1].title, "Second");
    }

    #[test]
    fn parse_ddg_html_handles_garbage_without_panicking() {
        assert!(parse_ddg_html("<<<<not html at all", 5).is_empty());
        assert!(parse_ddg_html("", 5).is_empty());
    }
}
