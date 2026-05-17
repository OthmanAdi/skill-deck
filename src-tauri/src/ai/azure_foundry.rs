// @agent-context: Azure AI Foundry provider — OpenAI-compatible REST.
//
// Endpoint shapes accepted from settings:
//   https://<resource>.openai.azure.com
//   https://<resource>.services.ai.azure.com
//
// Chat completions: POST {endpoint}/openai/v1/chat/completions
// Auth: header `api-key: {key}` (legacy) or `Authorization: Bearer ...`
// (Entra). We support api-key here since the user said "API key" — Entra
// flow can be added later without changing the trait.
//
// Streaming format: Server-Sent Events. Each `data: {json}` line carries
// `choices[0].delta` with `content` and/or `tool_calls`. `data: [DONE]`
// terminates the stream.

use super::provider::{
    ChatChunk, ChatMessage, ChatRequest, ChatRole, ChunkSink, LlmProvider, ModelInfo,
    ProviderConfig, ProviderError, ProviderHealth, ToolDefinition,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

const DEFAULT_API_VERSION: &str = "2024-10-21";

pub struct AzureFoundryProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl AzureFoundryProvider {
    pub fn new(config: ProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .build()
            .expect("reqwest client");
        Self { config, client }
    }

    fn endpoint(&self) -> Result<String, ProviderError> {
        self.config
            .endpoint
            .clone()
            .map(|s| s.trim_end_matches('/').to_string())
            .ok_or_else(|| {
                ProviderError::NotConfigured("Azure endpoint URL is required".into())
            })
    }

    fn api_key(&self) -> Result<String, ProviderError> {
        self.config
            .api_key
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ProviderError::NotConfigured("Azure API key is required".into()))
    }

    fn api_version(&self) -> String {
        self.config
            .api_version
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_API_VERSION.to_string())
    }
}

#[async_trait]
impl LlmProvider for AzureFoundryProvider {
    fn id(&self) -> &str {
        &self.config.id
    }
    fn kind(&self) -> &'static str {
        "azure-foundry"
    }

    async fn health(&self) -> ProviderHealth {
        let endpoint = match self.endpoint() {
            Ok(e) => e,
            Err(e) => {
                return ProviderHealth {
                    ok: false,
                    detail: Some(e.user_message()),
                }
            }
        };
        let api_key = match self.api_key() {
            Ok(k) => k,
            Err(e) => {
                return ProviderHealth {
                    ok: false,
                    detail: Some(e.user_message()),
                }
            }
        };
        // models list endpoint is the cheapest health probe.
        let url = format!("{}/openai/models?api-version={}", endpoint, self.api_version());
        match self
            .client
            .get(&url)
            .header("api-key", &api_key)
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => ProviderHealth {
                ok: true,
                detail: Some(format!("reachable at {}", endpoint)),
            },
            Ok(res) => {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                ProviderHealth {
                    ok: false,
                    detail: Some(format!(
                        "HTTP {}: {}",
                        status,
                        body.chars().take(160).collect::<String>()
                    )),
                }
            }
            Err(e) => ProviderHealth {
                ok: false,
                detail: Some(format!("{}", e)),
            },
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let endpoint = self.endpoint()?;
        let api_key = self.api_key()?;
        let url = format!("{}/openai/models?api-version={}", endpoint, self.api_version());

        #[derive(Deserialize)]
        struct ModelsResponse {
            data: Vec<ModelEntry>,
        }
        #[derive(Deserialize)]
        struct ModelEntry {
            id: String,
            #[serde(default)]
            model: Option<String>,
        }

        let res = self
            .client
            .get(&url)
            .header("api-key", &api_key)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        if !res.status().is_success() {
            return Err(ProviderError::Provider(format!("HTTP {}", res.status())));
        }
        let parsed: ModelsResponse = res
            .json()
            .await
            .map_err(|e| ProviderError::InvalidResponse(e.to_string()))?;
        Ok(parsed
            .data
            .into_iter()
            .map(|m| {
                let label = m.model.clone().unwrap_or_else(|| m.id.clone());
                ModelInfo {
                    id: m.id,
                    label,
                    context_window: None,
                    supports_tools: true,
                }
            })
            .collect())
    }

    async fn stream_chat(
        &self,
        req: ChatRequest,
        on_chunk: ChunkSink,
    ) -> Result<(), ProviderError> {
        let endpoint = self.endpoint()?;
        let api_key = self.api_key()?;
        // model: explicit > config default > caller value
        let model = if !req.model.is_empty() {
            req.model.clone()
        } else {
            self.config
                .default_model
                .clone()
                .ok_or_else(|| ProviderError::NotConfigured("model required".into()))?
        };
        let url = format!(
            "{}/openai/v1/chat/completions?api-version={}",
            endpoint,
            self.api_version()
        );

        let payload = build_payload(&model, &req);

        let res = self
            .client
            .post(&url)
            .header("api-key", &api_key)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
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
            // SSE frames are separated by blank lines (\n\n).
            while let Some(idx) = find_double_newline(&buf) {
                let frame: Vec<u8> = buf.drain(..idx + 2).collect();
                let frame_str = String::from_utf8_lossy(&frame);
                process_sse_frame(&frame_str, &on_chunk);
            }
        }
        if !buf.is_empty() {
            let frame_str = String::from_utf8_lossy(&buf);
            process_sse_frame(&frame_str, &on_chunk);
        }
        Ok(())
    }
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

