<!--
  @agent-context: Harness settings console. The built-in local model renders
  as the first provider-card in the providers panel, driven by
  the harness_builtin_* IPC contract + the `builtin-model-event` progress
  channel (card hides itself when the backend lacks builtin support). Provider
  setup + active model routing stay the always-visible first-run surface;
  harness tuning + web-search keys live behind an "Advanced" disclosure
  (default collapsed, persisted in localStorage). All mutation flows through
  the existing harness IPC helpers. When settings have not loaded yet, the
  tuning body self-heals (one refreshSettings() call) and shows a live Spinner
  instead of a dead "reopen the panel" message.
-->
<script lang="ts">
  import {
    cancelBuiltinDownload,
    deleteBuiltinModel,
    deleteProvider,
    detectOllama,
    downloadBuiltinModel,
    harnessStore,
    listModels,
    probeHealth,
    refreshBuiltinStatus,
    refreshSettings,
    saveProvider,
    setActiveSelection,
  } from "$lib/stores/harness.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { HarnessSettings, ProviderConfig } from "$lib/types/harness";
  import Spinner from "../ui/Spinner.svelte";

  let draft = $state<ProviderConfig>({
    id: "",
    kind: "ollama",
    label: "Ollama (local)",
    endpoint: "http://127.0.0.1:11434",
    apiKey: null,
    deployment: null,
    apiVersion: null,
    defaultModel: null,
    notes: null,
  });
  let saving = $state(false);
  let probing = $state<string | null>(null);

  // Advanced disclosure — collapsed by default, persisted so power users don't
  // re-open it every session.
  const ADVANCED_KEY = "skilldeck.settingsAdvanced";
  let advancedOpen = $state(readAdvanced());
  function readAdvanced(): boolean {
    try {
      return localStorage.getItem(ADVANCED_KEY) === "1";
    } catch {
      return false;
    }
  }
  function toggleAdvanced() {
    advancedOpen = !advancedOpen;
    try {
      localStorage.setItem(ADVANCED_KEY, advancedOpen ? "1" : "0");
    } catch {
      // Non-fatal: disclosure still works this session, just not persisted.
    }
  }

  // Self-heal: if settings have not arrived yet, request them once so the
  // Advanced body resolves on its own instead of dead-ending.
  let triedLoad = $state(false);
  $effect(() => {
    if (!harnessStore.settings && !triedLoad) {
      triedLoad = true;
      void refreshSettings();
    }
  });

  // Re-fetch built-in model status once per mount.
  let triedBuiltinStatus = $state(false);
  $effect(() => {
    if (!triedBuiltinStatus) {
      triedBuiltinStatus = true;
      void refreshBuiltinStatus();
    }
  });

  const activeProvider = $derived.by(() =>
    harnessStore.providers.find((provider) => provider.id === harnessStore.activeProvider) ?? null,
  );

  /** Harness command errors serialize as { kind, message }. */
  function wireError(err: unknown): { kind: string; message: string } {
    if (err && typeof err === "object" && "kind" in err) {
      const e = err as { kind?: unknown; message?: unknown };
      return {
        kind: String(e.kind),
        message: typeof e.message === "string" ? e.message : "",
      };
    }
    return { kind: "internal", message: err instanceof Error ? err.message : String(err) };
  }

  /* === Built-in model card state ========================================= */
  let builtinBusy = $state(false);
  let builtinError = $state<string | null>(null);

  const builtin = $derived(harnessStore.builtinStatus);
  const builtinProvider = $derived(
    harnessStore.providers.find((p) => p.kind === "builtin") ?? null,
  );
  const builtinActive = $derived(
    Boolean(builtinProvider && harnessStore.activeProvider === builtinProvider.id) ||
      harnessStore.activeProvider === "builtin",
  );
  /** Providers list minus the synthetic builtin entry (it has its own card). */
  const visibleProviders = $derived(harnessStore.providers.filter((p) => p.kind !== "builtin"));
  const builtinBusyState = $derived(
    builtin?.state === "downloading" || builtin?.state === "verifying",
  );
  const builtinPct = $derived.by(() => {
    const s = harnessStore.builtinStatus;
    if (!s) return 0;
    if (typeof s.progressPct === "number") return Math.min(100, Math.max(0, s.progressPct));
    if (s.bytesTotal && s.bytesDone) {
      return Math.min(100, Math.max(0, (s.bytesDone / s.bytesTotal) * 100));
    }
    return 0;
  });

  function humanGb(bytes: number | null | undefined): string {
    if (!bytes) return "2.5 GB";
    return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
  }

  async function doBuiltinDownload() {
    builtinBusy = true;
    builtinError = null;
    try {
      await downloadBuiltinModel();
    } catch (err) {
      builtinError = wireError(err).message || "Download failed. Try again.";
    } finally {
      builtinBusy = false;
    }
  }

  async function doBuiltinCancel() {
    builtinBusy = true;
    try {
      await cancelBuiltinDownload();
    } catch (err) {
      builtinError = wireError(err).message || "Couldn't cancel the download.";
    } finally {
      builtinBusy = false;
    }
  }

  async function doBuiltinDelete() {
    builtinBusy = true;
    builtinError = null;
    try {
      await deleteBuiltinModel();
    } catch (err) {
      builtinError = wireError(err).message || "Couldn't delete the model.";
    } finally {
      builtinBusy = false;
    }
  }

  async function doBuiltinUse() {
    await setActiveSelection(
      builtinProvider?.id ?? "builtin",
      builtinProvider?.defaultModel ?? builtin?.modelName ?? null,
    );
  }

  async function doDetectOllama() {
    saving = true;
    try {
      const detected = await detectOllama();
      if (detected) {
        const id = `ollama-${Date.now()}`;
        const defaultModel: string | null = detected.models[0]?.id ?? null;
        const config: ProviderConfig = {
          id,
          kind: "ollama",
          label: "Ollama (auto-detected)",
          endpoint: detected.endpoint,
          apiKey: null,
          deployment: null,
          apiVersion: null,
          defaultModel,
          notes: null,
        };
        await saveProvider(config);
        if (!harnessStore.activeProvider) {
          await setActiveSelection(id, defaultModel);
        }
      }
    } finally {
      saving = false;
    }
  }

  async function doSaveDraft() {
    saving = true;
    try {
      const id = draft.id || `${draft.kind}-${Date.now()}`;
      const cfg = { ...draft, id };
      await saveProvider(cfg);
      draft = { ...draft, id: "" };
    } finally {
      saving = false;
    }
  }

  async function doProbe(p: ProviderConfig) {
    probing = p.id;
    try {
      await probeHealth(p.id);
      await listModels(p.id);
    } finally {
      probing = null;
    }
  }

  async function doSetActive(providerId: string, model: string | null) {
    await setActiveSelection(providerId, model);
  }

  async function doDelete(p: ProviderConfig) {
    await deleteProvider(p.id);
  }

  function setKind(kind: string) {
    draft = {
      ...draft,
      kind,
      label: kind === "azure" ? "Azure OpenAI" : "Ollama (local)",
      endpoint: kind === "azure" ? "https://your-resource.openai.azure.com/" : "http://127.0.0.1:11434",
    };
  }

  async function updateSettings(patch: Partial<HarnessSettings>) {
    if (!harnessStore.settings) return;
    const merged = { ...harnessStore.settings, ...patch };
    await invoke<HarnessSettings>("harness_set_settings", { settings: merged });
    await refreshSettings();
  }

  function providerModelValue(p: ProviderConfig): string {
    if (harnessStore.activeProvider === p.id) return harnessStore.activeModel ?? "";
    return p.defaultModel ?? "";
  }

  function providerHealthLabel(p: ProviderConfig): string {
    return harnessStore.healthByProvider[p.id]?.status ?? "Not checked";
  }
