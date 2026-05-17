/**
 * @agent-context: Frontend mirrors of the Rust AI types.
 * Field names use camelCase to match `#[serde(rename_all = "camelCase")]`.
 */

export type AiProviderKind = "ollama" | "azure-foundry";

export interface AiProviderConfig {
  id: string;
  kind: AiProviderKind | string;
  label: string;
  endpoint?: string | null;
  apiKey?: string | null;
  deployment?: string | null;
  apiVersion?: string | null;
  defaultModel?: string | null;
  notes?: string | null;
}

export interface AiProviderRegistrySnapshot {
  providers: AiProviderConfig[];
  activeProvider: string | null;
  activeModel: string | null;
}

export interface AiHealthResult {
  ok: boolean;
  detail: string | null;
}

export interface AiModelInfo {
  id: string;
  label: string;
  contextWindow: number | null;
  supportsTools: boolean;
}

export interface DetectedOllama {
  endpoint: string;
  models: AiModelInfo[];
}

export type AiChatRole = "system" | "user" | "assistant" | "tool";

export interface AiToolCall {
  id: string;
  name: string;
  argumentsJson: string;
}

export interface AiSessionMessage {
  id: string;
  role: AiChatRole;
  content: string;
  toolCalls?: AiToolCall[];
  toolCallId?: string | null;
  toolName?: string | null;
  toolLabel?: string | null;
  toolError?: string | null;
  createdAt: number;
}

export interface ToolDispatchRecord {
  name: string;
  label?: string | null;
  success: boolean;
  error?: string | null;
  durationMs: number;
}

export interface AgentTurnTelemetry {
  userText: string;
  iterations: number;
  totalChunks: number;
  totalAssistantChars: number;
  durationMs: number;
  toolDispatches: ToolDispatchRecord[];
  finishReason?: string | null;
  error?: string | null;
}

export interface AiSession {
  id: string;
  title: string;
  providerId: string;
  model: string;
  createdAt: number;
  updatedAt: number;
  messages: AiSessionMessage[];
  lastTelemetry?: AgentTurnTelemetry | null;
}

export interface AgentTurnResult {
  sessionId: string;
  assistantText: string;
  toolCallsMade: number;
}

export type AgentEvent =
  | { kind: "sessionStarted"; sessionId: string }
  | { kind: "contentDelta"; sessionId: string; text: string }
  | {
      kind: "toolCallStart";
      sessionId: string;
      callId: string;
      name: string;
      argumentsJson: string;
    }
  | {
      kind: "toolCallResult";
      sessionId: string;
      callId: string;
      name: string;
      label: string;
      data: unknown;
    }
  | {
      kind: "toolCallError";
      sessionId: string;
      callId: string;
      name: string;
      message: string;
    }
  | { kind: "finish"; sessionId: string; reason: string }
  | { kind: "error"; sessionId: string; message: string };
