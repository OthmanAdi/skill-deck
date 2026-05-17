<!--
  @agent-context: Provider configuration panel.
  Sections:
    - Ollama: detect button → seeds local provider + model list.
    - Azure Foundry: form (endpoint, key, deployment, api version).
    - Active selection: provider + model picker.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import {
    aiStore,
    detectOllama,
    deleteProvider,
    listModels,
    probeHealth,
    refreshRegistry,
    saveProvider,
    seedOllamaProvider,
    setActiveSelection,
  } from "$lib/stores/ai.svelte";
  import type { AiProviderConfig } from "$lib/types/ai";

  let detectInFlight = $state(false);
  let modelLoadInFlight = $state<Record<string, boolean>>({});
  let healthInFlight = $state<Record<string, boolean>>({});

  // Azure form state — bound directly so the user sees their edits.
  let azureDraft = $state<AiProviderConfig>({
    id: "",
    kind: "azure-foundry",
    label: "Azure Foundry",
    endpoint: "",
    apiKey: "",
    deployment: "",
    apiVersion: "",
    defaultModel: "",
    notes: "",
  });

  onMount(() => {
    void refreshRegistry();
  });

  $effect(() => {
    // Hydrate the Azure draft from the saved provider if one exists.
    const existing = aiStore.providers.find((p) => p.kind === "azure-foundry");
    if (existing && azureDraft.id !== existing.id) {
      azureDraft = { ...existing };
    }
  });

  async function runOllamaDetect() {
    detectInFlight = true;
    try {
      const detected = await detectOllama();
      if (detected) {
        await seedOllamaProvider(detected);
      }
    } finally {
      detectInFlight = false;
    }
  }

  async function saveAzure() {
    if (!azureDraft.id) {
      azureDraft.id = `azure-foundry-${Date.now()}`;
    }
    await saveProvider({ ...azureDraft });
    if (!aiStore.activeProvider) {
      await setActiveSelection(azureDraft.id, azureDraft.defaultModel ?? null);
    }
  }

  async function loadModelsFor(providerId: string) {
    modelLoadInFlight = { ...modelLoadInFlight, [providerId]: true };
    try {
      await listModels(providerId);
    } finally {
      modelLoadInFlight = { ...modelLoadInFlight, [providerId]: false };
    }
  }

  async function probe(providerId: string) {
    healthInFlight = { ...healthInFlight, [providerId]: true };
    try {
      await probeHealth(providerId);
    } finally {
      healthInFlight = { ...healthInFlight, [providerId]: false };
    }
  }

  async function selectProvider(id: string) {
    const models = aiStore.modelsByProvider[id];
    const firstModel = models?.[0]?.id ?? null;
    await setActiveSelection(id, aiStore.activeModel ?? firstModel);
  }

  async function selectModel(modelId: string) {
    if (!aiStore.activeProvider) return;
    await setActiveSelection(aiStore.activeProvider, modelId);
  }

  async function dropProvider(id: string) {
    await deleteProvider(id);
  }

  const activeModels = $derived(
    aiStore.activeProvider ? aiStore.modelsByProvider[aiStore.activeProvider] ?? [] : []
  );
</script>