fn build_payload(model: &str, req: &ChatRequest) -> Value {
    let mut messages = Vec::new();
    for m in &req.messages {
        messages.push(serialize_message(m));
    }
    let mut payload = json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });
    if !req.tools.is_empty() {
        payload["tools"] = serialize_tools(&req.tools);
        payload["tool_choice"] = json!("auto");
    }
    if let Some(t) = req.temperature {
        payload["temperature"] = json!(t);
    }
    if let Some(mt) = req.max_tokens {
        payload["max_tokens"] = json!(mt);
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
                json!({
                    "id": c.id,
                    "type": "function",
                    "function": {
                        "name": c.name,
                        "arguments": c.arguments_json,
                    }
                })
            })
            .collect();
    }
    if let Some(call_id) = &m.tool_call_id {
        obj["tool_call_id"] = json!(call_id);
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

fn process_sse_frame(frame: &str, on_chunk: &ChunkSink) {
    for raw_line in frame.split('\n') {
        let line = raw_line.trim_start();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let data = match line.strip_prefix("data:") {
            Some(d) => d.trim(),
            None => continue,
        };
        if data == "[DONE]" {
            on_chunk(ChatChunk::Finish {
                reason: "stop".into(),
            });
            continue;
        }
        let parsed: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let choices = match parsed.get("choices").and_then(|c| c.as_array()) {
            Some(c) => c,
            None => continue,
        };
        for choice in choices {
            let delta = choice.get("delta");

            if let Some(text) = delta
                .and_then(|d| d.get("content"))
                .and_then(|c| c.as_str())
            {
                if !text.is_empty() {
                    on_chunk(ChatChunk::ContentDelta {
                        text: text.to_string(),
                    });
                }
            }

            if let Some(tcs) = delta
                .and_then(|d| d.get("tool_calls"))
                .and_then(|v| v.as_array())
            {
                for tc in tcs {
                    let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                    let id = tc
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("call-{}", idx));
                    let func = tc.get("function");
                    let name = func
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string());
                    let args = func
                        .and_then(|f| f.get("arguments"))
                        .and_then(|a| a.as_str())
                        .unwrap_or("")
                        .to_string();
                    on_chunk(ChatChunk::ToolCallDelta {
                        id,
                        name,
                        arguments_json_delta: args,
                    });
                }
            }

            if let Some(finish) = choice
                .get("finish_reason")
                .and_then(|f| f.as_str())
                .map(|s| s.to_string())
            {
                if !finish.is_empty() {
                    on_chunk(ChatChunk::Finish { reason: finish });
                }
            }
        }
    }
}
