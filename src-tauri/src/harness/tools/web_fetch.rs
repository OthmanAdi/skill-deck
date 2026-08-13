// @agent-context: web_fetch — pull URL content, strip tags, cap by approx tokens.

use crate::harness::tokens::{Counter, TokenizerKind};
use crate::harness::tools::{parse_args_or_teach, HarnessContext, ToolError, ToolResult};
use regex::Regex;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::IpAddr;
use std::time::Instant;

/// Maximum response body size (2 MB).
const MAX_RESPONSE_BYTES: usize = 2_000_000;

/// Reject URLs that could reach internal services (SSRF protection).
/// Blocks: non-http(s) schemes, private/link-local IPs, localhost.
/// String-based parsing to avoid adding a url crate dependency.
fn url_is_safe(raw: &str) -> Result<(), ToolError> {
    // Must start with http:// or https://
    let lower = raw.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err(ToolError::InvalidArgs(
            "only http:// and https:// URLs are allowed".into(),
        ));
    }

    // Extract host portion (between "://" and first "/" or ":" or end).
    let after_scheme = &raw[raw.find("://").map(|i| i + 3).unwrap_or(0)..];
    let host_end = after_scheme
        .find(['/', ':', '?', '#'])
        .unwrap_or(after_scheme.len());
    let host = &after_scheme[..host_end];

    if host.is_empty() {
        return Err(ToolError::InvalidArgs("URL has no host".into()));
    }

    // Block localhost and similar.
    let host_lower = host.to_ascii_lowercase();
    if host_lower == "localhost"
        || host_lower.ends_with(".localhost")
        || host_lower == "127.0.0.1"
        || host_lower == "[::1]"
    {
        return Err(ToolError::InvalidArgs(
            "localhost/loopback URLs are blocked".into(),
        ));
    }

    // Block numeric private IPs.
    // Strip brackets from IPv6 literal like [fe80::1].
    let bare_host = host_lower.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare_host.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(v4) => {
                if v4.is_loopback() || v4.is_private() || v4.is_link_local() {
                    return Err(ToolError::InvalidArgs(
                        "private/loopback/link-local IPs are blocked".into(),
                    ));
                }
            }
            IpAddr::V6(v6) => {
                if v6.is_loopback() {
                    return Err(ToolError::InvalidArgs(
                        "loopback IPv6 addresses are blocked".into(),
                    ));
                }
            }
        }
    }

    Ok(())
}

const NAME: &str = "web_fetch";
const DESCRIPTION: &str = "Fetch a URL and return its text content (tags stripped, whitespace collapsed). Cap the response by approximate token count to keep context small. Use after web_search to actually read what a result is about.";
const EXAMPLE: &str = r#"{"url":"https://example.com/article","max_tokens":2000}"#;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InnerArgs {
    url: String,
    #[serde(default)]
    max_tokens: Option<u32>,
}

#[derive(Clone)]
pub struct WebFetch {
    ctx: HarnessContext,
}

impl WebFetch {
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

impl Tool for WebFetch {
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
            "required": ["url"],
            "properties": {
                "url": { "type": "string" },
                "max_tokens": { "type": "integer", "description": "Cap on returned content. Defaults to settings.web_fetch_default_max_tokens." }
            }
        }),
    }
}

