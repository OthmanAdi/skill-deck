<!--
  @agent-context: Registry / marketplace browser tab. Searches multiple
  marketplaces (Skills.sh, ClawHub) via a uniform provider abstraction in the
  Rust backend. Each result row exposes:
    - copy install command (clipboard, toast kind 'ok' / 'error')
    - open source URL (origin repo, opens in default browser)
    - open marketplace page URL (skills.sh / clawhub.ai listing)
    - copy reference (pseudo-skill path)

  PHOSPHOR (DESIGN 8): flat surface-1 rows on the 4px grid, hairline dividers,
  a subtle surface-2 hover tint (no glass, no lift). The provider selector is a
  segmented control whose active tab carries the one accent as small text
  (--color-accent-text); the rest stay quiet. Source and kind render as
  <Badge variant="meta"> chips; installs / date / score fold into one muted
  tabular stats line so the row never pill-floods. Copy install is the single
  emphasized row action (btn-secondary); Source / Listing / Copy ref stay
  btn-ghost. Loading shows skeleton rows on the shared skeleton-shimmer; the
  error state gives an honest headline + reason + Retry. Buttons / inputs use
  the .btn-* / .field / .seg primitives so press feedback + focus rings come
  from the system, and the one <Spinner/> replaces the old inline refresh-spin.
