/**
 * @agent-context: Harness store — Svelte 5 runes.
 * Owns: panel visibility, active session, pending stream, providers, models,
 * settings and built-in local-model status.
 * All mutation flows through the helpers so IPC stays single-pathed.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  BuiltinModelEvent,
  BuiltinModelStatus,
  DetectedOllama,
  HarnessEvent,
  HarnessError,
  HarnessSession,
  HarnessSettings,
  HealthReport,
  ModelInfo,
  ProviderConfig,
  ProviderSnapshot,
  ToolResult,
  TurnSummary,
} from "$lib/types/harness";

export type ChatView = "chat" | "wizard" | "history" | "settings";

export interface PendingToolCard {
  callId: string;
  name: string;
  argumentsJson: string;
  status: "running" | "success" | "error" | "vetoed";
  result?: ToolResult;
  errorMessage?: string;
}

export interface PendingTurn {
  id: string;
  /** Real backend session id, learned from the sessionStarted event. Lets
   * Stop/cancel work on the first turn of a brand-new chat, where activeSession
   * is still null and the backend minted its own session-<uuid> (chat-cancel-newsession). */
  sessionId?: string;
  text: string;
  toolCalls: PendingToolCard[];
  inTokens: number;
  outTokens: number;
  compactionEvents: number;
  /** User-side text surfaced as a bubble immediately on send — eliminates the
   * gap where the bubble would otherwise be invisible until IPC resolves. */
  userText: string;
  /** Set when the turn's IPC call threw. The pending turn is kept on screen
   * (not nulled) so the user's message survives a failed send and stays
   * retryable — nulling it here was the message-loss bug (chat-send-failure). */
  failed?: boolean;
}

export interface SessionEnvelope {
  suggestedNextPrompts: string[];
  sources: { title: string; url: string; kind: string }[];
  skillsReferenced: string[];
}

class HarnessStore {
  open = $state(false);
  view = $state<ChatView>("chat");
  providers = $state<ProviderConfig[]>([]);
  activeProvider = $state<string | null>(null);
  activeModel = $state<string | null>(null);
  settings = $state<HarnessSettings | null>(null);
  modelsByProvider = $state<Record<string, ModelInfo[]>>({});
  healthByProvider = $state<Record<string, HealthReport>>({});
  sessions = $state<HarnessSession[]>([]);
  activeSession = $state<HarnessSession | null>(null);
  pending = $state<PendingTurn | null>(null);
  isStreaming = $state(false);
  /** Friendly, mapped error copy shown in the transcript error pill. */
  lastError = $state<string | null>(null);
  /** Original unmapped error string, kept for the collapsed "details" line. */
  lastErrorRaw = $state<string | null>(null);
  /** The user text of the last turn that failed to send — the Retry target. */
  lastFailedUserText = $state<string | null>(null);
  /** One-shot channel for filling the composer from elsewhere (empty-state
   * example prompts). Composer watches it, adopts the value, then clears it. */
  composerPrefill = $state<string | null>(null);
  detectedOllama = $state<DetectedOllama | null>(null);
  toolCardExpansion = $state<Record<string, boolean>>({});
  /** Most-recent FinalEnvelope per session, keyed by session id. */
  envelopeBySession = $state<Record<string, SessionEnvelope>>({});
  /** Built-in local-model status (harness_builtin_status). Null when the
   * backend has no builtin support yet — the Settings card hides itself. */
  builtinStatus = $state<BuiltinModelStatus | null>(null);
  /** Last error-phase message from the builtin-model-event channel (a failed
   * download surfaces here, not as a command rejection). Cleared on progress. */
  builtinEventError = $state<string | null>(null);
}

export const harnessStore = new HarnessStore();
let unlisten: UnlistenFn | undefined;
let unlistenBuiltin: UnlistenFn | undefined;
let listenerReady = false;

export async function initHarness(): Promise<void> {
  if (!listenerReady) {
    listenerReady = true;
    unlisten = await listen<HarnessEvent>("harness-event", (event) => {
      handleEvent(event.payload);
    });
    // Separate channel: download/verify progress for the built-in model.
    unlistenBuiltin = await listen<BuiltinModelEvent>("builtin-model-event", (event) => {
      handleBuiltinEvent(event.payload);
    });
  }
  await refreshProviders();
  await refreshSettings();
  await refreshSessions();
  await refreshBuiltinStatus();
}

