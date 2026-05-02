<!--
  @agent-context: Theme selection popover triggered by gear icon in title bar.
  Lists all registered themes from the theme store.
  Active theme has a checkmark. Click to switch instantly.
-->
<script lang="ts">
  import { themeStore, setTheme, THEMES, type ThemeId } from "$lib/stores/theme.svelte";

  let isOpen = $state(false);
  let menuEl: HTMLDivElement | undefined = $state();
  let triggerEl: HTMLButtonElement | undefined = $state();

  function toggle(e: MouseEvent) {
    e.stopPropagation();
    isOpen = !isOpen;
  }

  function selectTheme(themeId: ThemeId) {
    setTheme(themeId);
    isOpen = false;
  }

  function handleWindowClick(e: MouseEvent) {
    if (isOpen && menuEl && !menuEl.contains(e.target as Node)) {
      isOpen = false;
    }
  }

  function handleWindowKeydown(e: KeyboardEvent) {
    if (isOpen && e.key === "Escape") {
      isOpen = false;
      triggerEl?.focus();
    }
  }
</script>

<svelte:window onclick={handleWindowClick} onkeydown={handleWindowKeydown} />

<div class="relative" bind:this={menuEl}>
  <!-- Trigger: gear icon -->
  <button
    bind:this={triggerEl}
    class="flex h-6 w-6 items-center justify-center rounded-[var(--radius-sm)]
      text-[var(--color-text-muted)]
      transition-all duration-[120ms] ease-out
      hover:bg-[var(--color-surface-2)] hover:text-[var(--color-text-secondary)]
      active:scale-[0.95]"
    onclick={toggle}
    aria-label="Settings / Theme"
    aria-expanded={isOpen}
    aria-haspopup="menu"
  >
    <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
      <path stroke-linecap="round" stroke-linejoin="round"
        d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
      <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
    </svg>
  </button>

  <!-- Dropdown menu -->
  {#if isOpen}
    <div
      role="menu"
      class="absolute right-0 top-full z-50 mt-1.5 w-[200px]
        rounded-[var(--radius-md)] border border-[var(--color-border)]
        bg-[var(--color-surface-1)] py-1
        shadow-[0_12px_32px_-6px_var(--color-overlay-shadow)]"
    >
      <!-- Section label -->
      <div class="px-3 py-1.5 text-[9px] font-semibold uppercase tracking-[0.08em]
        text-[var(--color-text-muted)]">
        Theme
      </div>

      <div class="mx-1 border-t border-[var(--color-border)] mb-1"></div>

      {#each THEMES as theme (theme.id)}
        {@const isActive = themeStore.currentThemeId === theme.id}
        <button
          role="menuitem"
          class="flex w-full items-center gap-2.5 rounded-[var(--radius-sm)] mx-1 px-2 py-2
            text-left
            transition-[background-color] duration-[120ms] ease-out
            hover:bg-[var(--color-surface-2)]"
          style="width: calc(100% - 8px);"
          onclick={() => selectTheme(theme.id)}
        >
          <!-- Checkmark / placeholder -->
          <span class="flex h-3.5 w-3.5 shrink-0 items-center justify-center">
            {#if isActive}
              <svg class="h-3.5 w-3.5 text-[var(--color-accent)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
              </svg>
            {/if}
          </span>

          <div class="min-w-0">
            <div class="text-[12px] font-medium
              {isActive ? 'text-[var(--color-text-primary)]' : 'text-[var(--color-text-secondary)]'}">
              {theme.name}
            </div>
            <div class="text-[10px] text-[var(--color-text-muted)] leading-snug">
              {theme.description}
            </div>
          </div>

          <!-- Color scheme indicator dot -->
          <span
            class="ml-auto h-2 w-2 shrink-0 rounded-full border border-[var(--color-border)]"
            style="background: {theme.id === 'system' ? 'linear-gradient(135deg, #18191a 50%, #f5f6f6 50%)' : theme.colorScheme === 'dark' ? '#18191a' : '#f5f6f6'};"
            title={theme.colorScheme}
          ></span>
        </button>
      {/each}
    </div>
  {/if}
</div>
