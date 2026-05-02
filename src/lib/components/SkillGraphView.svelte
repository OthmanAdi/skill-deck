<!--
  @agent-context: Lightweight graph view for skill relationships.
  Renders parent-child links using SVG, supports click to inspect selected node.
-->
<script lang="ts">
  import type { Skill } from "$lib/types";
  import { DEFAULT_AGENT_COLOR } from "$lib/types";
  import { store } from "$lib/stores/skills.svelte";

  type GraphNode = {
    skill: Skill;
    x: number;
    y: number;
    degree: number;
  };

  type GraphEdge = {
    from: string;
    to: string;
  };

  const VIEWPORT_W = 1200;
  const VIEWPORT_H = 740;
  const GRAPH_CENTER_X = VIEWPORT_W / 2;
  const GRAPH_CENTER_Y = VIEWPORT_H / 2;
  const MIN_ZOOM = 0.5;
  const MAX_ZOOM = 2.6;

  let { skills }: { skills: Skill[] } = $props();

  let selectedSkillId = $state<string | null>(null);
  let svgEl: SVGSVGElement | undefined = $state();
  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let isPanning = $state(false);

  let panPointerId: number | null = null;
  let panStartClientX = 0;
  let panStartClientY = 0;
  let panStartX = 0;
  let panStartY = 0;

  const idToSkill = $derived.by(() => {
    const map = new Map<string, Skill>();
    for (const s of skills) map.set(s.id, s);
    return map;
  });

  const edges = $derived.by((): GraphEdge[] => {
    const result: GraphEdge[] = [];
    for (const s of skills) {
      if (s.parentId && idToSkill.has(s.parentId)) {
        result.push({ from: s.parentId, to: s.id });
      }
    }
    return result;
  });

  const degrees = $derived.by(() => {
    const map = new Map<string, number>();
    for (const s of skills) map.set(s.id, 0);
    for (const e of edges) {
      map.set(e.from, (map.get(e.from) ?? 0) + 1);
      map.set(e.to, (map.get(e.to) ?? 0) + 1);
    }
    return map;
  });

  const selectedSkill = $derived.by(() => {
    if (!selectedSkillId) return null;
    return skills.find((s) => s.id === selectedSkillId) ?? null;
  });

  const nodes = $derived.by((): GraphNode[] => {
    if (skills.length === 0) return [];

    const sorted = [...skills].sort((a, b) => {
      const da = degrees.get(a.id) ?? 0;
      const db = degrees.get(b.id) ?? 0;
      return db - da || a.name.localeCompare(b.name);
    });

    const maxRadius = Math.min(VIEWPORT_W, VIEWPORT_H) * 0.43;
    const ringGap = 86;

    const result: GraphNode[] = [];

    sorted.forEach((skill, i) => {
      const ring = Math.floor(i / 20);
      const ringIndex = i % 20;
      const ringCount = Math.min(20, sorted.length - ring * 20);
      const theta = (2 * Math.PI * ringIndex) / Math.max(1, ringCount);
      const radius = Math.min(maxRadius, 68 + ring * ringGap);
      const jitter = ((skill.id.length % 7) - 3) * 3;

      const x = GRAPH_CENTER_X + Math.cos(theta) * (radius + jitter);
      const y = GRAPH_CENTER_Y + Math.sin(theta) * (radius - jitter);

      result.push({
        skill,
        x,
        y,
        degree: degrees.get(skill.id) ?? 0,
      });
    });

    return result;
  });

  const nodeById = $derived.by(() => {
    const map = new Map<string, GraphNode>();
    for (const n of nodes) map.set(n.skill.id, n);
    return map;
  });

  function nodeRadius(node: GraphNode): number {
    const base = 9;
    return Math.min(18, base + node.degree * 1.2);
  }

  function isEdgeActive(edge: GraphEdge): boolean {
    if (!selectedSkillId) return false;
    return edge.from === selectedSkillId || edge.to === selectedSkillId;
  }

  function hasParent(skill: Skill): boolean {
    return !!skill.parentId && idToSkill.has(skill.parentId);
  }

  function childCount(skill: Skill): number {
    let count = 0;
    for (const s of skills) {
      if (s.parentId === skill.id) count += 1;
    }
    return count;
  }

  const graphTransform = $derived.by(() => {
    const offsetX = panX + GRAPH_CENTER_X * (1 - zoom);
    const offsetY = panY + GRAPH_CENTER_Y * (1 - zoom);
    return `matrix(${zoom} 0 0 ${zoom} ${offsetX} ${offsetY})`;
  });

  const zoomPercent = $derived(Math.round(zoom * 100));

  function clampZoom(value: number): number {
    return Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, value));
  }

  function isNodeTarget(target: EventTarget | null): boolean {
    if (!(target instanceof Element)) return false;
    return !!target.closest("[data-graph-node]");
  }

  function toViewportPoint(clientX: number, clientY: number): { x: number; y: number } | null {
    if (!svgEl) return null;
    const rect = svgEl.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    return {
      x: ((clientX - rect.left) / rect.width) * VIEWPORT_W,
      y: ((clientY - rect.top) / rect.height) * VIEWPORT_H,
    };
  }

  function applyZoomAt(viewX: number, viewY: number, nextZoom: number) {
    const clamped = clampZoom(nextZoom);
    if (clamped === zoom) return;

    const currentOffsetX = panX + GRAPH_CENTER_X * (1 - zoom);
    const currentOffsetY = panY + GRAPH_CENTER_Y * (1 - zoom);
    const worldX = (viewX - currentOffsetX) / zoom;
    const worldY = (viewY - currentOffsetY) / zoom;

    panX = viewX - clamped * worldX - GRAPH_CENTER_X * (1 - clamped);
    panY = viewY - clamped * worldY - GRAPH_CENTER_Y * (1 - clamped);
    zoom = clamped;
  }

  function zoomIn() {
    applyZoomAt(GRAPH_CENTER_X, GRAPH_CENTER_Y, zoom * 1.14);
  }

  function zoomOut() {
    applyZoomAt(GRAPH_CENTER_X, GRAPH_CENTER_Y, zoom / 1.14);
  }

  function resetView() {
    zoom = 1;
    panX = 0;
    panY = 0;
  }

  function handleWheel(e: WheelEvent) {
    e.preventDefault();
    const point = toViewportPoint(e.clientX, e.clientY);
    if (!point) return;

    const scaleFactor = Math.exp(-e.deltaY * 0.0016);
    applyZoomAt(point.x, point.y, zoom * scaleFactor);
  }

  function handlePointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    if (isNodeTarget(e.target)) return;

    isPanning = true;
    panPointerId = e.pointerId;
    panStartClientX = e.clientX;
    panStartClientY = e.clientY;
    panStartX = panX;
    panStartY = panY;

    svgEl?.setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function handlePointerMove(e: PointerEvent) {
    if (!isPanning) return;
    if (panPointerId !== e.pointerId) return;

    const start = toViewportPoint(panStartClientX, panStartClientY);
    const current = toViewportPoint(e.clientX, e.clientY);
    if (!start || !current) return;

    panX = panStartX + (current.x - start.x);
    panY = panStartY + (current.y - start.y);
  }

  function endPan(e: PointerEvent) {
    if (!isPanning) return;
    if (panPointerId !== e.pointerId) return;

    isPanning = false;
    panPointerId = null;
  }