-->
<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";

  import Badge from "./ui/Badge.svelte";
  import Spinner from "./ui/Spinner.svelte";
  import {
    copySkillReference,
    labelForProvider,
    searchMarketplace,
    setRegistryQuery,
    setRegistryProvider,
    showToast,
    store,
  } from "$lib/stores/skills.svelte";
  import type {
    RegistryItem,
    RegistryProviderSelection,
    Skill,
  } from "$lib/types";

  const PROVIDER_OPTIONS: { id: RegistryProviderSelection; label: string; hint: string }[] = [
    { id: "skills-sh", label: "Skills.sh", hint: "npx skills, Vercel registry" },
    { id: "claw-hub", label: "ClawHub", hint: "OpenClaw community registry" },
    { id: "all", label: "All sources", hint: "Fan-out search across every hub" },
  ];

  // Placeholder rows while a search is in flight (foundation skeleton-shimmer).
  const SKELETON_ROWS = [0, 1, 2, 3];

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
      lastModifiedAt: null,
      archiveCount: 0,
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
      showToast("Copied install command", "ok");
    } catch {
      showToast("Could not copy install command", "error");
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
      showToast(`${label}: ${message}`, "error");
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

  // Fold the popularity / freshness / relevance numbers into one muted,
  // tabular line instead of three separate pills (DESIGN 8: at most 2 meta
  // chips + no pill flooding). Every value stays visible; nothing is dropped.
  function metaStats(item: RegistryItem): string {
    const parts: string[] = [];
    if (item.installs > 0) parts.push(`${item.installs.toLocaleString()} installs`);
    if (item.updatedAt) parts.push(`updated ${item.updatedAt.slice(0, 10)}`);
    if (item.score !== null && item.score > 0) parts.push(`score ${item.score.toFixed(2)}`);
    return parts.join(" · ");
  }
</script>

<div class="h-full flex flex-col gap-2 px-3 pb-3">
  <!-- Provider selector + search bar -->
  <div
    class="rounded-md border p-2"
    style="background: var(--color-surface-1); border-color: var(--color-border);"
  >
    <!-- Provider segment control -->
    <div class="mb-2 flex flex-wrap items-center gap-1" role="tablist" aria-label="Registry source">
      {#each PROVIDER_OPTIONS as option (option.id)}
        {@const isActive = store.registryProvider === option.id}
        <button
          type="button"
          role="tab"
          aria-selected={isActive}
          class="seg hit-expand instant-tooltip"
          data-tooltip={option.hint}
          onclick={() => setRegistryProvider(option.id)}
        >
          {option.label}
        </button>
      {/each}
    </div>

    <!-- Search input -->
    <div class="flex items-center gap-2">
      <input
        type="text"
        class="field flex-1"
        value={store.registryQuery}
        placeholder="Search {labelForProvider(store.registryProvider)} — rust, react, testing..."
        oninput={(event) => setRegistryQuery(event.currentTarget.value)}
        onkeydown={handleKeydown}
      />
      <button
        type="button"
        class="btn-secondary hit-expand"
        onclick={() => void searchMarketplace()}
        disabled={store.registryLoading}
      >
        {#if store.registryLoading}
          <Spinner size={12} label="Searching" />
          Searching...
        {:else}
          Search
        {/if}
      </button>
    </div>

    <!-- Status line -->
    <div class="mt-2 flex items-center justify-between gap-2 text-2xs text-[var(--color-text-muted)]">
      {#if store.registryLastSearchedQuery.length >= 2}
        <span class="tabular-nums">
          {store.registryResultCount} results across {labelForProvider(store.registryLastSearchedProvider)}
          {#if store.registryDurationMs > 0}
            · {store.registryDurationMs}ms
          {/if}
        </span>
        {#if store.registryProvider === "all" && store.registryProviderOutcomes.length > 0}
          <span class="flex flex-wrap items-center gap-1">
            {#each store.registryProviderOutcomes as outcome (outcome.provider)}
              <span
                class="badge-meta instant-tooltip"
                data-tooltip={outcome.error ?? `${outcome.response?.items.length ?? 0} from ${labelForProvider(outcome.provider)}`}
              >
                <span
                  class="mr-1 inline-block h-1.5 w-1.5 rounded-full"
                  style="background: {outcome.error ? 'var(--color-error)' : 'var(--color-ok)'};"
                ></span>
                {labelForProvider(outcome.provider)}
                <span class="ml-1 tabular-nums">{outcome.response?.items.length ?? 0}</span>
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
      <!-- Loading skeletons (DESIGN 7 shimmer) -->
      <div class="divide-y" style="border-color: var(--color-border);">
        {#each SKELETON_ROWS as row (row)}
          <div class="flex gap-3 px-3 py-3">
            <div class="sk-avatar h-9 w-9 shrink-0"></div>
            <div class="min-w-0 flex-1 space-y-2 py-1">
              <div class="sk h-3 w-1/3"></div>
              <div class="sk h-2 w-3/5"></div>
              <div class="flex gap-2 pt-1">
                <div class="sk h-4 w-16"></div>
                <div class="sk h-4 w-12"></div>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {:else if store.registryError && store.registryItems.length === 0}
      <div class="flex flex-col items-start gap-2 px-3 py-4">
        <p class="text-xs font-medium text-[var(--color-error)]">
          Couldn't reach {labelForProvider(store.registryAttemptedProvider)}
        </p>
        <p class="text-2xs text-pretty text-[var(--color-text-muted)]">{store.registryError}</p>
        <button
          type="button"
          class="btn-secondary btn-sm hit-expand"
          onclick={() => void searchMarketplace()}
        >
          Retry search
        </button>
      </div>
    {:else if store.registryLastSearchedQuery.length < 2}
      <div class="px-3 py-4 text-xs text-[var(--color-text-muted)]">
        Start with a keyword like rust, svelte, testing, or docker.
      </div>
    {:else if store.registryItems.length === 0}
      <div class="px-3 py-4 text-xs text-[var(--color-text-muted)]">
        No matching items in {labelForProvider(store.registryLastSearchedProvider)}.
      </div>
    {:else}
      {#if store.registryError}
        <div
          class="flex items-center gap-2 border-b px-3 py-2 text-2xs text-[var(--color-warn)]"
          style="border-color: var(--color-border); background: var(--color-warn-subtle);"
          role="status"
        >
          <span aria-hidden="true">!</span>
          <span>Latest search failed. Showing the last successful results.</span>
          <button type="button" class="btn-ghost btn-sm hit-expand ml-auto" onclick={() => void searchMarketplace()}>Retry</button>
        </div>
      {/if}
      <div class="divide-y" style="border-color: var(--color-border);">
        {#each store.registryItems as item (item.provider + ":" + item.id)}
          {@const sourceHost = prettyHost(item.sourceUrl)}
          {@const homepageHost = prettyHost(item.homepageUrl)}
          {@const stats = metaStats(item)}
          <article class="reg-row flex w-full gap-3 px-3 py-3">
            <!-- Avatar -->
            <div
              class="instant-tooltip relative flex h-9 w-9 shrink-0 items-center justify-center overflow-hidden rounded-md
                text-sm font-semibold text-[var(--color-text-secondary)]"
              style="background: var(--color-surface-3); border: 1px solid var(--color-border);"
              data-tooltip={item.author ?? item.authorHandle ?? "Unknown author"}
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
              <!-- Name + version -->
              <div class="flex items-center gap-2">
                <h3 class="truncate text-sm font-medium leading-snug text-[var(--color-text-primary)]">
                  {item.name}
                </h3>
                {#if item.version}
                  <span class="shrink-0 text-2xs tabular-nums text-[var(--color-text-muted)]">
                    v{item.version}
                  </span>
                {/if}
              </div>
              <p class="truncate text-2xs text-[var(--color-text-muted)]">
                {item.id}{#if item.author} · by {item.author}{/if}
              </p>

              {#if item.description}
                <p class="mt-2 line-clamp-2 text-xs leading-relaxed text-pretty text-[var(--color-text-secondary)]">
                  {item.description}
                </p>
              {/if}

              <!-- Source + kind badges + folded stats -->
              <div class="mt-2 flex flex-wrap items-center gap-2 text-2xs text-[var(--color-text-muted)]">
                <Badge variant="meta">{labelForProvider(item.provider)}</Badge>
                <Badge variant="meta">{item.kind}</Badge>
                {#if stats}
                  <span class="tabular-nums">{stats}</span>
                {/if}
              </div>

              <!-- Source links + actions -->
              <div class="mt-2 flex flex-wrap items-center gap-2">
                {#if item.sourceUrl}
                  <button
                    type="button"
                    class="btn-ghost btn-sm hit-expand instant-tooltip"
                    onclick={() => openExternalUrl(item.sourceUrl, "Open source failed")}
                    data-tooltip="Open the original source — {item.sourceUrl}"
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
                    class="btn-ghost btn-sm hit-expand instant-tooltip"
                    onclick={() => openExternalUrl(item.homepageUrl, "Open hub page failed")}
                    data-tooltip="Open marketplace listing — {item.homepageUrl}"
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
                  class="btn-secondary btn-sm hit-expand"
                  onclick={() => copyInstallCommand(item)}
                >
                  Copy install
                </button>

                <button
                  type="button"
                  class="btn-ghost btn-sm hit-expand instant-tooltip"
                  onclick={() => copyReference(item)}
                  data-tooltip="Copy the marketplace reference (pseudo-skill path)"
                >
                  Copy ref
                </button>
              </div>

              <!-- Install command preview -->
              <p
                class="install-preview instant-tooltip mt-2 truncate rounded-sm px-2 py-1 text-2xs text-[var(--color-text-muted)]"
                data-tooltip={item.installCommand}
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

<style>
  /* Segmented provider selector. Active tab carries the one accent as AA-safe
     small text (--color-accent-text); the rest stay quiet. Named-property
     transitions + scale(0.96) press feedback from the system vocabulary — never
     transition: all. */
  .seg {
    display: inline-flex;
    align-items: center;
    height: 24px;
    padding: 0 8px;
    font-family: var(--font-ui);
    font-size: var(--text-2xs);
    font-weight: 500;
    line-height: 1;
    color: var(--color-text-secondary);
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition-property: background-color, border-color, color, scale;
    transition-duration: var(--motion-fast);
    transition-timing-function: var(--ease-standard);
  }
  .seg:hover {
    background: var(--color-surface-3);
    color: var(--color-text-primary);
    border-color: var(--color-border-hover);
  }
  .seg:active {
    scale: 0.96;
  }
  .seg[aria-selected="true"] {
    background: var(--color-accent-subtle);
    border-color: var(--color-border-active);
    color: var(--color-accent-text);
  }

  /* Subtle row hover — surface tint only, no lift, no glass (DESIGN 8). */
  .reg-row {
    transition: background-color var(--motion-base) var(--ease-standard);
  }
  .reg-row:hover {
    background: var(--color-surface-2);
  }

  /* Loading skeletons ride the shared skeleton-shimmer keyframe (DESIGN 7). */
  .sk,
  .sk-avatar {
    background: var(--color-surface-3);
    animation: skeleton-shimmer 1.8s ease-in-out infinite;
  }
  .sk {
    border-radius: var(--radius-xs);
  }
  .sk-avatar {
    border-radius: var(--radius-md);
  }

  /* Install command preview — the one mono voice, quiet carbon code bar. */
  .install-preview {
    font-family: var(--font-ui);
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
  }

  @media (prefers-reduced-motion: reduce) {
    .sk,
    .sk-avatar {
      animation-duration: 0.01ms;
    }
  }
</style>
