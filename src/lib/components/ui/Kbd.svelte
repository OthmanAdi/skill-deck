<!--
  @agent-context: Kbd chip primitive (DESIGN 8). Renders real <kbd> elements so
  shortcut hints are semantic, not styled prose spans. All visuals are global in
  app.css (kbd, .kbd). Two ways to use it:
    <Kbd>Esc</Kbd>                      single key
    <Kbd keys={["Ctrl","Shift","K"]} /> a combo with hairline "+" separators
-->
<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    keys = null,
    class: klass = "",
    children,
  }: {
    /** Render a combo. When set, `children` is ignored. */
    keys?: string[] | null;
    class?: string;
    children?: Snippet;
  } = $props();
</script>

{#if keys && keys.length}
  <span class="inline-flex items-center gap-1 {klass}">
    {#each keys as key, i (key + i)}
      {#if i > 0}
        <span class="text-text-muted text-2xs select-none">+</span>
      {/if}
      <kbd>{key}</kbd>
    {/each}
  </span>
{:else}
  <kbd class={klass}>{@render children?.()}</kbd>
{/if}
