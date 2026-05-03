<!--
  @agent-context: Registry browser tab for searching public npx skills entries.
  Keeps install action explicit by copying install command instead of auto-running.
-->
<script lang="ts">
  import {
    copySkillReference,
    searchSkillsRegistry,
    showToast,
    store,
  } from "$lib/stores/skills.svelte";
  import type { RegistrySkillSummary, Skill } from "$lib/types";

  function buildPseudoSkill(entry: RegistrySkillSummary): Skill {
    return {
      id: `registry:${entry.id}`,
      name: entry.name,
      description: `Registry entry ${entry.id}`,
      artifactType: "skill",
      agentId: "universal",
      sourceAgents: [],
      filePath: entry.id,
      sourcePaths: [],
      legacyIds: [],
      scope: "global",
      projectPath: null,
      metadata: {
        version: null,
        author: null,
        category: null,
        tags: null,
        useCases: null,
        globs: null,
        trigger: null,
        allowedTools: null,
        userInvocable: null,
        language: null,
        slashCommand: null,
        hookEvent: null,
        hookMatcher: null,
        hookCommand: null,
        extra: null,
        repositoryUrl: null,
        installCommand: entry.installCommand,
      },
      discoveryTags: [],
      useCases: [],
      discoveryHints: [],
      icon: null,
      starred: false,
      updateAvailable: false,
      installedAt: null,
      parentId: null,
      children: [],
    };
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      void searchSkillsRegistry();
    }
  }

  async function copyInstallCommand(entry: RegistrySkillSummary) {
    try {
      await navigator.clipboard.writeText(entry.installCommand);
      showToast("Copied install command");
    } catch {
      showToast("Could not copy install command");
    }
  }

  function copyReference(entry: RegistrySkillSummary) {
    void copySkillReference(buildPseudoSkill(entry));
  }
</script>

<div class="h-full flex flex-col gap-2 px-3 pb-3">
  <div class="rounded-md border p-2"
    style="background: var(--color-surface-1); border-color: var(--color-border);">
    <div class="flex items-center gap-2">
      <input
        type="text"
        class="flex-1 rounded-md border px-2 py-1.5 text-[11px]
          focus:outline-none"
        style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-primary);"
        bind:value={store.registryQuery}
        placeholder="Search registry, example rust"
        onkeydown={handleKeydown}
      />
      <button
        class="rounded-md border px-2 py-1.5 text-[10px] font-medium"
        style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-secondary);"
        onclick={() => void searchSkillsRegistry()}
        disabled={store.registryLoading}
      >
        {store.registryLoading ? "Searching..." : "Search"}
      </button>
    </div>

    <div class="mt-1.5 text-[10px] text-[var(--color-text-muted)]">
      {#if store.registryLastSearchedQuery.length >= 2}
        {store.registryResultCount} results
        {#if store.registryDurationMs > 0}
          in {store.registryDurationMs}ms
        {/if}
      {:else}
        Type at least 2 characters to search the npx skills registry
      {/if}
    </div>
  </div>

  <div class="skill-list flex-1 overflow-y-auto rounded-md border"
    style="background: var(--color-surface-1); border-color: var(--color-border);">
    {#if store.registryLoading}
      <div class="px-3 py-4 text-[11px] text-[var(--color-text-muted)]">Searching registry...</div>
    {:else if store.registryError}
      <div class="px-3 py-4 text-[11px] text-[var(--color-error)]">{store.registryError}</div>
    {:else if store.registryLastSearchedQuery.length < 2}
      <div class="px-3 py-4 text-[11px] text-[var(--color-text-muted)]">
        Start with a keyword like rust, svelte, testing, or docker
      </div>
    {:else if store.registryResults.length === 0}
      <div class="px-3 py-4 text-[11px] text-[var(--color-text-muted)]">No matching registry skills</div>
    {:else}
      <div class="divide-y" style="border-color: var(--color-border);">
        {#each store.registryResults as entry (entry.id)}
          <div class="px-3 py-2.5">
            <div class="flex items-start justify-between gap-2">
              <div class="min-w-0">
                <p class="truncate text-[12px] font-medium text-[var(--color-text-primary)]">{entry.name}</p>
                <p class="truncate text-[10px] text-[var(--color-text-muted)]">{entry.id}</p>
              </div>
              <span class="shrink-0 rounded-md border px-1.5 py-0.5 text-[9px] tabular-nums"
                style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-secondary);">
                {entry.installs}
              </span>
            </div>

            <div class="mt-2 flex flex-wrap items-center gap-1.5">
              {#if entry.source}
                <span class="rounded-md border px-1.5 py-0.5 text-[9px]"
                  style="border-color: var(--color-border); color: var(--color-text-muted);">
                  {entry.source}
                </span>
              {/if}

              <button
                class="rounded-md border px-1.5 py-0.5 text-[9px]"
                style="border-color: var(--color-border); color: var(--color-text-secondary);"
                onclick={() => copyInstallCommand(entry)}
              >
                Copy install
              </button>

              <button
                class="rounded-md border px-1.5 py-0.5 text-[9px]"
                style="border-color: var(--color-border); color: var(--color-text-secondary);"
                onclick={() => copyReference(entry)}
              >
                Copy ref
              </button>
            </div>

            <p class="mt-1.5 truncate font-mono text-[9px] text-[var(--color-text-muted)]">
              {entry.installCommand}
            </p>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
