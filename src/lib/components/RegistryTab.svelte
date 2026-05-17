<!--
  @agent-context: Registry / marketplace browser tab. Searches multiple
  marketplaces (Skills.sh, ClawHub) via a uniform provider abstraction in the
  Rust backend. Each result card exposes:
    - copy install command (clipboard)
    - open source URL (origin repo, opens in default browser)
    - open marketplace page URL (skills.sh / clawhub.ai listing)
    - copy reference (pseudo-skill path)

  Design mirrors SkillCard.svelte: glass-card surfaces, accent badges, kind
  iconography, hover lift, focus ring.
-->
<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";

  import {
    copySkillReference,
    labelForProvider,
    searchMarketplace,
    setRegistryProvider,
    showToast,
    store,
  } from "$lib/stores/skills.svelte";
  import type {
    RegistryItem,
    RegistryProviderId,
    RegistryProviderSelection,
    Skill,
  } from "$lib/types";

  const PROVIDER_OPTIONS: { id: RegistryProviderSelection; label: string; hint: string }[] = [
    { id: "skills-sh", label: "Skills.sh", hint: "npx skills, Vercel registry" },
    { id: "claw-hub", label: "ClawHub", hint: "OpenClaw community registry" },
    { id: "all", label: "All sources", hint: "Fan-out search across every hub" },
  ];

  function buildPseudoSkill(item: RegistryItem): Skill {
    return {
      id: `registry:${item.provider}:${item.id}`,
      name: item.name,
      description: item.description ?? `Registry entry ${item.id}`,
      artifactType: "skill",
      agentId: "universal",
      sourceAgents: [],
      filePath: item.id,
      sourcePaths: [],
      legacyIds: [],
      scope: "global",
      projectPath: null,
      metadata: {
        version: item.version,
        author: item.author,
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
        repositoryUrl: item.sourceUrl,
        installCommand: item.installCommand,
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
      void searchMarketplace();
    }
  }

  async function copyInstallCommand(item: RegistryItem) {
    try {
      await navigator.clipboard.writeText(item.installCommand);
      showToast("Copied install command");
    } catch {
      showToast("Could not copy install command");
    }
  }

  function copyReference(item: RegistryItem) {
    void copySkillReference(buildPseudoSkill(item));
  }

  async function openExternalUrl(url: string | null, label: string) {
    if (!url) return;
    try {
      await openUrl(url);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      showToast(`${label}: ${message}`);
    }
  }

  function prettyHost(url: string | null): string | null {
    if (!url) return null;
    try {
      return new URL(url).host.replace(/^www\./, "");
    } catch {
      return url;
    }
  }

  function avatarFallback(item: RegistryItem): string {
    const source = item.author ?? item.authorHandle ?? item.name;
    return source.trim().charAt(0).toUpperCase() || "?";
  }

  function providerBadgeColor(provider: RegistryProviderId): string {
    switch (provider) {
      case "skills-sh":
        return "var(--color-accent)";
      case "claw-hub":
        return "#6dbf9f";
      default:
        return "var(--color-text-muted)";
    }
  }
</script>

<div class="h-full flex flex-col gap-2 px-3 pb-3">
  <!-- Provider selector + search bar -->
  <div
    class="rounded-md border p-2"
    style="background: var(--color-surface-1); border-color: var(--color-border);"
  >
    <!-- Provider segment control -->
    <div class="flex flex-wrap items-center gap-1 mb-2" role="tablist" aria-label="Registry source">
      {#each PROVIDER_OPTIONS as option (option.id)}
        {@const isActive = store.registryProvider === option.id}
        <button
          type="button"
          role="tab"
          aria-selected={isActive}
          class="group flex items-center gap-1.5 rounded-md border px-2 py-1 text-[10px] font-medium
            transition-all duration-150"
          style="
            border-color: {isActive ? 'var(--color-border-active)' : 'var(--color-border)'};
            background: {isActive ? 'var(--color-accent-subtle)' : 'var(--color-surface-2)'};
            color: {isActive ? 'var(--color-accent)' : 'var(--color-text-secondary)'};
          "
          title={option.hint}
          onclick={() => setRegistryProvider(option.id)}
        >
          <span
            class="h-1.5 w-1.5 rounded-full"
            style="background: {isActive
              ? 'var(--color-accent)'
              : option.id === 'all'
                ? 'var(--color-text-muted)'
                : providerBadgeColor(option.id as RegistryProviderId)};"
          ></span>
          {option.label}
        </button>
      {/each}
    </div>

    <!-- Search input -->
    <div class="flex items-center gap-2">
      <input
        type="text"
        class="flex-1 rounded-md border px-2 py-1.5 text-[11px]
          focus:outline-none focus:border-[var(--color-accent-muted)]"
        style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-primary);"
        bind:value={store.registryQuery}
        placeholder="Search {labelForProvider(store.registryProvider)} — rust, react, testing..."
        onkeydown={handleKeydown}
      />
      <button
        type="button"
        class="rounded-md border px-2 py-1.5 text-[10px] font-medium transition-colors duration-150"
        style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-secondary);"
        onclick={() => void searchMarketplace()}
        disabled={store.registryLoading}
      >
        {store.registryLoading ? "Searching..." : "Search"}
      </button>
    </div>

    <!-- Status line -->
    <div class="mt-1.5 flex items-center justify-between gap-2 text-[10px] text-[var(--color-text-muted)]">
      {#if store.registryLastSearchedQuery.length >= 2}
        <span>
          {store.registryResultCount} results across {labelForProvider(store.registryLastSearchedProvider)}
          {#if store.registryDurationMs > 0}
            • {store.registryDurationMs}ms
          {/if}
        </span>
        {#if store.registryProvider === "all" && store.registryProviderOutcomes.length > 0}
          <span class="flex items-center gap-1.5">
            {#each store.registryProviderOutcomes as outcome (outcome.provider)}
              <span
                class="flex items-center gap-1 rounded px-1.5 py-0.5"
                style="background: var(--color-surface-2); border: 1px solid var(--color-border);"
                title={outcome.error ?? `${outcome.response?.items.length ?? 0} from ${labelForProvider(outcome.provider)}`}
              >
                <span
                  class="h-1 w-1 rounded-full"
                  style="background: {outcome.error
                    ? 'var(--color-error)'
                    : providerBadgeColor(outcome.provider)};"
                ></span>
                <span>{labelForProvider(outcome.provider)}</span>
                <span class="tabular-nums">{outcome.response?.items.length ?? 0}</span>
              </span>
            {/each}
          </span>
        {/if}
      {:else}
        <span>Type at least 2 characters. Pick a source above to scope your search.</span>
      {/if}
    </div>
  </div>

  <!-- Results -->
  <div
    class="skill-list flex-1 overflow-y-auto rounded-md border"
    style="background: var(--color-surface-1); border-color: var(--color-border);"
  >
    {#if store.registryLoading}
      <div class="flex items-center gap-2 px-3 py-4 text-[11px] text-[var(--color-text-muted)]">
        <div
          class="h-3 w-3 rounded-full border-[1.5px] border-[var(--color-accent)] border-t-transparent"
          style="animation: refresh-spin 0.7s linear infinite;"
        ></div>
        Searching {labelForProvider(store.registryProvider)}...
      </div>
    {:else if store.registryError}
      <div class="px-3 py-4 text-[11px] text-[var(--color-error)]">{store.registryError}</div>
    {:else if store.registryLastSearchedQuery.length < 2}
      <div class="px-3 py-4 text-[11px] text-[var(--color-text-muted)]">
        Start with a keyword like rust, svelte, testing, or docker.
      </div>
    {:else if store.registryItems.length === 0}
      <div class="px-3 py-4 text-[11px] text-[var(--color-text-muted)]">
        No matching items in {labelForProvider(store.registryLastSearchedProvider)}.
      </div>
    {:else}
      <div class="divide-y" style="border-color: var(--color-border);">
        {#each store.registryItems as item (item.provider + ":" + item.id)}
          {@const sourceHost = prettyHost(item.sourceUrl)}
          {@const homepageHost = prettyHost(item.homepageUrl)}
          <article class="group flex w-full gap-3 px-3 py-2.5">
            <!-- Avatar -->
            <div
              class="relative flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-lg
                text-[12px] font-semibold text-[var(--color-text-secondary)]"
              style="background: var(--color-surface-3); border: 1px solid var(--color-border);"
              title={item.author ?? item.authorHandle ?? ""}
            >
              {#if item.authorAvatarUrl}
                <img
                  src={item.authorAvatarUrl}
                  alt={item.author ?? ""}
                  class="h-full w-full object-cover"
                  loading="lazy"
                  referrerpolicy="no-referrer"
                />
              {:else}
                <span>{avatarFallback(item)}</span>
              {/if}
            </div>

            <div class="min-w-0 flex-1">
              <!-- Header -->
              <div class="flex items-start justify-between gap-2">
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-1.5">
                    <h3
                      class="truncate text-[12px] font-medium leading-snug text-[var(--color-text-primary)]"
                    >
                      {item.name}
                    </h3>
                    {#if item.version}
                      <span
                        class="rounded-md border px-1.5 py-0.5 text-[9px] font-semibold tabular-nums text-[var(--color-text-secondary)]"
                        style="border-color: var(--color-border); background: var(--color-surface-3);"
                      >
                        v{item.version}
                      </span>
                    {/if}
                  </div>
                  <p class="truncate text-[10px] text-[var(--color-text-muted)]">
                    {item.id}
                    {#if item.author}
                      <span class="text-[var(--color-text-muted)]">· by {item.author}</span>
                    {/if}
                  </p>
                </div>

                <div class="flex shrink-0 items-center gap-1">
                  {#if item.installs > 0}
                    <span
                      class="rounded-md border px-1.5 py-0.5 text-[9px] tabular-nums"
                      style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-secondary);"
                      title="Total installs"
                    >
                      {item.installs.toLocaleString()}
                    </span>
                  {/if}
                </div>
              </div>

              {#if item.description}
                <p
                  class="mt-1 line-clamp-2 text-[11px] leading-relaxed text-[var(--color-text-secondary)]"
                >
                  {item.description}
                </p>
              {/if}

              <!-- Metadata row -->
              <div class="mt-1.5 flex flex-wrap items-center gap-1.5">
                <span
                  class="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[9px] font-medium"
                  style="
                    background: var(--color-surface-2);
                    color: var(--color-text-secondary);
                    border: 1px solid var(--color-border);
                  "
                >
                  <span
                    class="h-1 w-1 rounded-full"
                    style="background: {providerBadgeColor(item.provider)};"
                  ></span>
                  {labelForProvider(item.provider)}
                </span>

                <span
                  class="rounded-md border px-1.5 py-0.5 text-[9px] font-medium"
                  style="border-color: var(--color-border); background: var(--color-surface-1); color: var(--color-text-muted);"
                  title="Artifact kind"
                >
                  {item.kind}
                </span>

                {#if item.updatedAt}
                  <span
                    class="rounded-md px-1.5 py-0.5 text-[9px] text-[var(--color-text-muted)]"
                    style="background: var(--color-surface-2); border: 1px solid var(--color-border);"
                    title={item.updatedAt}
                  >
                    {item.updatedAt.slice(0, 10)}
                  </span>
                {/if}

                {#if item.score !== null && item.score !== undefined && item.score > 0}
                  <span
                    class="rounded-md px-1.5 py-0.5 text-[9px] tabular-nums text-[var(--color-text-muted)]"
                    style="background: var(--color-surface-2); border: 1px solid var(--color-border);"
                    title="Relevance score"
                  >
                    score {item.score.toFixed(2)}
                  </span>
                {/if}
              </div>

              <!-- Source links + actions -->
              <div class="mt-2 flex flex-wrap items-center gap-1.5">
                {#if item.sourceUrl}
                  <button
                    type="button"
                    class="flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[9px] font-medium
                      transition-colors duration-150
                      hover:bg-[var(--color-surface-3)] hover:text-[var(--color-accent)]"
                    style="border-color: var(--color-border); color: var(--color-text-secondary);"
                    onclick={() => openExternalUrl(item.sourceUrl, "Open source failed")}
                    title="Open the original source — {item.sourceUrl}"
                  >
                    <svg
                      class="h-3 w-3"
                      fill="none"
                      viewBox="0 0 24 24"
                      stroke="currentColor"
                      stroke-width="2"
                    >
                      <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"
                      />
                    </svg>
                    {sourceHost ?? "Source"}
                  </button>
                {/if}

                {#if item.homepageUrl}
                  <button
                    type="button"
                    class="flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[9px] font-medium
                      transition-colors duration-150
                      hover:bg-[var(--color-surface-3)] hover:text-[var(--color-accent)]"
                    style="border-color: var(--color-border); color: var(--color-text-secondary);"
                    onclick={() => openExternalUrl(item.homepageUrl, "Open hub page failed")}
                    title="Open marketplace listing — {item.homepageUrl}"
                  >
                    <svg
                      class="h-3 w-3"
                      fill="none"
                      viewBox="0 0 24 24"
                      stroke="currentColor"
                      stroke-width="2"
                    >
                      <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        d="M3.055 11H5a2 2 0 012 2v1a2 2 0 002 2 2 2 0 012 2v2.945M8 3.935V5.5A2.5 2.5 0 0010.5 8h.5a2 2 0 012 2 2 2 0 104 0 2 2 0 012-2h1.064M15 20.488V18a2 2 0 012-2h3.064M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                      />
                    </svg>
                    {homepageHost ?? "Listing"}
                  </button>
                {/if}

                <button
                  type="button"
                  class="rounded-md border px-1.5 py-0.5 text-[9px] font-medium transition-colors duration-150
                    hover:bg-[var(--color-surface-3)] hover:text-[var(--color-accent)]"
                  style="border-color: var(--color-border); color: var(--color-text-secondary);"
                  onclick={() => copyInstallCommand(item)}
                  title="Copy install command"
                >
                  Copy install
                </button>

                <button
                  type="button"
                  class="rounded-md border px-1.5 py-0.5 text-[9px] font-medium transition-colors duration-150
                    hover:bg-[var(--color-surface-3)] hover:text-[var(--color-accent)]"
                  style="border-color: var(--color-border); color: var(--color-text-secondary);"
                  onclick={() => copyReference(item)}
                  title="Copy reference (pseudo-skill path)"
                >
                  Copy ref
                </button>
              </div>

              <!-- Install command preview -->
              <p
                class="mt-1.5 truncate font-mono text-[9px] text-[var(--color-text-muted)]"
                title={item.installCommand}
              >
                {item.installCommand}
              </p>
            </div>
          </article>
        {/each}
      </div>
    {/if}
  </div>
</div>