export async function disposeHarness(): Promise<void> {
  unlisten?.();
  unlisten = undefined;
  unlistenBuiltin?.();
  unlistenBuiltin = undefined;
  listenerReady = false;
}

export function openChat(view: ChatView = "chat"): void {
  harnessStore.view = view;
  harnessStore.open = true;
}
export function closeChat(): void {
  // If a turn is mid-flight, abort it instead of orphaning it. Hiding the
  // overlay alone left the backend (and a paid Azure provider) generating with
  // no way to stop it, and the half-streamed turn went stale (chat-escape-no-cancel).
  if (harnessStore.isStreaming) {
    void cancelCurrentTurn();
  }
  harnessStore.open = false;
}
export function toggleChat(): void {
  harnessStore.open = !harnessStore.open;
}

export async function refreshProviders(): Promise<void> {
  const snap = await invoke<ProviderSnapshot>("harness_list_providers");
  harnessStore.providers = snap.providers;
  harnessStore.activeProvider = snap.activeProvider;
  harnessStore.activeModel = snap.activeModel;
}

export async function refreshSettings(): Promise<void> {
  harnessStore.settings = await invoke<HarnessSettings>("harness_get_settings");
}

export async function refreshSessions(): Promise<void> {
  harnessStore.sessions = await invoke<HarnessSession[]>("harness_list_sessions");
}

export async function saveProvider(config: ProviderConfig): Promise<void> {
  const snap = await invoke<ProviderSnapshot>("harness_save_provider", { config });
  harnessStore.providers = snap.providers;
  harnessStore.activeProvider = snap.activeProvider;
  harnessStore.activeModel = snap.activeModel;
}

export async function deleteProvider(providerId: string): Promise<void> {
  const snap = await invoke<ProviderSnapshot>("harness_delete_provider", { providerId });
  harnessStore.providers = snap.providers;
  harnessStore.activeProvider = snap.activeProvider;
  harnessStore.activeModel = snap.activeModel;
}

export async function setActiveSelection(
  providerId: string | null,
  model: string | null,
): Promise<void> {
  const snap = await invoke<ProviderSnapshot>("harness_set_active", { providerId, model });
  harnessStore.providers = snap.providers;
  harnessStore.activeProvider = snap.activeProvider;
  harnessStore.activeModel = snap.activeModel;
}

export async function detectOllama(): Promise<DetectedOllama | null> {
  const detected = await invoke<DetectedOllama | null>("harness_detect_ollama");
  harnessStore.detectedOllama = detected;
  return detected;
}

export async function probeHealth(providerId: string): Promise<HealthReport> {
  const report = await invoke<HealthReport>("harness_health", { providerId });
  harnessStore.healthByProvider = { ...harnessStore.healthByProvider, [providerId]: report };
  return report;
}

export async function listModels(providerId: string): Promise<ModelInfo[]> {
  const models = await invoke<ModelInfo[]>("harness_list_models", { providerId });
  harnessStore.modelsByProvider = { ...harnessStore.modelsByProvider, [providerId]: models };
  return models;
}

/* === Built-in local model (harness_builtin_* contract) =================== */

/** Poll built-in model status. A backend without builtin support rejects the
 * invoke — we map that to null so the Settings card simply does not render. */
export async function refreshBuiltinStatus(): Promise<void> {
  try {
    harnessStore.builtinStatus = await invoke<BuiltinModelStatus>("harness_builtin_status");
  } catch {
    harnessStore.builtinStatus = null;
  }
}

/** Start (or resume) the model download. Progress streams on the
 * `builtin-model-event` channel. */
export async function downloadBuiltinModel(): Promise<void> {
  await invoke<void>("harness_builtin_download");
  await refreshBuiltinStatus();
}

export async function cancelBuiltinDownload(): Promise<void> {
  try {
    await invoke<void>("harness_builtin_cancel_download");
  } finally {
    await refreshBuiltinStatus();
  }
}

export async function deleteBuiltinModel(): Promise<void> {
  await invoke<void>("harness_builtin_delete");
  await refreshBuiltinStatus();
}