async fn run(ctx: &HarnessContext, args: InnerArgs) -> Result<ToolResult, ToolError> {
    let start = Instant::now();

    // SSRF protection: reject private IPs, localhost, non-http schemes.
    url_is_safe(&args.url)?;

    // Clamp BOTH ends (TQ-3): the floor stops a degenerate tiny cap, the ceiling
    // stops a model passing max_tokens: 500000 and pulling ~2M chars of page text
    // into the context window. Every sibling tool hard-caps its upper bound too.
    let max_tokens = args
        .max_tokens
        .unwrap_or(ctx.settings.web_fetch_default_max_tokens)
        .clamp(100, 32_000);

    let res = ctx
        .http
        .get(&args.url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (compatible; SkillDeckHarness/0.1)",
        )
        .send()
        .await?;
    let status = res.status();
    if !status.is_success() {
        return Err(ToolError::Network(format!("HTTP {status}")));
    }
    let content_type = res
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let raw_bytes = res.bytes().await?;
    // Cap response body size to prevent memory exhaustion on huge pages.
    let raw = if raw_bytes.len() > MAX_RESPONSE_BYTES {
        String::from_utf8_lossy(&raw_bytes[..MAX_RESPONSE_BYTES]).into_owned()
    } else {
        String::from_utf8_lossy(&raw_bytes).into_owned()
    };
    let cleaned = if content_type.contains("html") {
        strip_html(&raw)
    } else {
        raw.clone()
    };

    // Cap by approximate tokens using the OpenAI tokenizer as an estimator.
    let mut capped = cleaned.clone();
    let est = Counter::count(&TokenizerKind::TiktokenO200kBase, &capped);
    if est > max_tokens {
        // Roughly proportionally trim. 1 token ~ 4 chars.
        let target_chars = (max_tokens as usize) * 4;
        capped = capped.chars().take(target_chars).collect();
        capped.push_str("\n\n…[truncated]");
    }
    let final_tokens = Counter::count(&TokenizerKind::TiktokenO200kBase, &capped);

    let snippet = ToolResult::snippet_from(&capped);
    Ok(ToolResult {
        title: format!("{NAME}({})", args.url),
        kind: "web-fetch".into(),
        snippet,
        data: json!({
            "url": args.url,
            "content_type": content_type,
            "body": capped,
            "approx_tokens": final_tokens,
            "original_chars": raw.chars().count(),
        }),
        items_count: Some(1),
        source: Some("web".into()),
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

fn strip_html(s: &str) -> String {
    use std::sync::OnceLock;
    // Compiled once per process (they used to be rebuilt on every fetched
    // page). expect() is justified: the patterns are compile-time literals,
    // exercised by tests — Regex::new on them is infallible.
    static SCRIPT_RE: OnceLock<Regex> = OnceLock::new();
    static STYLE_RE: OnceLock<Regex> = OnceLock::new();
    static TAG_RE: OnceLock<Regex> = OnceLock::new();
    static WS_RE: OnceLock<Regex> = OnceLock::new();
    // Drop scripts and styles first so we don't leak code into the text.
    let script_re =
        SCRIPT_RE.get_or_init(|| Regex::new(r"(?si)<script.*?</script>").expect("static regex"));
    let style_re =
        STYLE_RE.get_or_init(|| Regex::new(r"(?si)<style.*?</style>").expect("static regex"));
    let tag_re = TAG_RE.get_or_init(|| Regex::new(r"<[^>]+>").expect("static regex"));
    let ws_re = WS_RE.get_or_init(|| Regex::new(r"\s+").expect("static regex"));
    let s = script_re.replace_all(s, "");
    let s = style_re.replace_all(&s, "");
    let s = tag_re.replace_all(&s, " ");
    ws_re.replace_all(&s, " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_removes_scripts_styles_tags() {
        let html = r#"<html><head><style>.a{color:red}</style><script>alert("x")</script></head>
            <body><h1>Title</h1><p>Body   text</p></body></html>"#;
        let out = strip_html(html);
        assert_eq!(out, "Title Body text");
    }

    #[test]
    fn url_guard_still_blocks_private_targets() {
        assert!(url_is_safe("https://example.com/page").is_ok());
        assert!(url_is_safe("http://127.0.0.1:8080/admin").is_err());
        assert!(url_is_safe("http://localhost/x").is_err());
        assert!(url_is_safe("ftp://example.com").is_err());
        assert!(url_is_safe("http://10.0.0.5/internal").is_err());
    }
}
