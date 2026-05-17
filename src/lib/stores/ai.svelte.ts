/**
 * @agent-context: AI panel state — visibility, sessions, streaming, providers.
 *
 * All state lives on a single instance so callers can read fields directly
 * (Svelte 5 runes track reads). Mutations go through the exported helpers,
 * which also drive Tauri IPC calls.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AgentEvent,
  AgentTurnResult,
  AiHealthResult,
  AiModelInfo,
  AiProviderConfig,
  AiProviderRegistrySnapshot,
  AiSession,
  AiSessionMessage,
  AiToolCall,
  DetectedOllama,
} from "$lib/types/ai";

export type ChatView = "chat" | "settings" | "sessions" | "debug";

export interface PendingToolCall {
  callId: string;
  name: string;
  argumentsJson: string;
  status: "running" | "success" | "error";
  label?: string;
  data?: unknown;
  errorMessage?: string;
}

export interface ChatPendingMessage {
  id: string;
  text: string;
  toolCalls: PendingToolCall[];
}

class AIStore {
  open = $state(false);
  view = $state<ChatView>("chat");

  providers = $state<AiProviderConfig[]>([]);
  activeProvider = $state<string | null>(null);
  activeModel = $state<string | null>(null);

  modelsByProvider = $state<Record<string, AiModelInfo[]>>({});
  healthByProvider = $state<Record<string, AiHealthResult>>({});

  sessions = $state<AiSession[]>([]);
  activeSession = $state<AiSession | null>(null);

  pending = $state<ChatPendingMessage | null>(null);
  isStreaming = $state(false);
  lastError = $state<string | null>(null);

  detectedOllama = $state<DetectedOllama | null>(null);

  /**
   * Per-tool-card expand/collapse state lifted out of the component so it
   * survives the AIPanel unmount that happens when the user hides the
   * overlay via the tray. Keyed by `{sessionId}:{callId}` for persisted
   * cards and `{pendingId}:{callId}` while streaming.
   */
  toolCardExpansion = $state<Record<string, boolean>>({});
}

export const aiStore = new AIStore();

let agentEventUnlisten: UnlistenFn | undefined;
let listenerInitialized = false;

export async function initAiPanel(): Promise<void> {
  if (!listenerInitialized) {
    listenerInitialized = true;
    agentEventUnlisten = await listen<AgentEvent>("ai-agent-event", (event) => {
      handleAgentEvent(event.payload);
    });
  }
  await refreshRegistry();
  await refreshSessions();
}

export async function disposeAiPanel(): Promise<void> {
  agentEventUnlisten?.();
  agentEventUnlisten = undefined;
  listenerInitialized = false;
}

export function openAiPanel(view: ChatView = "chat"): void {
  aiStore.view = view;
  aiStore.open = true;
}

export function closeAiPanel(): void {
  aiStore.open = false;
}

export function toggleAiPanel(): void {
  aiStore.open = !aiStore.open;
}

export async function refreshRegistry(): Promise<void> {
  const snap = await invoke<AiProviderRegistrySnapshot>("ai_list_providers");
  aiStore.providers = snap.providers;
  aiStore.activeProvider = snap.activeProvider;
  aiStore.activeModel = snap.activeModel;
}

export async function refreshSessions(): Promise<void> {
  aiStore.sessions = await invoke<AiSession[]>("ai_list_sessions");
}

export async function detectOllama(): Promise<DetectedOllama | null> {
  const detected = await invoke<DetectedOllama | null>("ai_detect_ollama");
  aiStore.detectedOllama = detected;
  return detected;
}

export async function saveProvider(config: AiProviderConfig): Promise<void> {
  const snap = await invoke<AiProviderRegistrySnapshot>("ai_save_provider", { config });
  aiStore.providers = snap.providers;
  aiStore.activeProvider = snap.activeProvider;
  aiStore.activeModel = snap.activeModel;
}

export async function deleteProvider(providerId: string): Promise<void> {
  const snap = await invoke<AiProviderRegistrySnapshot>("ai_delete_provider", {
    providerId,
  });
  aiStore.providers = snap.providers;
  aiStore.activeProvider = snap.activeProvider;
  aiStore.activeModel = snap.activeModel;
}

export async function setActiveSelection(
  providerId: string | null,
  model: string | null,
): Promise<void> {
  const snap = await invoke<AiProviderRegistrySnapshot>("ai_set_active_selection", {
    providerId,
    model,
  });
  aiStore.providers = snap.providers;
  aiStore.activeProvider = snap.activeProvider;
  aiStore.activeModel = snap.activeModel;
}

export async function probeHealth(providerId: string): Promise<AiHealthResult> {
  const result = await invoke<AiHealthResult>("ai_health", { providerId });
  aiStore.healthByProvider = { ...aiStore.healthByProvider, [providerId]: result };
  return result;
}

export async function listModels(providerId: string): Promise<AiModelInfo[]> {
  const models = await invoke<AiModelInfo[]>("ai_list_models", { providerId });
  aiStore.modelsByProvider = { ...aiStore.modelsByProvider, [providerId]: models };
  return models;
}

export async function loadSession(sessionId: string): Promise<void> {
  aiStore.activeSession = await invoke<AiSession>("ai_get_session", { sessionId });
}

export function startNewSession(): void {
  aiStore.activeSession = null;
  aiStore.pending = null;
  aiStore.lastError = null;
}

export async function deleteSession(sessionId: string): Promise<void> {
  await invoke<void>("ai_delete_session", { sessionId });
  if (aiStore.activeSession?.id === sessionId) {
    aiStore.activeSession = null;
  }
  await refreshSessions();
}