export async function startBuiltin(): Promise<void> {
  await invoke<void>("harness_builtin_start");
  await refreshBuiltinStatus();
}

export async function stopBuiltin(): Promise<void> {
  await invoke<void>("harness_builtin_stop");
  await refreshBuiltinStatus();
}

/** Fold a builtin-model-event into the cached status; on terminal phases
 * (ready/error) re-sync the authoritative state from the backend. */
function handleBuiltinEvent(ev: BuiltinModelEvent): void {
  if (ev.phase === "downloading" || ev.phase === "verifying") {
    harnessStore.builtinEventError = null;
    const cur = harnessStore.builtinStatus;
    if (cur) {
      harnessStore.builtinStatus = {
        ...cur,
        state: ev.phase,
        progressPct: ev.pct,
        bytesDone: ev.bytesDone,
        bytesTotal: ev.bytesTotal,
      };
    }
    return;
  }
  if (ev.phase === "error") {
    harnessStore.builtinEventError = ev.message || "Model download failed. Try again.";
  } else {
    harnessStore.builtinEventError = null;
  }
  void refreshBuiltinStatus();
}

export async function loadSession(sessionId: string): Promise<void> {
  harnessStore.activeSession = await invoke<HarnessSession>("harness_get_session", { sessionId });
}

export function startNewSession(): void {
  harnessStore.activeSession = null;
  harnessStore.pending = null;
  harnessStore.lastError = null;
  harnessStore.lastErrorRaw = null;
  harnessStore.lastFailedUserText = null;
  harnessStore.view = "chat";
}

export async function deleteSession(sessionId: string): Promise<void> {
  await invoke<void>("harness_delete_session", { sessionId });
  if (harnessStore.activeSession?.id === sessionId) {
    harnessStore.activeSession = null;
  }
  await refreshSessions();
}

export async function sendMessage(text: string): Promise<boolean> {
  const trimmed = text.trim();
  if (!trimmed) return false;
  if (!harnessStore.activeProvider || !harnessStore.activeModel) {
    harnessStore.lastError = "Pick a provider and model in Settings first.";
    harnessStore.lastErrorRaw = null;
    harnessStore.view = "settings";
    harnessStore.open = true;
    return false;
  }
  if (harnessStore.isStreaming) return false;
  harnessStore.lastError = null;
  harnessStore.lastErrorRaw = null;
  harnessStore.isStreaming = true;
  const sessionId = harnessStore.activeSession?.id ?? null;
  // A fresh pending turn overwrites any prior failed turn still on screen.
  harnessStore.pending = {
    id: `pending-${Date.now()}`,
    text: "",
    toolCalls: [],
    inTokens: 0,
    outTokens: 0,
    compactionEvents: 0,
    userText: trimmed,
  };
  try {
    const summary = await invoke<TurnSummary>("harness_chat_send", {
      sessionId,
      userText: trimmed,
      providerId: harnessStore.activeProvider,
      model: harnessStore.activeModel,
    });
    await loadSession(summary.sessionId);
    // Drop the pending copy the instant the persisted session is adopted, so
    // the user + assistant bubbles do not briefly render twice while
    // refreshSessions resolves (chat-duplicate-bubble-flash).
    harnessStore.pending = null;
    harnessStore.lastFailedUserText = null;
    await refreshSessions();
    return true;
  } catch (err) {
    // KEEP the pending turn on screen (marked failed) so the user's typed
    // message is never destroyed by a failed send, and remember the text so
    // Retry can resend it. Nulling pending in `finally` was the loss bug
    // (chat-send-failure). Raw error is mapped to friendly copy; the original
    // is stashed in lastErrorRaw for the pill's collapsed details line.
    const wire = asHarnessError(err);
    const raw = wire?.message ?? (err instanceof Error ? err.message : String(err));
    harnessStore.lastErrorRaw = raw;
    harnessStore.lastError = wire ? mapHarnessErrorKind(wire) : mapHarnessError(raw);
    harnessStore.lastFailedUserText = trimmed;
    if (harnessStore.pending) {
      harnessStore.pending = { ...harnessStore.pending, failed: true };
    }
    return false;
  } finally {
    harnessStore.isStreaming = false;
  }
}

/**
 * Resend the last user turn that failed. Clears the failed pending + error
 * first so the transcript doesn't briefly show the dead turn twice.
 */