</script>

<div class="relative h-full overflow-hidden rounded-xl border"
  style="border-color: var(--color-border); background:
    radial-gradient(circle at 20% 15%, rgba(94, 106, 210, 0.11), transparent 32%),
    radial-gradient(circle at 80% 85%, rgba(94, 106, 210, 0.09), transparent 30%),
    linear-gradient(180deg, var(--color-surface-1), var(--color-surface-0));">
  {#if skills.length === 0}
    <div class="flex h-full items-center justify-center text-[12px] text-[var(--color-text-secondary)]">
      No skills to graph
    </div>
  {:else}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <svg
      class="h-full w-full"
      style="cursor: {isPanning ? 'grabbing' : 'grab'}; touch-action: none;"
      viewBox={`0 0 ${VIEWPORT_W} ${VIEWPORT_H}`}
      preserveAspectRatio="xMidYMid meet"
      bind:this={svgEl}
      onwheel={handleWheel}
      onpointerdown={handlePointerDown}
      onpointermove={handlePointerMove}
      onpointerup={endPan}
      onpointercancel={endPan}
      onlostpointercapture={endPan}
    >
      <defs>
        <pattern id="graph-grid" width="36" height="36" patternUnits="userSpaceOnUse">
          <path d="M 36 0 L 0 0 0 36" fill="none" stroke="var(--color-border)" stroke-opacity="0.22" stroke-width="1" />
        </pattern>
      </defs>

      <rect x="0" y="0" width={VIEWPORT_W} height={VIEWPORT_H} fill="url(#graph-grid)" opacity="0.35" />

      <g transform={graphTransform}>
        {#each edges as edge (edge.from + "->" + edge.to)}
          {@const from = nodeById.get(edge.from)}
          {@const to = nodeById.get(edge.to)}
          {#if from && to}
            <line
              x1={from.x}
              y1={from.y}
              x2={to.x}
              y2={to.y}
              stroke={isEdgeActive(edge) ? "var(--color-accent)" : "var(--color-border-hover)"}
              stroke-width={isEdgeActive(edge) ? "2.1" : "1.05"}
              stroke-opacity={isEdgeActive(edge) ? "0.9" : "0.45"}
            />
          {/if}
        {/each}

        {#each nodes as node (node.skill.id)}
          {@const isSelected = selectedSkillId === node.skill.id}
          {@const color = store.getAgentColor(typeof node.skill.agentId === "string" ? node.skill.agentId : "universal") ?? DEFAULT_AGENT_COLOR}
          <g transform={`translate(${node.x} ${node.y})`} data-graph-node="true">
            <circle
              role="button"
              tabindex="0"
              aria-label={`Select ${node.skill.name}`}
              onclick={() => (selectedSkillId = isSelected ? null : node.skill.id)}
              onkeydown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  selectedSkillId = isSelected ? null : node.skill.id;
                }
              }}
              r={Math.max(nodeRadius(node) + 12, 20)}
              fill="transparent"
              data-graph-node="true"
              style="cursor:pointer;"
            />
            <circle
              r={nodeRadius(node) + (isSelected ? 4 : 0)}
              fill={color}
              opacity={isSelected ? "0.34" : "0.13"}
              data-graph-node="true"
            />
            <circle
              r={nodeRadius(node)}
              fill={isSelected ? "var(--color-accent)" : "var(--color-surface-3)"}
              stroke={color}
              stroke-width={isSelected ? "2.5" : "1.35"}
              data-graph-node="true"
            />
            {#if zoom >= 0.85 || isSelected}
              <text
                x={nodeRadius(node) + 7}
                y="3.5"
                font-size="11"
                fill="var(--color-text-secondary)"
                opacity={isSelected ? "0.96" : "0.83"}
                data-graph-node="true"
              >
                {node.skill.name.length > 26 ? `${node.skill.name.slice(0, 24)}..` : node.skill.name}
              </text>
            {/if}
          </g>
        {/each}
      </g>
    </svg>

    <div class="pointer-events-none absolute left-3 top-3 rounded-md border px-2.5 py-1.5 text-[10px]"
      style="border-color: var(--color-border); background: color-mix(in srgb, var(--color-surface-1) 85%, transparent); color: var(--color-text-secondary);">
      skill graph, click node to inspect
    </div>

    <div
      class="absolute bottom-3 left-3 flex items-center gap-1 rounded-md border px-1.5 py-1"
      style="border-color: var(--color-border); background: color-mix(in srgb, var(--color-surface-1) 90%, transparent);"
    >
      <button
        class="flex h-6 w-6 items-center justify-center rounded text-[14px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-3)]"
        onclick={zoomOut}
        aria-label="Zoom out"
        title="Zoom out"
      >
        -
      </button>
      <button
        class="flex h-6 min-w-[52px] items-center justify-center rounded px-2 text-[10px] font-semibold tabular-nums text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-3)]"
        onclick={resetView}
        aria-label="Reset graph view"
        title="Reset graph view"
      >
        {zoomPercent}%
      </button>
      <button
        class="flex h-6 w-6 items-center justify-center rounded text-[14px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-3)]"
        onclick={zoomIn}
        aria-label="Zoom in"
        title="Zoom in"
      >
        +
      </button>
    </div>

    {#if selectedSkill}
      <div class="absolute right-3 top-3 w-[300px] rounded-lg border p-3"
        style="border-color: var(--color-border); background: color-mix(in srgb, var(--color-surface-1) 92%, transparent);">
        <div class="mb-2 flex items-start justify-between gap-2">
          <div class="min-w-0">
            <div class="truncate text-[12px] font-semibold text-[var(--color-text-primary)]">{selectedSkill.name}</div>
            <div class="mt-0.5 truncate text-[10px] text-[var(--color-text-muted)]">
              {store.getAgentDisplayName(typeof selectedSkill.agentId === "string" ? selectedSkill.agentId : "universal")}
            </div>
          </div>
          <button
            class="h-5 w-5 rounded text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-3)]"
            onclick={() => (selectedSkillId = null)}
            aria-label="Close node details"
          >
            ×
          </button>
        </div>

        <p class="line-clamp-3 text-[10px] leading-relaxed text-[var(--color-text-secondary)]">
          {selectedSkill.description || "No description"}
        </p>

        <div class="mt-2 grid grid-cols-2 gap-1.5 text-[9px]">
          <div class="rounded border px-2 py-1"
            style="border-color: var(--color-border); background: var(--color-surface-2);">
            <span class="text-[var(--color-text-muted)]">parent</span>
            <div class="mt-0.5 font-semibold text-[var(--color-text-secondary)]">{hasParent(selectedSkill) ? "yes" : "no"}</div>
          </div>
          <div class="rounded border px-2 py-1"
            style="border-color: var(--color-border); background: var(--color-surface-2);">
            <span class="text-[var(--color-text-muted)]">children</span>
            <div class="mt-0.5 font-semibold text-[var(--color-text-secondary)]">{childCount(selectedSkill)}</div>
          </div>
        </div>

        {#if selectedSkill.metadata.trigger}
          <div class="mt-2 inline-flex rounded border px-1.5 py-0.5 text-[9px]"
            style="border-color: var(--color-border); background: var(--color-surface-2); color: var(--color-text-secondary);">
            {selectedSkill.metadata.trigger}
          </div>
        {/if}
      </div>
    {/if}
  {/if}
</div>