export async function sendMessage(text: string): Promise<void> {
  const trimmed = text.trim();
  if (!trimmed) return;
  if (!aiStore.activeProvider || !aiStore.activeModel) {
    aiStore.lastError = "Choose an AI provider and model in Settings first.";
    aiStore.view = "settings";
    aiStore.open = true;
    return;
  }
  if (aiStore.isStreaming) return;

  aiStore.lastError = null;
  aiStore.isStreaming = true;

  const sessionId = aiStore.activeSession?.id ?? null;
  appendUserMessage(trimmed);

  aiStore.pending = {
    id: `pending-${Date.now()}`,
    text: "",
    toolCalls: [],
  };

  try {
    const result = await invoke<AgentTurnResult>("ai_chat_send", {
      sessionId,
      userText: trimmed,
      providerId: aiStore.activeProvider,
      model: aiStore.activeModel,
    });
    await loadSession(result.sessionId);
    await refreshSessions();
    aiStore.pending = null;
  } catch (err: unknown) {
    aiStore.lastError = err instanceof Error ? err.message : String(err);
    // Even on cancel, persist whatever the agent saved so the UI shows the partial transcript.
    const idForReload = aiStore.activeSession?.id;
    if (idForReload) {
      try {
        await loadSession(idForReload);
        await refreshSessions();
      } catch {
        /* session may not exist yet — ignore */
      }
    }
    aiStore.pending = null;
  } finally {
    aiStore.isStreaming = false;
  }
}

export async function cancelCurrentTurn(): Promise<void> {
  const id = aiStore.activeSession?.id;
  if (!id) return;
  try {
    await invoke<void>("ai_cancel_turn", { sessionId: id });
  } catch (err: unknown) {
    aiStore.lastError = err instanceof Error ? err.message : String(err);
  }
}

function appendUserMessage(text: string): void {
  if (!aiStore.activeSession) {
    // Synthesize a placeholder session so the user sees their own message immediately.
    aiStore.activeSession = {
      id: "",
      title: text.slice(0, 60),
      providerId: aiStore.activeProvider ?? "",
      model: aiStore.activeModel ?? "",
      createdAt: Math.floor(Date.now() / 1000),
      updatedAt: Math.floor(Date.now() / 1000),
      messages: [],
    };
  }
  const msg: AiSessionMessage = {
    id: `local-user-${Date.now()}`,
    role: "user",
    content: text,
    createdAt: Math.floor(Date.now() / 1000),
  };
  aiStore.activeSession = {
    ...aiStore.activeSession,
    messages: [...aiStore.activeSession.messages, msg],
    updatedAt: Math.floor(Date.now() / 1000),
  };
}

function handleAgentEvent(event: AgentEvent): void {
  switch (event.kind) {
    case "sessionStarted":
      if (aiStore.activeSession && !aiStore.activeSession.id) {
        aiStore.activeSession = { ...aiStore.activeSession, id: event.sessionId };
      }
      return;
    case "contentDelta":
      if (!aiStore.pending) return;
      aiStore.pending = {
        ...aiStore.pending,
        text: aiStore.pending.text + event.text,
      };
      return;
    case "toolCallStart":
      if (!aiStore.pending) return;
      aiStore.pending = {
        ...aiStore.pending,
        toolCalls: [
          ...aiStore.pending.toolCalls,
          {
            callId: event.callId,
            name: event.name,
            argumentsJson: event.argumentsJson,
            status: "running",
          },
        ],
      };
      return;
    case "toolCallResult":
      if (!aiStore.pending) return;
      aiStore.pending = {
        ...aiStore.pending,
        toolCalls: aiStore.pending.toolCalls.map((c) =>
          c.callId === event.callId
            ? { ...c, status: "success", label: event.label, data: event.data }
            : c,
        ),
      };
      return;
    case "toolCallError":
      if (!aiStore.pending) return;
      aiStore.pending = {
        ...aiStore.pending,
        toolCalls: aiStore.pending.toolCalls.map((c) =>
          c.callId === event.callId
            ? { ...c, status: "error", errorMessage: event.message }
            : c,
        ),
      };
      return;
    case "finish":
      return;
    case "error":
      aiStore.lastError = event.message;
      return;
  }
}

/**
 * Helper used by AISettings to seed a one-click Ollama provider from
 * `detect_local`. Idempotent — updates the existing entry if found.
 */
export async function seedOllamaProvider(detected: DetectedOllama): Promise<void> {
  const existing = aiStore.providers.find((p) => p.kind === "ollama");
  const config: AiProviderConfig = existing
    ? { ...existing, endpoint: detected.endpoint }
    : {
        id: `ollama-local-${Date.now()}`,
        kind: "ollama",
        label: "Ollama (local)",
        endpoint: detected.endpoint,
        defaultModel: detected.models[0]?.id ?? null,
      };
  await saveProvider(config);
  if (!aiStore.activeProvider) {
    await setActiveSelection(
      config.id,
      config.defaultModel ?? detected.models[0]?.id ?? null,
    );
  }
}

export function getToolCardExpanded(cardKey: string, fallback: boolean): boolean {
  const v = aiStore.toolCardExpansion[cardKey];
  return v === undefined ? fallback : v;
}

export function setToolCardExpanded(cardKey: string, expanded: boolean): void {
  aiStore.toolCardExpansion = { ...aiStore.toolCardExpansion, [cardKey]: expanded };
}

export function formatToolCallArguments(args: string): string {
  if (!args) return "";
  try {
    const parsed = JSON.parse(args);
    return JSON.stringify(parsed, null, 2);
  } catch {
    return args;
  }
}

export function isAiToolCall(value: unknown): value is AiToolCall {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return typeof v.id === "string" && typeof v.name === "string";
}