export async function retryLastTurn(): Promise<void> {
  const text = harnessStore.lastFailedUserText;
  if (!text || harnessStore.isStreaming) return;
  harnessStore.pending = null;
  harnessStore.lastError = null;
  harnessStore.lastErrorRaw = null;
  await sendMessage(text);
}

/** Fill the composer with `text` (empty-state example prompts). Composer
 * adopts the value via a $effect and clears the channel. */
export function prefillComposer(text: string): void {
  harnessStore.composerPrefill = text;
}

/** Host of the active provider's endpoint, for the connection-error copy. */
function activeEndpointHost(): string | null {
  const p = harnessStore.providers.find((x) => x.id === harnessStore.activeProvider);
  const ep = p?.endpoint;
  if (!ep) return null;
  try {
    return new URL(ep).host || ep;
  } catch {
    return ep;
  }
}

/**
 * Map a raw provider/reqwest error string to friendly, honest operator copy —
 * the harness-side twin of skills.svelte.ts's errorKind mapping. The unmapped
 * string is preserved separately (lastErrorRaw) for the collapsed details line.
 */
export function mapHarnessError(raw: string): string {
  const s = (raw || "").toLowerCase();
  if (
    /(connection refused|econnrefused|failed to connect|error sending request|error trying to connect|tcp connect|could not connect|connection reset|connection closed|dns error|no route to host)/.test(
      s,
    )
  ) {
    const provider = harnessStore.providers.find((x) => x.id === harnessStore.activeProvider);
    const host = activeEndpointHost();
    const label = provider?.kind === "ollama" ? "Ollama" : provider?.label || "the model server";
    return host ? `Can't reach ${label} at ${host}. Is it running?` : `Can't reach ${label}. Is it running?`;
  }
  if (/(unauthorized|forbidden|invalid api key|invalid_api_key|authentication|\b401\b|\b403\b)/.test(s)) {
    return "Authentication failed. Check the provider API key in Settings.";
  }
  if (/(rate limit|too many requests|\b429\b)/.test(s)) {
    return "Rate limited by the provider. Wait a moment, then retry.";
  }
  if (/(timed out|timeout|deadline exceeded)/.test(s)) {
    return "The request timed out. The model may be busy; try again.";
  }
  if (/(model not found|no such model|unknown model|does not exist|deployment .* not found|\b404\b)/.test(s)) {
    return "Model not found. Probe the provider and pick an available model.";
  }
  if (/(cancelled|canceled|aborted)/.test(s)) {
    return "The turn was cancelled.";
  }
  if (
    /(n_prompt_tokens|n_ctx|context budget|context length|context window)/.test(s) &&
    /(exceed|too large|too many tokens)/.test(s)
  ) {
    return "The request exceeded this model's context budget after one reduced retry. Start a new session or choose a model with a larger context window.";
  }
  return "Something went wrong talking to the model.";
}

function asHarnessError(value: unknown): HarnessError | null {
  if (!value || typeof value !== "object") return null;
  const candidate = value as Partial<HarnessError>;
  return typeof candidate.kind === "string" && typeof candidate.message === "string"
    ? (candidate as HarnessError)
    : null;
}

function mapHarnessErrorKind(error: HarnessError): string {
  switch (error.kind) {
    case "contextBudgetExceeded":
      return "The request exceeded this model's context budget after one reduced retry. Start a new session or choose a model with a larger context window.";
    case "providerUnreachable":
    case "providerAuth":
    case "providerTimeout":
    case "modelNotFound":
    case "rateLimited":
    case "cancelled":
      return mapHarnessError(error.message);
    case "busy":
      return "This session already has a turn running. Wait for it to finish or cancel it.";
    case "toolFailed":
      return "A Skill Deck tool failed before the answer could finish. The local transcript keeps the error details.";
    case "internal":
    default:
      return mapHarnessError(error.message);
  }
}

export async function cancelCurrentTurn(): Promise<void> {
  // Prefer the real backend session id (learned from sessionStarted) so cancel
  // works on the first turn of a new chat; fall back to the active session, then
  // the optimistic pending id.
  const id =
    harnessStore.pending?.sessionId ??
    harnessStore.activeSession?.id ??
    harnessStore.pending?.id;
  if (!id) return;
  try {
    await invoke<void>("harness_cancel_turn", { sessionId: id });
  } catch (err) {
    harnessStore.lastError = err instanceof Error ? err.message : String(err);
  }
}

