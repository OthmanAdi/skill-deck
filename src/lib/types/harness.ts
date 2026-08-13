/**
 * @agent-context: Frontend mirrors of the Rust harness types.
 * Field names use camelCase to match the Rust serde rename rule.
 */

export type ProviderKind = "ollama" | "azure" | string;

export interface ProviderConfig {
  id: string;
  kind: ProviderKind;
  label: string;
  endpoint?: string | null;
  apiKey?: string | null;
  deployment?: string | null;
  apiVersion?: string | null;
  defaultModel?: string | null;
  notes?: string | null;
}

export interface ProviderSnapshot {
  providers: ProviderConfig[];
  activeProvider: string | null;
  activeModel: string | null;
}

export interface HarnessSettings {
  memoryCompactionFloor: number;
  localCompactionOnly: boolean;
  toolResultSpillThresholdChars: number;
  itemCountDefault: number;
  maxTurns: number;
  maxTokens: number;
  webSearchProvider: string;
  webSearchApiKey: string | null;
  modelsCacheTtlHours: number;
  webFetchDefaultMaxTokens: number;
  minimumFinalAnswerTokens: number;
  contextSafetyFraction: number;
  contextSafetyMinTokens: number;
  toolContextFraction: number;
  toolResultMaxTokens: number;
  summaryMaxTokens: number;
  maxToolSteps: number;
  maxSameToolCalls: number;
}

export type HarnessErrorKind =
  | "providerUnreachable"
  | "providerAuth"
  | "providerTimeout"
  | "modelNotFound"
  | "rateLimited"
  | "toolFailed"
  | "contextBudgetExceeded"
  | "cancelled"
  | "busy"
  | "internal";

export interface HarnessError {
  kind: HarnessErrorKind;
  message: string;
}

export interface HealthReport {
  ok: boolean;
  status: string;
  detail: string | null;
}

export interface ModelInfo {
  id: string;
  label: string;
  contextWindow: number | null;
  supportsTools: boolean;
}

export interface DetectedOllama {
  endpoint: string;
  models: ModelInfo[];
}

export interface ToolResult {
  title: string;
  kind: string;
  snippet: string;
  data: unknown;
  itemsCount: number | null;
  source: string | null;
  elapsedMs: number;
}

export interface EnvelopeSource {
  title: string;
  url: string;
  kind: string;
}

export interface MessageRecord {
  id: string;
  role: "user" | "assistant" | "tool" | string;
  text: string;
  cleared: boolean;
  spilledPath: string | null;
  createdAt: number;
  /** Tool-record-only — the tool name (e.g. "search_skills"). */
  name?: string | null;
  /** Tool-record-only — raw args JSON. */
  argsJson?: string | null;
  /** Tool-record-only — rig's call id, to dedupe pending + persisted cards. */
  toolCallId?: string | null;
  /** Tool-record-only — full envelope when the tool succeeded. */
  toolResult?: ToolResult | null;
  /** Tool-record-only — error message when the tool failed. */
  toolError?: string | null;
}

export interface SpillRef {
  blockId: string;
  path: string;
  byteCount: number;
}

export interface ConversationMemory {
  sessionId: string;
  messages: MessageRecord[];
  runningSummary: string | null;
  spilledBlocks: SpillRef[];
}

export interface HarnessSession {
  id: string;
  title: string;
  providerId: string;
  model: string;
  createdAt: number;
  updatedAt: number;
  memory: ConversationMemory;
  finalSummary: string | null;
}

export interface TurnSummary {
  sessionId: string;
  finalText: string;
  inTokens: number;
  outTokens: number;
  durationMs: number;
  itemCountHint: number | null;
  toolCallsMade: number;
}

/* === Built-in local model (harness_builtin_* contract) ==================
 * Status is polled via harness_builtin_status(); live download/verify
 * progress streams on the separate `builtin-model-event` channel. */

export type BuiltinModelState =
  | "notDownloaded"
  | "downloading"
  | "verifying"
  | "ready"
  | "running";

export interface BuiltinModelStatus {
  state: BuiltinModelState;
  progressPct?: number | null;
  bytesDone?: number | null;
  bytesTotal?: number | null;
  modelName: string;
  sizeBytes?: number | null;
  path?: string | null;
  port?: number | null;
}

export interface BuiltinModelEvent {
  phase: "downloading" | "verifying" | "ready" | "error";
  pct: number;
  bytesDone: number;
  bytesTotal: number;
  message?: string | null;
}

export type HarnessEvent =
  | { kind: "sessionStarted"; sessionId: string }
  | {
      kind: "preModel";
      sessionId: string;
      model: string;
      messageCount: number;
      estTokens: number;
    }
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
      result: ToolResult;
    }
  | {
      kind: "toolCallError";
      sessionId: string;
      callId: string;
      name: string;
      message: string;
    }
  | {
      kind: "toolCallVetoed";
      sessionId: string;
      callId: string;
      name: string;
      reason: string;
    }
  | {
      kind: "compaction";
      sessionId: string;
      strategy: string;
      freedTokens: number;
      before: number;
      after: number;
    }
  | {
      kind: "spilled";
      sessionId: string;
      blockId: string;
      byteCount: number;
      path: string;
    }
  | {
      kind: "postModel";
      sessionId: string;
      model: string;
      finishReason: string;
      inTokens: number;
      outTokens: number;
      costUsd: number | null;
    }
  | {
      kind: "finalEnvelope";
      sessionId: string;
      suggestedNextPrompts: string[];
      sources: EnvelopeSource[];
      skillsReferenced: string[];
    }
  | { kind: "finish"; sessionId: string; reason: string; durationMs: number }
  | {
      kind: "error";
      sessionId: string;
      message: string;
      errorKind: HarnessErrorKind;
    };
