// @agent-context: Ollama provider — talks to a local Ollama daemon.
//
// Default endpoint http://127.0.0.1:11434.
// Models listed via GET /api/tags. Streaming chat via POST /api/chat with
// `"stream": true` which yields newline-delimited JSON objects; the final
// object has `"done": true`.
//
// Tool calls: when `tools` is supplied, the model can return
// `message.tool_calls = [...]` in a single (non-fragmented) ndjson line
// — Ollama doesn't fragment tool-call JSON the way OpenAI SSE does, so we
// emit one ToolCallDelta carrying the full arguments_json per call.

use super::provider::{
    ChatChunk, ChatMessage, ChatRequest, ChatRole, ChunkSink, LlmProvider, ModelInfo,
    ProviderConfig, ProviderError, ProviderHealth, ToolCall, ToolDefinition,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:11434";
const OLLAMA_DETECT_ENDPOINTS: &[&str] =
    &["http://127.0.0.1:11434", "http://localhost:11434"];

pub struct OllamaProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(config: ProviderConfig) -> Self {
        // Long timeout — model load can take many seconds.
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .expect("reqwest client");
        Self { config, client }
    }

    fn endpoint(&self) -> String {
        self.config
            .endpoint
            .clone()
            .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string())
            .trim_end_matches('/')
            .to_string()
    }
}

/// Probe localhost (and any user override) and return the first reachable
/// Ollama base URL, plus the model list it advertises.
pub async fn detect_local() -> Option<DetectedOllama> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(800))
        .build()
        .ok()?;

    for ep in OLLAMA_DETECT_ENDPOINTS {
        let url = format!("{}/api/tags", ep);
        if let Ok(res) = client.get(&url).send().await {
            if res.status().is_success() {
                let models = parse_tag_list(res).await.unwrap_or_default();
                return Some(DetectedOllama {
                    endpoint: (*ep).to_string(),
                    models,
                });
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedOllama {
    pub endpoint: String,
    pub models: Vec<ModelInfo>,
}

async fn parse_tag_list(res: reqwest::Response) -> Result<Vec<ModelInfo>, ProviderError> {
    #[derive(Deserialize)]
    struct TagList {
        models: Vec<Tag>,
    }
    #[derive(Deserialize)]
    struct Tag {
        name: String,
        #[serde(default)]
        details: Option<TagDetails>,
    }
    #[derive(Deserialize)]
    struct TagDetails {
        #[serde(default)]
        parameter_size: Option<String>,
    }
    let body = res
        .json::<TagList>()
        .await
        .map_err(|e| ProviderError::InvalidResponse(format!("tag list: {}", e)))?;
    Ok(body
        .models
        .into_iter()
        .map(|t| {
            let label = match t.details.as_ref().and_then(|d| d.parameter_size.as_ref()) {
                Some(size) => format!("{} ({})", t.name, size),
                None => t.name.clone(),
            };
            ModelInfo {
                id: t.name,
                label,
                context_window: None,
                // Most modern Ollama models support tools; we surface true and let the model
                // ignore tools if it doesn't understand them.
                supports_tools: true,
            }
        })
        .collect())
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn id(&self) -> &str {
        &self.config.id
    }
    fn kind(&self) -> &'static str {
        "ollama"
    }

    async fn health(&self) -> ProviderHealth {
        let url = format!("{}/api/tags", self.endpoint());
        match self.client.get(&url).send().await {
            Ok(res) if res.status().is_success() => ProviderHealth {
                ok: true,
                detail: Some(format!("reachable at {}", self.endpoint())),
            },
            Ok(res) => ProviderHealth {
                ok: false,
                detail: Some(format!("HTTP {}", res.status())),
            },
            Err(e) => ProviderHealth {
                ok: false,
                detail: Some(format!("{}", e)),
            },
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let url = format!("{}/api/tags", self.endpoint());
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        if !res.status().is_success() {
            return Err(ProviderError::Provider(format!("HTTP {}", res.status())));
        }
        parse_tag_list(res).await
    }

    async fn stream_chat(
        &self,
        req: ChatRequest,
        on_chunk: ChunkSink,
    ) -> Result<(), ProviderError> {
        let url = format!("{}/api/chat", self.endpoint());
        let payload = build_chat_payload(&req);
        let res = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(ProviderError::Provider(format!(
                "HTTP {}: {}",
                status,
                body.chars().take(200).collect::<String>()
            )));
        }

        let mut stream = res.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();

        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    on_chunk(ChatChunk::Error {
                        message: format!("stream: {}", e),
                    });
                    return Err(ProviderError::Network(e.to_string()));
                }
            };
            buf.extend_from_slice(&bytes);
            // Ollama yields ndjson — split on '\n'.
            while let Some(nl) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=nl).collect();
                let line_str = String::from_utf8_lossy(&line);
                let line_trim = line_str.trim();
                if line_trim.is_empty() {
                    continue;
                }
                handle_ndjson_line(line_trim, &on_chunk);
            }
        }
        // Drain any trailing partial line.
        if !buf.is_empty() {
            let line_str = String::from_utf8_lossy(&buf);
            let line_trim = line_str.trim();
            if !line_trim.is_empty() {
                handle_ndjson_line(line_trim, &on_chunk);
            }
        }
        Ok(())
    }
}