function handleEvent(event: HarnessEvent): void {
  // Diagnostic timing log — captures when each backend event arrives at the
  // frontend so we can confirm tool events are streaming in real-time and not
  // batched at end-of-turn. View in Webview2 devtools. Cheap to leave in.
  if (import.meta.env.DEV && typeof console !== "undefined" && event.kind !== "contentDelta") {
    // eslint-disable-next-line no-console
    console.debug(`[harness-event] ${new Date().toISOString().slice(11, 23)} ${event.kind}`,
      event.kind === "toolCallStart" || event.kind === "toolCallResult" || event.kind === "toolCallError"
        ? `${(event as { name?: string }).name ?? ""}`
        : "");
  }
  switch (event.kind) {
    case "sessionStarted":
      // Record the backend session id on the pending turn so cancel can target
      // it immediately (chat-cancel-newsession).
      if (harnessStore.pending && !harnessStore.pending.sessionId) {
        harnessStore.pending = { ...harnessStore.pending, sessionId: event.sessionId };
      }
      if (harnessStore.activeSession && !harnessStore.activeSession.id) {
        harnessStore.activeSession = { ...harnessStore.activeSession, id: event.sessionId };
      }
      return;
    case "contentDelta":
      if (!harnessStore.pending) return;
      harnessStore.pending = { ...harnessStore.pending, text: harnessStore.pending.text + event.text };
      return;
    case "toolCallStart":
      if (!harnessStore.pending) return;
      harnessStore.pending = {
        ...harnessStore.pending,
        toolCalls: [
          ...harnessStore.pending.toolCalls,
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
      if (!harnessStore.pending) return;
      harnessStore.pending = {
        ...harnessStore.pending,
        toolCalls: harnessStore.pending.toolCalls.map((c) =>
          c.callId === event.callId ? { ...c, status: "success", result: event.result } : c,
        ),
      };
      return;
    case "toolCallError":
      if (!harnessStore.pending) return;
      harnessStore.pending = {
        ...harnessStore.pending,
        toolCalls: harnessStore.pending.toolCalls.map((c) =>
          c.callId === event.callId
            ? { ...c, status: "error", errorMessage: event.message }
            : c,
        ),
      };
      return;
    case "toolCallVetoed":
      if (!harnessStore.pending) return;
      harnessStore.pending = {
        ...harnessStore.pending,
        toolCalls: harnessStore.pending.toolCalls.map((c) =>
          c.callId === event.callId ? { ...c, status: "vetoed", errorMessage: event.reason } : c,
        ),
      };
      return;
    case "compaction":
      if (!harnessStore.pending) return;
      harnessStore.pending = {
        ...harnessStore.pending,
        compactionEvents: harnessStore.pending.compactionEvents + 1,
      };
      return;
    case "postModel":
      if (!harnessStore.pending) return;
      harnessStore.pending = {
        ...harnessStore.pending,
        inTokens: event.inTokens,
        outTokens: event.outTokens,
      };
      return;
    case "finalEnvelope":
      harnessStore.envelopeBySession = {
        ...harnessStore.envelopeBySession,
        [event.sessionId]: {
          suggestedNextPrompts: event.suggestedNextPrompts,
          sources: event.sources,
          skillsReferenced: event.skillsReferenced,
        },
      };
      return;
    case "error":
      harnessStore.lastErrorRaw = event.message;
      harnessStore.lastError = mapHarnessErrorKind({
        kind: event.errorKind,
        message: event.message,
      });
      return;
    default:
      return;
  }
}

export function currentEnvelope(): SessionEnvelope | null {
  const id = harnessStore.activeSession?.id;
  if (!id) return null;
  return harnessStore.envelopeBySession[id] ?? null;
}

export function getToolCardExpanded(key: string, fallback: boolean): boolean {
  const v = harnessStore.toolCardExpansion[key];
  return v === undefined ? fallback : v;
}

export function setToolCardExpanded(key: string, expanded: boolean): void {
  harnessStore.toolCardExpansion = { ...harnessStore.toolCardExpansion, [key]: expanded };
}