<div class="settings space-y-3">
  <!-- Ollama -->
  <section class="provider-card">
    <header class="provider-header">
      <div>
        <h3>Ollama (on-device)</h3>
        <p>Run any local model. Defaults to <code>http://127.0.0.1:11434</code>.</p>
      </div>
      <button class="ghost-btn" onclick={runOllamaDetect} disabled={detectInFlight}>
        {detectInFlight ? "Detecting…" : "Detect"}
      </button>
    </header>

    {#if aiStore.detectedOllama}
      <div class="detected">
        Reachable at <code>{aiStore.detectedOllama.endpoint}</code> —
        {aiStore.detectedOllama.models.length} model(s) advertised.
      </div>
    {/if}

    {#each aiStore.providers.filter((p) => p.kind === "ollama") as provider (provider.id)}
      <div class="provider-row">
        <label>
          <span>Endpoint</span>
          <input
            type="text"
            value={provider.endpoint ?? ""}
            placeholder="http://127.0.0.1:11434"
            oninput={(e) => {
              const v = (e.currentTarget as HTMLInputElement).value;
              void saveProvider({ ...provider, endpoint: v });
            }}
          />
        </label>
        <label>
          <span>Default model</span>
          <input
            type="text"
            value={provider.defaultModel ?? ""}
            placeholder="llama3.1:8b"
            oninput={(e) => {
              const v = (e.currentTarget as HTMLInputElement).value;
              void saveProvider({ ...provider, defaultModel: v });
            }}
          />
        </label>
        <div class="row-actions">
          <button class="ghost-btn" onclick={() => loadModelsFor(provider.id)} disabled={modelLoadInFlight[provider.id]}>
            {modelLoadInFlight[provider.id] ? "…" : "Refresh models"}
          </button>
          <button class="ghost-btn" onclick={() => probe(provider.id)} disabled={healthInFlight[provider.id]}>
            {healthInFlight[provider.id] ? "…" : "Test"}
          </button>
          <button class="ghost-btn danger" onclick={() => dropProvider(provider.id)}>Remove</button>
        </div>
        {#if aiStore.healthByProvider[provider.id]}
          <div class="health {aiStore.healthByProvider[provider.id]!.ok ? 'ok' : 'fail'}">
            {aiStore.healthByProvider[provider.id]!.ok ? "✓" : "✕"}
            {aiStore.healthByProvider[provider.id]!.detail ?? ""}
          </div>
        {/if}
      </div>
    {/each}
  </section>

  <!-- Azure Foundry -->
  <section class="provider-card">
    <header class="provider-header">
      <div>
        <h3>Azure AI Foundry</h3>
        <p>Endpoint + API key. Calls <code>/openai/v1/chat/completions</code>.</p>
      </div>
      <button class="ghost-btn primary" onclick={saveAzure}>Save</button>
    </header>

    <div class="provider-row">
      <label>
        <span>Endpoint</span>
        <input
          type="text"
          bind:value={azureDraft.endpoint}
          placeholder="https://your-resource.openai.azure.com"
        />
      </label>
      <label>
        <span>API key</span>
        <input
          type="password"
          bind:value={azureDraft.apiKey}
          placeholder="sk-…"
        />
      </label>
      <label>
        <span>Deployment / default model</span>
        <input
          type="text"
          bind:value={azureDraft.defaultModel}
          placeholder="gpt-4o-mini"
        />
      </label>
      <label>
        <span>API version (optional)</span>
        <input
          type="text"
          bind:value={azureDraft.apiVersion}
          placeholder="2024-10-21"
        />
      </label>
      {#if azureDraft.id}
        <div class="row-actions">
          <button class="ghost-btn" onclick={() => loadModelsFor(azureDraft.id)} disabled={modelLoadInFlight[azureDraft.id]}>
            {modelLoadInFlight[azureDraft.id] ? "…" : "Refresh models"}
          </button>
          <button class="ghost-btn" onclick={() => probe(azureDraft.id)} disabled={healthInFlight[azureDraft.id]}>
            {healthInFlight[azureDraft.id] ? "…" : "Test"}
          </button>
          <button class="ghost-btn danger" onclick={() => dropProvider(azureDraft.id)}>Remove</button>
        </div>
        {#if aiStore.healthByProvider[azureDraft.id]}
          <div class="health {aiStore.healthByProvider[azureDraft.id]!.ok ? 'ok' : 'fail'}">
            {aiStore.healthByProvider[azureDraft.id]!.ok ? "✓" : "✕"}
            {aiStore.healthByProvider[azureDraft.id]!.detail ?? ""}
          </div>
        {/if}
      {/if}
    </div>
  </section>

  <!-- Active selection -->
  <section class="provider-card">
    <header class="provider-header">
      <div>
        <h3>Active selection</h3>
        <p>The agent uses this provider + model for every turn.</p>
      </div>
    </header>

    <div class="provider-row">
      <label>
        <span>Provider</span>
        <select
          value={aiStore.activeProvider ?? ""}
          onchange={(e) => selectProvider((e.currentTarget as HTMLSelectElement).value)}
        >
          <option value="">(none)</option>
          {#each aiStore.providers as p (p.id)}
            <option value={p.id}>{p.label}</option>
          {/each}
        </select>
      </label>

      <label>
        <span>Model</span>
        <select
          value={aiStore.activeModel ?? ""}
          onchange={(e) => selectModel((e.currentTarget as HTMLSelectElement).value)}
          disabled={!aiStore.activeProvider}
        >
          <option value="">(choose model)</option>
          {#each activeModels as m (m.id)}
            <option value={m.id}>{m.label}</option>
          {/each}
          {#if aiStore.activeModel && !activeModels.find((m) => m.id === aiStore.activeModel)}
            <option value={aiStore.activeModel}>{aiStore.activeModel} (custom)</option>
          {/if}
        </select>
      </label>

      {#if aiStore.activeProvider}
        <div class="row-actions">
          <button class="ghost-btn" onclick={() => loadModelsFor(aiStore.activeProvider!)} disabled={modelLoadInFlight[aiStore.activeProvider!]}>
            {modelLoadInFlight[aiStore.activeProvider!] ? "…" : "Refresh model list"}
          </button>
        </div>
      {/if}
    </div>
  </section>
</div>

<style>
  .settings {
    padding: 12px;
    color: var(--color-text-primary);
    font-size: 12px;
  }
  .provider-card {
    padding: 12px;
    border: 1px solid var(--color-border);
    border-radius: 12px;
    background: var(--color-surface-1);
  }
  .provider-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 10px;
  }
  .provider-header h3 {
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
  }
  .provider-header p {
    margin: 2px 0 0 0;
    font-size: 10.5px;
    color: var(--color-text-muted);
  }
  .provider-header code {
    font-size: 10.5px;
    background: var(--color-surface-2);
    padding: 1px 4px;
    border-radius: 4px;
  }
  .provider-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .provider-row label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 10.5px;
    color: var(--color-text-secondary);
  }
  .provider-row input,
  .provider-row select {
    padding: 6px 8px;
    border-radius: 8px;
    border: 1px solid var(--color-border);
    background: var(--color-surface-0);
    color: var(--color-text-primary);
    font-size: 12px;
    outline: none;
    font-family: inherit;
  }
  .provider-row input:focus,
  .provider-row select:focus {
    border-color: var(--color-border-active, var(--color-accent));
  }
  .row-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 4px;
  }
  .ghost-btn {
    height: 26px;
    padding: 0 10px;
    border-radius: 999px;
    border: 1px solid var(--color-border);
    background: var(--color-surface-2);
    color: var(--color-text-primary);
    font-size: 10.5px;
    cursor: pointer;
    transition: background-color 150ms ease;
  }
  .ghost-btn:hover:not(:disabled) {
    background: var(--color-surface-3);
  }
  .ghost-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .ghost-btn.primary {
    background: var(--color-accent);
    color: var(--color-surface-0);
    border-color: transparent;
  }
  .ghost-btn.danger {
    color: var(--color-danger, #d97373);
  }
  .detected {
    margin-bottom: 10px;
    font-size: 11px;
    color: var(--color-text-secondary);
  }
  .health {
    margin-top: 6px;
    font-size: 11px;
  }
  .health.ok {
    color: var(--color-accent);
  }
  .health.fail {
    color: var(--color-danger, #d97373);
  }
</style>