fn build_chat_payload(req: &ChatRequest) -> Value {
    let mut messages = Vec::new();
    for m in &req.messages {
        messages.push(serialize_message(m));
    }
    let mut payload = json!({
        "model": req.model,
        "messages": messages,
        "stream": true,
    });
    if !req.tools.is_empty() {
        payload["tools"] = serialize_tools(&req.tools);
    }
    if let Some(t) = req.temperature {
        payload["options"] = json!({ "temperature": t });
    }
    if let Some(mt) = req.max_tokens {
        let options = payload
            .as_object_mut()
            .unwrap()
            .entry("options".to_string())
            .or_insert_with(|| json!({}));
        options["num_predict"] = json!(mt);
    }
    payload
}

fn serialize_message(m: &ChatMessage) -> Value {
    let role = match m.role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
        ChatRole::Tool => "tool",
    };
    let mut obj = json!({
        "role": role,
        "content": m.content,
    });
    if !m.tool_calls.is_empty() {
        obj["tool_calls"] = m
            .tool_calls
            .iter()
            .map(|c| {
                let args: Value = serde_json::from_str(&c.arguments_json)
                    .unwrap_or(Value::String(c.arguments_json.clone()));
                json!({
                    "function": {
                        "name": c.name,
                        "arguments": args,
                    }
                })
            })
            .collect();
    }
    if let Some(name) = &m.tool_name {
        obj["name"] = json!(name);
    }
    obj
}

fn serialize_tools(tools: &[ToolDefinition]) -> Value {
    tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            })
        })
        .collect()
}

fn handle_ndjson_line(line: &str, on_chunk: &ChunkSink) {
    let parsed: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return,
    };

    // Content delta.
    if let Some(text) = parsed
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
    {
        if !text.is_empty() {
            on_chunk(ChatChunk::ContentDelta {
                text: text.to_string(),
            });
        }
    }

    // Tool calls (Ollama emits the full call atomically per ndjson line).
    if let Some(calls) = parsed
        .get("message")
        .and_then(|m| m.get("tool_calls"))
        .and_then(|v| v.as_array())
    {
        for (idx, call) in calls.iter().enumerate() {
            let func = call.get("function");
            let name = func
                .and_then(|f| f.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or_default()
                .to_string();
            let args_value = func.and_then(|f| f.get("arguments"));
            let arguments_json = match args_value {
                Some(v) if v.is_string() => v.as_str().unwrap_or("").to_string(),
                Some(v) => serde_json::to_string(v).unwrap_or_default(),
                None => String::new(),
            };
            // Synthesize a stable id; Ollama doesn't always provide one.
            let id = call
                .get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("ollama-{}-{}", name, idx));
            on_chunk(ChatChunk::ToolCallDelta {
                id,
                name: Some(name),
                arguments_json_delta: arguments_json,
            });
        }
    }

    if parsed
        .get("done")
        .and_then(|d| d.as_bool())
        .unwrap_or(false)
    {
        let reason = parsed
            .get("done_reason")
            .and_then(|r| r.as_str())
            .unwrap_or("stop")
            .to_string();
        on_chunk(ChatChunk::Finish { reason });
    }
}

#[allow(dead_code)]
fn _toolcall_for_doc() -> ToolCall {
    ToolCall {
        id: String::new(),
        name: String::new(),
        arguments_json: String::new(),
    }
}