</script>

<div class="settings-pane selectable">
  <header class="settings-head">
    <div class="head-copy">
      <span class="kicker">Agent harness</span>
      <h3>Provider &amp; model</h3>
      <p>Route the agent to a model, probe it, then set the active route.</p>
    </div>
    <div class="active-card" class:ready={Boolean(activeProvider)}>
      <span class="active-label">Active route</span>
      <strong>{activeProvider?.label ?? "No provider selected"}</strong>
      <small>{harnessStore.activeModel ?? "Pick a model before sending agent turns"}</small>
    </div>
  </header>

  <section class="settings-grid">
    <div class="panel providers-panel">
      <div class="panel-head">
        <div>
          <span class="section-kicker">Providers</span>
          <h4>Model routes</h4>
          <p>Probe a provider to load models, then select the route the agent should use.</p>
        </div>
        <button class="btn-secondary btn-sm" onclick={doDetectOllama} disabled={saving}>
          {saving ? "Detecting…" : "Auto-detect Ollama"}
        </button>
      </div>

      {#if !builtin && visibleProviders.length === 0}
        <div class="empty-state">
          <strong>No providers configured.</strong>
          <span>Start local with Ollama, or add Azure/OpenAI below for a hosted model.</span>
        </div>
      {:else}
        <div class="provider-list">
          {#if builtin}
            <!-- Managed local model — first route. Driven by harness_builtin_status +
                 the builtin-model-event progress channel. -->
            <article class="provider-card builtin-card" class:active={builtinActive}>
              <div class="provider-main">
                <div class="provider-mark">LM</div>
                <div class="provider-copy">
                  <div class="provider-title">
                    <strong>{builtin.modelName}</strong>
                    <span class="kind-pill">managed local</span>
                    {#if builtinActive}
                      <span class="active-pill">active</span>
                    {/if}
                    {#if builtin.state === "ready" || builtin.state === "running"}
                      <span class="badge-signal is-ok">{builtin.state === "running" ? "running" : "ready"}</span>
                    {:else if builtinBusyState}
                      <span class="badge-signal">{builtin.state === "verifying" ? "verifying" : "downloading"}</span>
                    {:else}
                      <span class="badge-meta">not downloaded</span>
                    {/if}
                  </div>
                  <div class="endpoint">Windows x64 · included with Skill Deck</div>
                  <div class="endpoint">2.5 GB one-time online download · SHA-256 verified · local thereafter</div>
                  <div class="endpoint">llama.cpp (MIT) · Jan-nano (Apache-2.0) · THIRD_PARTY_NOTICES included</div>
                  {#if builtinBusyState}
                    <div
                      class="dl-track"
                      role="progressbar"
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-valuenow={Math.round(builtinPct)}
                      aria-label="Model download progress"
                    >
                      <div class="dl-fill" style="width: {builtinPct}%"></div>
                    </div>
                    <div class="dl-meta">
                      <span>{Math.round(builtinPct)}%</span>
                      {#if builtin.bytesTotal}
                        <span>{humanGb(builtin.bytesDone ?? 0)} / {humanGb(builtin.bytesTotal)}</span>
                      {/if}
                    </div>
                  {:else if builtin.state === "notDownloaded"}
                    <div class="health-row">
                      <span>Download required before first local use</span>
                    </div>
                  {:else if builtin.path}
                    <div class="endpoint" title={builtin.path}>{builtin.path}</div>
                  {/if}
                </div>
              </div>

              <div class="provider-controls">
                <div class="card-actions">
                  {#if builtin.state === "notDownloaded"}
                    <button class="btn-primary btn-sm" onclick={doBuiltinDownload} disabled={builtinBusy}>
                      {builtinBusy ? "Starting…" : "Download"}
                    </button>
                  {:else if builtinBusyState}
                    <button class="btn-secondary btn-sm" onclick={doBuiltinCancel} disabled={builtinBusy}>Cancel</button>
                  {:else}
                    <button class="btn-secondary btn-sm" onclick={doBuiltinUse} disabled={builtinActive}>Use</button>
                    <button class="btn-icon del" onclick={doBuiltinDelete} aria-label="Delete downloaded Jan-nano model" disabled={builtinBusy}>
                      <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                        <path
                          d="M3 4.5h10M6.5 4.5V3.6a1 1 0 0 1 1-1h1a1 1 0 0 1 1 1v.9M4.8 4.5l.45 8a1 1 0 0 0 1 .95h3.5a1 1 0 0 0 1-.95l.45-8"
                          stroke="currentColor"
                          stroke-width="1.3"
                          stroke-linecap="round"
                          stroke-linejoin="round"
                        />
                      </svg>
                    </button>
                  {/if}
                </div>
                {#if builtinError || harnessStore.builtinEventError}
                  <p class="model-error" role="alert">{builtinError ?? harnessStore.builtinEventError}</p>
                {/if}
              </div>
            </article>
          {/if}
          {#each visibleProviders as p (p.id)}
            <article class="provider-card" class:active={harnessStore.activeProvider === p.id}>
              <div class="provider-main">
                <div class="provider-mark">{p.kind.slice(0, 2).toUpperCase()}</div>
                <div class="provider-copy">
                  <div class="provider-title">
                    <strong>{p.label || p.id}</strong>
                    <span class="kind-pill">{p.kind}</span>
                    {#if harnessStore.activeProvider === p.id}
                      <span class="active-pill">active</span>
                    {/if}
                  </div>
                  <div class="endpoint" title={p.endpoint ?? "No endpoint"}>{p.endpoint ?? "No endpoint configured"}</div>
                  <div class="health-row">
                    <span class:ok={harnessStore.healthByProvider[p.id]?.ok} class:bad={harnessStore.healthByProvider[p.id]?.ok === false}>
                      {providerHealthLabel(p)}
                    </span>
                    {#if probing === p.id}
                      <span class="dim"><Spinner size={11} weight={2} /> probing…</span>
                    {/if}
                  </div>
                </div>
              </div>

              <div class="provider-controls">
                <label class="control-label">Model
                  {#if harnessStore.modelsByProvider[p.id]}
                    <select
                      value={providerModelValue(p)}
                      onchange={(e) => doSetActive(p.id, (e.target as HTMLSelectElement).value)}
                    >
                      {#each harnessStore.modelsByProvider[p.id] as m (m.id)}
                        <option value={m.id}>{m.label}</option>
                      {/each}
                    </select>
                  {:else}
                    <div class="model-placeholder">Probe to load model list</div>
                  {/if}
                </label>
                <div class="card-actions">
                  <button class="btn-secondary btn-sm" onclick={() => doProbe(p)} disabled={probing === p.id}>Probe</button>
                  <button class="btn-secondary btn-sm" onclick={() => doSetActive(p.id, harnessStore.activeModel ?? p.defaultModel ?? null)}>Use</button>
                  <button class="btn-icon del" onclick={() => doDelete(p)} aria-label="Delete provider">
                    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" aria-hidden="true">
                      <path
                        d="M3 4.5h10M6.5 4.5V3.6a1 1 0 0 1 1-1h1a1 1 0 0 1 1 1v.9M4.8 4.5l.45 8a1 1 0 0 0 1 .95h3.5a1 1 0 0 0 1-.95l.45-8"
                        stroke="currentColor"
                        stroke-width="1.3"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                      />
                    </svg>
                  </button>
                </div>
              </div>
            </article>
          {/each}
        </div>
      {/if}
    </div>

    <div class="panel add-panel">
      <div class="panel-head stacked">
        <span class="section-kicker">Add provider</span>
        <h4>Connect the next brain</h4>
        <p>Local Ollama for privacy, Azure/OpenAI when you need frontier model quality.</p>
      </div>

      <div class="segmented kind-switch" role="group" aria-label="Provider kind">
        <button class="seg" class:active={draft.kind === "ollama"} onclick={() => setKind("ollama")}>Ollama</button>
        <button class="seg" class:active={draft.kind === "azure"} onclick={() => setKind("azure")}>Azure</button>
      </div>

      <div class="form-grid single">
        <label>Label<input bind:value={draft.label} /></label>
        <label>Endpoint<input bind:value={draft.endpoint} /></label>
        {#if draft.kind === "azure"}
          <label>API key<input bind:value={draft.apiKey} type="password" autocomplete="off" /></label>
          <label>Deployment<input bind:value={draft.deployment} /></label>
          <label>API version<input bind:value={draft.apiVersion} placeholder="2024-08-01-preview" /></label>
        {/if}
        <label>Default model<input bind:value={draft.defaultModel} placeholder={draft.kind === "azure" ? "gpt-5.5" : "hermes3:8b"} /></label>
      </div>

      <button class="btn-primary wide" onclick={doSaveDraft} disabled={saving}>
        {saving ? "Saving…" : "Save provider"}
      </button>
    </div>
  </section>

  <section class="advanced">
    <button class="disclosure" aria-expanded={advancedOpen} onclick={toggleAdvanced}>
      <span class="chevron" class:open={advancedOpen} aria-hidden="true">
        <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
          <path d="M6 4l4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </span>
      <span class="disclosure-label">Advanced</span>
      <span class="disclosure-hint">harness tuning · web search</span>
    </button>

    {#if advancedOpen}
      <div class="advanced-body">
        {#if harnessStore.settings}
          <div class="tuning-grid">
            <label>Memory compaction floor
              <span>How full context gets before old turns compact.</span>
              <input
                type="number" step="0.05" min="0" max="1"
                value={harnessStore.settings.memoryCompactionFloor}
                onchange={(e) => updateSettings({ memoryCompactionFloor: parseFloat((e.target as HTMLInputElement).value) || 0.7 })}
              />
            </label>
            <label>Compaction privacy
              <span>Local keeps transcript compaction on this device. Selected provider never falls back to another provider.</span>
              <select
                value={harnessStore.settings.localCompactionOnly ? "local" : "selected"}
                onchange={(e) => updateSettings({
                  localCompactionOnly: (e.target as HTMLSelectElement).value === "local",
                })}
              >
                <option value="local">Local only (recommended)</option>
                <option value="selected">Selected provider</option>
              </select>
            </label>
            <label>Default item count
              <span>Used when users ask for skills but do not state a count.</span>
              <input
                type="number" min="1" max="50"
                value={harnessStore.settings.itemCountDefault}
                onchange={(e) => updateSettings({ itemCountDefault: parseInt((e.target as HTMLInputElement).value) || 3 })}
              />
            </label>
            <label>Max turns
              <span>How many ReAct tool iterations one answer may take.</span>
              <input
                type="number" min="1" max="40"
                value={harnessStore.settings.maxTurns}
                onchange={(e) => updateSettings({ maxTurns: parseInt((e.target as HTMLInputElement).value) || 12 })}
              />
            </label>
            <label>Max tokens
              <span>Output cap per model call.</span>
              <input
                type="number" min="64" max="32000"
                value={harnessStore.settings.maxTokens}
                onchange={(e) => updateSettings({ maxTokens: parseInt((e.target as HTMLInputElement).value) || 2048 })}
              />
            </label>
            <label>Web search provider
              <span>DuckDuckGo is keyless; Brave can use your API key.</span>
              <select
                value={harnessStore.settings.webSearchProvider}
                onchange={(e) => updateSettings({ webSearchProvider: (e.target as HTMLSelectElement).value })}
              >
                <option value="duckduckgo">duckduckgo</option>
                <option value="brave">brave</option>
              </select>
            </label>
            <label>Web search API key
              <span>Optional. Only needed for providers that require a key.</span>
              <input
                type="password"
                value={harnessStore.settings.webSearchApiKey ?? ""}
                autocomplete="off"
                onchange={(e) => updateSettings({ webSearchApiKey: (e.target as HTMLInputElement).value || null })}
              />
            </label>
          </div>
        {:else}
          <div class="loading">
            <Spinner size={16} weight={2} />
            <span>Loading harness settings…</span>
          </div>
        {/if}
      </div>
    {/if}
  </section>
</div>

<style>
  .settings-pane {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    color: var(--color-text-primary);
    font-family: var(--font-ui);
    background: var(--color-surface-0);
  }

  .settings-head {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(220px, 0.34fr);
    gap: 16px;
    align-items: stretch;
  }

  .model-error {
    margin: 0;
    color: var(--color-error);
    font-size: var(--text-xs);
    line-height: var(--text-xs--line-height);
  }

  /* === Built-in model card extras ======================================= */
  .dl-track {
    height: 4px;
    border-radius: var(--radius-xs);
    background: var(--color-surface-2);
    overflow: hidden;
  }

  .dl-fill {
    height: 100%;
    background: var(--color-accent);
    transition: width var(--motion-base) var(--ease-standard);
  }

  .dl-meta {
    display: flex;
    gap: 8px;
    color: var(--color-text-muted);
    font-size: var(--text-2xs);
    line-height: var(--text-2xs--line-height);
    font-variant-numeric: tabular-nums;
  }

  .kicker,
  .section-kicker,
  .active-label {
    color: var(--color-accent-text);
    font-family: var(--font-ui);
    font-size: var(--text-2xs);
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  h3,
  h4,
  p {
    margin: 0;
  }

  h3 {
    margin-top: 6px;
    font-family: var(--font-display);
    font-size: var(--text-lg);
    line-height: var(--text-lg--line-height);
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  h4 {
    font-family: var(--font-display);
    font-size: var(--text-base);
    line-height: var(--text-base--line-height);
    font-weight: 600;
  }

  p {
    margin-top: 6px;
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    line-height: var(--text-sm--line-height);
  }

  .active-card {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 5px;
    padding: 12px 14px;
    border-radius: var(--radius-md);
    border: 1px solid var(--color-border);
    background: var(--color-surface-2);
  }

  .active-card.ready {
    border-color: var(--color-border-active);
    background: var(--color-accent-subtle);
  }

  .active-card strong {
    font-size: var(--text-base);
    line-height: var(--text-base--line-height);
    font-weight: 600;
  }

  .active-card small,
  .endpoint,
  .health-row,
  .model-placeholder,
  label span,
  .empty-state span {
    color: var(--color-text-muted);
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    line-height: var(--text-xs--line-height);
  }

  .settings-grid {
    display: grid;
    grid-template-columns: minmax(0, 1.35fr) minmax(280px, 0.65fr);
    gap: 12px;
  }

  .panel {
    padding: 14px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-surface-1);
  }

  .panel-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
  }

  .panel-head.stacked {
    display: block;
  }

  .panel-head h4 {
    margin-top: 4px;
  }

  .provider-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .provider-card {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(230px, 0.42fr);
    gap: 12px;
    padding: 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-0);
    transition-property: border-color, box-shadow;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }

  .provider-card.active {
    border-color: var(--color-border-active);
    box-shadow: 0 0 0 3px var(--color-accent-subtle);
  }

  .provider-main {
    display: flex;
    gap: 11px;
    min-width: 0;
  }

  .provider-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    flex: 0 0 36px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-2);
    color: var(--color-accent-text);
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    font-weight: 700;
  }

  .provider-copy {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
  }

  .provider-title {
    display: flex;
    align-items: center;
    gap: 7px;
    flex-wrap: wrap;
  }

  .provider-title strong {
    font-size: var(--text-sm);
    line-height: var(--text-sm--line-height);
    font-weight: 600;
  }

  .kind-pill,
  .active-pill {
    display: inline-flex;
    align-items: center;
    height: 16px;
    padding: 0 6px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-xs);
    background: var(--color-surface-2);
    color: var(--color-text-muted);
    font-family: var(--font-ui);
    font-size: var(--text-2xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .active-pill {
    color: var(--color-accent-text);
    background: var(--color-accent-subtle);
    border-color: var(--color-border-active);
  }

  .endpoint {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .health-row {
    display: flex;
    gap: 9px;
    align-items: center;
    flex-wrap: wrap;
  }

  .health-row .ok {
    color: var(--color-ok);
  }

  .health-row .bad {
    color: var(--color-error);
  }

  .health-row .dim {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--color-text-muted);
  }

  .provider-controls {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .control-label {
    gap: 5px;
  }

  .card-actions {
    display: flex;
    gap: 7px;
    justify-content: flex-end;
    align-items: center;
    flex-wrap: wrap;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 16px;
    border: 1px dashed var(--color-border-hover);
    border-radius: var(--radius-md);
    background: var(--color-surface-0);
  }

  .empty-state strong {
    font-size: var(--text-sm);
    font-weight: 600;
  }

  /* Segmented control (DESIGN 8) — radius-sm track, concentric radius-xs
   * segments, phosphor active (surface-3 + accent underline). Not a pill. */
  .segmented {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-2);
  }

  .kind-switch {
    display: grid;
    grid-template-columns: 1fr 1fr;
    margin: 12px 0;
  }

  .seg {
    appearance: none;
    border: none;
    background: transparent;
    color: var(--color-text-muted);
    height: 26px;
    padding: 0 10px;
    border-radius: var(--radius-xs);
    font-family: var(--font-ui);
    font-size: var(--text-sm);
    font-weight: 500;
    line-height: 1;
    cursor: pointer;
    transition-property: background-color, color, box-shadow, scale;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }

  .seg:hover {
    color: var(--color-text-secondary);
  }

  .seg.active {
    background: var(--color-surface-3);
    color: var(--color-text-primary);
    box-shadow: inset 0 -2px 0 var(--color-accent);
  }

  .seg:active {
    scale: 0.96;
  }

  .form-grid,
  .tuning-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }

  .form-grid.single {
    grid-template-columns: 1fr;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 5px;
    color: var(--color-text-secondary);
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    line-height: var(--text-xs--line-height);
    font-weight: 500;
  }

  /* Field spec (DESIGN 8). */
  input,
  select {
    width: 100%;
    box-sizing: border-box;
    height: 28px;
    padding: 0 8px;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    color: var(--color-text-primary);
    border-radius: var(--radius-sm);
    font-family: var(--font-ui);
    font-size: var(--text-sm);
    outline: none;
    transition-property: border-color, box-shadow, background-color;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }

  input:hover,
  select:hover {
    border-color: var(--color-border-hover);
  }

  input:focus,
  select:focus {
    border-color: var(--color-accent);
    box-shadow: 0 0 0 3px var(--color-accent-subtle);
  }

  input::placeholder {
    color: var(--color-text-muted);
  }

  .model-placeholder {
    display: flex;
    align-items: center;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-1);
  }

  .del:hover {
    color: var(--color-error);
    background: var(--color-error-subtle);
  }

  .btn-primary.wide {
    width: 100%;
    margin-top: 12px;
  }

  /* === Advanced disclosure ============================================== */
  .advanced {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .disclosure {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 9px 12px;
    background: var(--color-surface-1);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color: var(--color-text-secondary);
    cursor: pointer;
    transition-property: background-color, border-color, color;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }

  .disclosure:hover {
    background: var(--color-surface-2);
    border-color: var(--color-border-hover);
    color: var(--color-text-primary);
  }

  .chevron {
    display: inline-flex;
    color: var(--color-text-muted);
    transition: transform var(--motion-base) var(--ease-standard);
  }

  .chevron.open {
    transform: rotate(90deg);
  }

  .disclosure-label {
    font-family: var(--font-ui);
    font-size: var(--text-sm);
    font-weight: 600;
  }

  .disclosure-hint {
    color: var(--color-text-muted);
    font-size: var(--text-2xs);
  }

  .advanced-body {
    animation: u-fade-in var(--motion-base) var(--ease-standard);
  }

  .tuning-grid label {
    padding: 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-1);
  }

  .loading {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 20px;
    justify-content: center;
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-surface-1);
  }

  @media (max-width: 860px) {
    .settings-head,
    .settings-grid,
    .provider-card,
    .tuning-grid,
    .form-grid {
      grid-template-columns: 1fr;
    }

    .panel-head {
      flex-direction: column;
      align-items: flex-start;
    }

    .card-actions {
      justify-content: flex-start;
    }
  }
</style>
